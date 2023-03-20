use instant::{Duration, Instant};
use std::collections::HashSet;
use std::path::PathBuf;
use std::str::FromStr;
use std::{
    cell::{Cell, RefCell},
    path::Path,
    rc::Rc,
};

use anyhow::Result;
use nes_emulator::{
    genie::GameGenieCode,
    hook::HookHandle,
    nes::Nes,
    port::ControllerButton,
    ppu::{DotBreakpointCallbackAction, DotBreakpointHandle},
};
use serde::{Deserialize, Serialize};

use crate::RomIdentifier;

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub struct MacroWait {
    pub frame: Option<u32>,
    pub line: Option<u16>,
    pub dot: u16,
}
impl MacroWait {
    /// Returns true if this wait will wait less than the given wait parameters
    /// Used to determine if a new wait would be redundant, if a macro will have already
    /// reached the same point in time.
    pub fn less_than(&self, other: &MacroWait) -> bool {
        if self.frame.unwrap_or(0) < other.frame.unwrap_or(0) {
            return true;
        }
        if self.line.unwrap_or(0) < other.line.unwrap_or(0) {
            return true;
        }
        self.dot < other.dot
    }
}

#[derive(Serialize, Deserialize, Debug, Copy, Clone)]
pub enum InputEvent {
    Pad {
        i: u8,
        b: u8,
        p: bool,
    },

    /// Zapper input with a more terse struct since there may be lots of position and light-level
    /// updates
    Zap {
        /// port
        i: u8,
        /// Screen X
        x: u16,
        /// Screen Y
        y: u16,
        /// Triggered 0 or 1
        t: u8,
        /// Light level
        l: u8,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum MacroCommand {
    Reset,
    WaitForDot(MacroWait),
    Input(InputEvent),

    /// While a macro is running there will CRC32 calculated for the framebuffer
    /// at the end of each frame on line 239, dot 255 when the last pixel is written
    /// which can be read at any time until the next frame starts at line 0, dot 0.
    CheckFrameCRC32(u32),
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Macro {
    pub name: String,
    pub rom: String,
    pub notes: String,

    #[serde(default)]
    pub genie_codes: Vec<String>,

    /// `true` if this test is known/expected to fail, and so a check "failure" implies
    /// some change in behaviour that should be investigated
    //pub fails: bool,

    #[serde(default)]
    pub tags: HashSet<String>,

    pub commands: Vec<MacroCommand>,
}

impl Macro {
    pub fn rom_id(&self) -> Option<RomIdentifier> {
        #[cfg(target_arch = "wasm32")]
        {
            Some(self.rom.clone())
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            match PathBuf::from_str(&self.rom) {
                Ok(path) => Some(path),
                Err(_) => {
                    log::error!("No valid rom ID associated with macro {}", self.name);
                    None
                }
            }
        }
    }
}

pub fn read_macro_library_from_file<P: AsRef<std::path::Path>>(
    path: P,
    filter: &[String],
) -> Result<Vec<Macro>> {
    let file = std::fs::File::open(path)?;
    let reader = std::io::BufReader::new(file);
    let library = serde_json::from_reader(reader)?;

    if filter.iter().any(|name| name == "all") {
        Ok(library)
    } else {
        let mut queue = vec![];
        for name in filter.iter() {
            let mut found = false;
            for m in library.iter() {
                if &m.name == name {
                    found = true;
                    queue.push(m.clone());
                }
                if !found {
                    log::warn!("No macro name \"{name}\" found in library");
                }
            }
        }
        Ok(queue)
    }
}

pub fn register_frame_crc_hasher(nes: &mut Nes, shared_crc32: Rc<RefCell<u32>>) -> HookHandle {
    let mut hasher = crc32fast::Hasher::new();

    nes.ppu_mut()
        .add_mux_hook(Box::new(move |_ppu, _cartridge, state| {
            if state.screen_x == 0 && state.screen_y == 0 {
                hasher.reset();
            }

            hasher.update(&[state.palette_value]);

            if state.screen_x == 255 && state.screen_y == 239 {
                let crc = hasher.clone().finalize();
                //println!("Frame {}, CRC = {:08x}", ppu.frame, crc);
                *shared_crc32.borrow_mut() = crc;
            }
        }))
}

pub fn name_from_rom_path(path: &Path, default_name: String) -> String {
    let components: Vec<String> = path
        .iter()
        .map(|c| c.to_string_lossy().to_string())
        .collect();

    if components.is_empty() {
        return default_name;
    }

    // TODO: use std::path::MAIN_SEPARATOR_STR
    // ref: https://github.com/rust-lang/rust/issues/94071
    if components[0] == std::path::MAIN_SEPARATOR.to_string() {
        return default_name;
    }

    let file = components.last().unwrap();
    let stripped = if file.ends_with(".nes") {
        file.strip_suffix(".nes").unwrap()
    } else {
        file.as_str()
    };

    if components.len() >= 2 {
        if stripped == components[0] {
            stripped.to_string()
        } else {
            format!("{}:{stripped}", components[0])
        }
    } else {
        stripped.to_string()
    }
}

type MacroCheckFailureCallback = Box<dyn FnMut(&mut Nes, &String, &HashSet<String>, String)>;

pub struct MacroPlayer {
    recording: Macro,
    all_checks_passed: bool,
    command: Cell<usize>,
    shared_crc32: Rc<RefCell<u32>>,
    //waiting_for_dot: bool,
    wait_breakpoint: Option<DotBreakpointHandle>,
    wait_update_timestamp: Instant,
    check_failure_callback: Option<MacroCheckFailureCallback>,
}
impl MacroPlayer {
    pub fn new(recording: Macro, nes: &mut Nes, shared_crc32: Rc<RefCell<u32>>) -> Self {
        let genie_codes: Vec<GameGenieCode> = recording
            .genie_codes
            .iter()
            .filter_map(|c| {
                let code: Result<GameGenieCode> = c.as_str().try_into();
                match code {
                    Ok(c) => Some(c),
                    Err(err) => {
                        log::error!("Ignoring Game Genie Code {c} - {}", err);
                        None
                    }
                }
            })
            .collect();

        nes.set_game_genie_codes(genie_codes);

        Self {
            recording,
            all_checks_passed: true,
            command: Cell::new(0),
            shared_crc32,
            wait_update_timestamp: Instant::now(),
            wait_breakpoint: None,
            check_failure_callback: None,
        }
    }

    pub fn next(&self) -> bool {
        self.command.replace(self.command.get() + 1);
        self.playing()
    }

    pub fn playing(&self) -> bool {
        self.command.get() < self.recording.commands.len()
    }

    fn current_cmd(&self) -> Option<&MacroCommand> {
        if self.command.get() < self.recording.commands.len() {
            Some(&self.recording.commands[self.command.get()])
        } else {
            None
        }
    }

    pub fn set_check_failure_callback(&mut self, callback: MacroCheckFailureCallback) {
        self.check_failure_callback = Some(callback);
    }

    pub fn all_checks_passed(&self) -> bool {
        self.all_checks_passed
    }

    pub fn checks_for_failure(&self) -> bool {
        self.recording.tags.contains("test_failure")
    }

    pub fn name(&self) -> &String {
        &self.recording.name
    }

    /// Returns true if the player was expecting to break at this point
    /// (so the emulator shouldn't be paused)
    pub fn check_breakpoint(&mut self, nes: &mut Nes) -> bool {
        //println!("Macro: checking breakpoint");
        if !self.playing() {
            //println!("Macro: checking breakpoint: not playing");
            return false;
        }

        if let MacroCommand::WaitForDot(target) = self.recording.commands[self.command.get()] {
            if let Some(target_frame) = target.frame {
                if nes.ppu_mut().frame != target_frame {
                    //println!("Macro: checking breakpoint: not required frame");
                    return false;
                }
            }
            if let Some(target_line) = target.line {
                if nes.ppu_mut().line != target_line {
                    //println!("Macro: checking breakpoint: not required line");
                    return false;
                }
            }
            if nes.ppu_mut().dot == target.dot {
                self.next();
                //println!("Macro: Finished waiting for dot");
                if let Some(bp) = self.wait_breakpoint {
                    nes.ppu_mut().remove_dot_breakpoint(bp);
                    self.wait_breakpoint = None;
                } else {
                    unreachable!();
                }
                return true;
            } else {
                //println!("Macro: checking breakpoint: not required dot {} != target {}", nes.ppu_mut().dot, target.dot);
            }
        }

        false
    }

    pub fn update(&mut self, nes: &mut Nes) {
        if self.wait_breakpoint.is_some() {
            //println!("Macro: Continuing to wait for dot");
            let duration = Instant::now() - self.wait_update_timestamp;
            if duration > Duration::from_secs(3) {
                let current_frame = nes.ppu_mut().frame;
                let current_line = nes.ppu_mut().line;
                let current_dot = nes.ppu_mut().dot;
                log::debug!(
                    "Waiting: frame: {current_frame}, line = {current_line}, dot = {current_dot}"
                );
                self.wait_update_timestamp = Instant::now();
            }
            return;
        }

        // Keep processing commands until we reach a WaitForDot command
        while self.playing() {
            log::debug!("Macro command: {}", self.command.get());
            match self.recording.commands[self.command.get()] {
                MacroCommand::Reset => {
                    log::debug!("Macro: Reset event");
                    nes.reset();
                    self.next();
                }
                MacroCommand::WaitForDot(target) => {
                    if self.wait_breakpoint.is_none() {
                        self.wait_breakpoint = Some(nes.ppu_mut().add_dot_breakpoint(
                            target.frame,
                            target.line,
                            target.dot,
                            Box::new(|_, _, _, _| DotBreakpointCallbackAction::Remove),
                        ));
                        self.wait_update_timestamp = Instant::now();
                        log::debug!(
                            "Macro: Started to wait for frame = {:?}, line = {:?}, dot = {}",
                            &target.frame,
                            &target.line,
                            target.dot
                        );
                        break;
                    }
                }
                MacroCommand::Input(event) => {
                    //println!("Macro: input event: {event:?}");
                    match event {
                        InputEvent::Pad { i, b, p } => {
                            let port_index = i;
                            let port = if port_index == 0 {
                                &mut nes.system_mut().port1
                            } else {
                                &mut nes.system_mut().port2
                            };
                            match ControllerButton::try_from(b) {
                                Ok(button) => {
                                    let pressed = p;
                                    if pressed {
                                        port.press_button(button);
                                    } else {
                                        port.release_button(button);
                                    }
                                }
                                Err(_) => {
                                    log::error!("Unknown input button ID = {b} in macro");
                                }
                            }
                        }
                        InputEvent::Zap { .. } => {
                            log::error!("Unsupported Zapper input");
                        }
                    }
                    self.next();
                }
                MacroCommand::CheckFrameCRC32(crc) => {
                    //println!("Macro: checking for framebuffer CRC32 = {crc:08x}");
                    let current_crc = *self.shared_crc32.borrow();
                    if current_crc != crc {
                        self.all_checks_passed = false;
                        let err = format!("Macro: CRC check failed!: CRC32 = {current_crc:08x}, expected CRC32 was {crc:08x}");
                        log::error!("{err}");
                        if let Some(callback) = self.check_failure_callback.as_mut() {
                            callback(nes, &self.recording.name, &self.recording.tags, err);
                        }
                    }
                    self.next();
                }
            }
        }
    }
}
