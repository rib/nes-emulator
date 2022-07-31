use std::{path::PathBuf, cell::RefCell, rc::Rc, collections::HashMap, str::FromStr, time::Instant, fs::File, io::Write};

use egui::{Vec2, TextEdit, Layout, Align};
use egui_extras::{TableBuilder, Size};
use nes_emulator::{port::ControllerButton, hook::HookHandle, nes::Nes};

use crate::{macros::{MacroCommand, InputEvent, Macro, MacroWait, MacroPlayer, self}, Args, utils, ui::{EmulatorUi, ViewRequest, ViewRequestSender}};

struct MacroBuilderHookState {
    crc32: u32,
}

pub struct MacroBuilderView {
    pub visible: bool,

    view_request_sender: ViewRequestSender,

    rom_dirs: Vec<PathBuf>,
    default_rom: Option<PathBuf>,

    library: Vec<Macro>,
    library_path: Option<PathBuf>,
    current_macro: usize,

    paused: bool,
    /// Waiting to be notified that the ROM has been loaded and the NES has been power cycled
    recording_pending: bool,
    recording: bool,

    // A special case for when we have stopped recording and the emulator
    // state hasn't changed, so we can start recording again to append
    // to the end.
    can_append: bool,

    last_wait: MacroWait,

    hook_handle: Option<HookHandle>,
    hook_state: Rc<RefCell<MacroBuilderHookState>>,

    /// Pending button state changes, relative to the current Nes state
    /// Flushed when another command progresses the Nes CPU clock
    pending_button_input: HashMap<ControllerButton, bool>,
}

impl MacroBuilderView {
    pub fn new(_ctx: &egui::Context, args: &Args, rom_dirs: Vec<PathBuf>, default_rom: Option<PathBuf>, view_request_sender: ViewRequestSender, paused: bool) -> Self {

        let mut view = Self  {
            visible: false,
            view_request_sender,

            rom_dirs,
            default_rom,

            library_path: None,
            library: vec![],
            current_macro: 0,

            paused,
            recording: false,
            recording_pending: false,
            can_append: false,

            last_wait: MacroWait { frame: None, line: None, dot: 0 },

            hook_handle: None,
            hook_state: Rc::new(RefCell::new(MacroBuilderHookState {
                crc32: 0
            })),

            pending_button_input: HashMap::new(),
        };

        if let Some(macros) = &args.macros {
            let path = PathBuf::from_str(macros).unwrap();
            view.open_macros_library(path);
        }

        view
    }

    /// Each time a new NES is loaded (typically whenever a new ROM is loaded) then we
    /// need to re-install our PPU hook for calculating the CRC32 for macros that
    /// check the framebuffer.
    ///
    /// This will also be called when the view becomes visible
    pub fn power_on_new_nes_hook(&mut self, nes: &mut Nes, loaded_rom: Option<&PathBuf>) {
        if loaded_rom.is_some() {
            self.default_rom = loaded_rom.cloned();
        }

        // We don't want the cost of calculating the framebuffer CRC if the macro
        // builder isn't currently in use.
        if !self.visible {
            return;
        }

        let shared = self.hook_state.clone();
        let mut hasher = crc32fast::Hasher::new();

        self.hook_handle = Some(nes.ppu_mut().add_mux_hook(Box::new(
            move |_ppu, _cartridge, state| {
                if state.screen_x == 0 && state.screen_y == 0 {
                    hasher.reset();
                }

                hasher.update(&[state.palette_value]);

                if state.screen_x == 255 && state.screen_y == 239 {
                    shared.borrow_mut().crc32 = hasher.clone().finalize();
                }
        })));
    }

    pub fn disconnect_nes(&mut self, nes: &mut Nes) {
        if let Some(handle) = self.hook_handle {
            nes.ppu_mut().remove_mux_hook(handle);
            self.hook_handle = None;
        }
    }

    /// Called in response to each ViewRequest::LoadRom request that's sent
    /// Note: this isn't called in case the user loads a rom via top level File->Open menu
    pub fn load_rom_request_finished(&mut self, success: bool) {
        if self.recording_pending {
            self.recording_pending = false;
            if success {
                self.recording = true;
            }
        }
    }

    // TODO: give feedback about progress through the macro playback
    pub fn started_playback(&mut self, _nes: &mut Nes, _player: &MacroPlayer) {

    }

    // TODO: give feedback about progress through the macro playback
    pub fn playback_update(&mut self, _nes: &mut Nes, _player: &MacroPlayer) {

    }

    fn create_new_library(&mut self) {
        self.library = vec![];
        self.library_path = None;
    }

    fn open_macros_library(&mut self, path: PathBuf) {
        match macros::read_macro_library_from_file(&path) {
            Ok(library) => {
                self.library = library;
                self.library_path = Some(path);
            }
            Err(err) => self.view_request_sender.send(ViewRequest::ShowUserNotice(log::Level::Error, format!("{err:#?}")))
        }
    }

    /*
    fn search_open_macros_library(&mut self, path: PathBuf) {
        match macros::search_read_macro_library(path, &self.rom_dirs) {
            Ok((library, path)) => {
                self.library = library;
                self.library_path = Some(path);
            }
            Err(err) => self.view_request_sender.send(ViewRequest::ShowUserNotice(log::Level::Error, format!("{err:#?}")))
        }
    }*/

    fn open_macros_library_dialog(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("json", &["json"])
            .pick_file()
        {
            self.open_macros_library(path);
        }
    }

    fn save_macros_library(&self, path: &PathBuf) {
        match File::create(path) {
            Ok(mut fd) => {
                match serde_json::to_string_pretty(&self.library) {
                    Ok(js) => {
                        if let Err(err) = fd.write(&js.as_bytes()) {
                            self.view_request_sender.send(ViewRequest::ShowUserNotice(log::Level::Error, format!("{}", err)))
                        }
                    }
                    Err(err) => {
                        log::error!("Failed to serialize test: {err:?}");
                    }
                }
            }
            Err(err) => {
                self.view_request_sender.send(ViewRequest::ShowUserNotice(log::Level::Error, format!("{}", err)))
            }
        }
    }

    pub fn save(&self) {
        if let Some(path) = &self.library_path {
            self.save_macros_library(path);
        }
    }

    fn save_macros_library_dialog(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("json", &["json"])
            .save_file()
        {
            self.save_macros_library(&path);
        }
    }

    pub fn controller_input_hook(&mut self, nes: &mut Nes, button: ControllerButton, pressed: bool) {
        if self.recording {
            if self.paused {
                self.pending_button_input.remove(&button);
                self.pending_button_input.insert(button, pressed);
            } else {

                // Ignore redundant input events for key repeat
                if nes.system_mut().port1.peek_button(button) != pressed {
                    self.record_input_command(nes, button, pressed);
                }
            }
        }
    }

    pub fn set_visible(&mut self, nes: &mut Nes, visible: bool) {
        self.visible = visible;
        #[cfg(feature="macro-builder")]
        if visible {
            self.power_on_new_nes_hook(nes, None);

        } else {
            if let Some(handle) = self.hook_handle {
                nes.ppu_mut().remove_mux_hook(handle);
                self.hook_handle = None;
            }
        }
    }

    fn start_recording(&mut self, nes: &mut Nes, clear_first: bool) {
        debug_assert_eq!(self.recording, false);

        self.last_wait = MacroWait {
            frame: None,
            line: None,
            dot: 0
        };
        let current_macro = &mut self.library[self.current_macro];

        nes.power_cycle(Instant::now());
        if clear_first {
            current_macro.commands.clear();
        } else {
            debug_assert_eq!(self.can_append, true);
        }

        self.view_request_sender.send(ViewRequest::LoadRom(current_macro.rom.clone()));
        self.recording_pending = true;
    }

    fn stop_recording(&mut self) {
        debug_assert_eq!(self.recording, true);
        self.recording = false;
        self.can_append = true;
    }

    fn record_input_command(&mut self, nes: &mut Nes, button: ControllerButton, pressed: bool) {
        debug_assert!(self.current_macro < self.library.len());
        let test = &mut self.library[self.current_macro];

        log::debug!("controller input: recording = {}, paused = {}", self.recording, self.paused);
        let wait = MacroWait {
            frame: Some(nes.ppu_mut().frame),
            line: Some(nes.ppu_mut().line),
            dot: nes.ppu_mut().dot
        };
        if self.last_wait.less_than(&wait) {
            test.commands.push(MacroCommand::WaitForDot(wait));
            self.last_wait = wait;
        }
        test.commands.push(MacroCommand::Input(InputEvent::Pad {
            i: 0, // input port
            b: button as u8,
            p: pressed
        }));
    }

    // Input changes are buffered while paused so we don't end up recording lots of redundant input changes
    // within a single cycle
    pub fn set_paused(&mut self, paused: bool, nes: &mut Nes) {

        self.paused = paused;

        if !paused {
            self.can_append = false;
        }

        const BUTTONS: [ControllerButton; 8] = [
            ControllerButton::A,
            ControllerButton::B,
            ControllerButton::Select,
            ControllerButton::Start,
            ControllerButton::Up,
            ControllerButton::Down,
            ControllerButton::Left,
            ControllerButton::Right,
        ];

        if self.recording {
            if !paused {
                log::debug!("Flushing pending recording input");
                for button in BUTTONS {
                    if let Some(pressed) = self.pending_button_input.get(&button) {
                        if nes.system_mut().port1.peek_button(button) != *pressed {

                            if *pressed {
                                nes.system_mut().port1.press_button(button);
                            } else {
                                nes.system_mut().port1.release_button(button);
                            }

                            self.record_input_command(nes, button, *pressed);
                        }
                    }
                }
                self.pending_button_input.clear();
            }
        }
    }


    pub fn update(&mut self, _nes: &mut Nes) {
        /*
        let bpp = 3;
        let stride = FRAME_WIDTH * bpp;

        for y in 0..FRAME_HEIGHT {
            for x in 0..FRAME_WIDTH {
                let pix = nes.debug_sample_sprites(x, y);
                let pos = y * stride + x * bpp;
                self.screen_framebuffer[pos + 0] = pix[0];
                self.screen_framebuffer[pos + 1] = pix[1];
                self.screen_framebuffer[pos + 2] = pix[2];
            }
        }

        */
    }

    pub fn draw(&mut self, nes: &mut Nes, ctx: &egui::Context) {

        egui::Window::new("Macro Builder")
            .default_width(900.0)
            .resizable(true)
            //.resize(|r| r.auto_sized())
            .show(ctx, |ui| {

                egui::TopBottomPanel::top("macros_menu_bar").show_inside(ui, |ui| {
                    ui.add_enabled_ui(!self.recording, |ui| {
                        egui::menu::bar(ui, |ui| {
                            ui.menu_button("File", |ui| {
                                if ui.button("New Library").clicked() {
                                    ui.close_menu();
                                    self.create_new_library();
                                }

                                if ui.button("Open").clicked() {
                                    ui.close_menu();
                                    self.open_macros_library_dialog();
                                }

                                ui.add_enabled_ui(self.library_path.is_some(), |ui| {
                                    if ui.button("Save").clicked() {
                                        ui.close_menu();
                                        self.save();
                                    }
                                });
                                ui.add_enabled_ui(self.library.len() > 0, |ui| {
                                    if ui.button("Save As...").clicked() {
                                        ui.close_menu();
                                        self.save_macros_library_dialog();
                                    }
                                })
                            });
                        });
                    });
                });

                let panel_width = ui.fonts().pixels_per_point() * 150.0;

                egui::TopBottomPanel::bottom("macro_builder_footer").frame(egui::Frame::none()).show_inside(ui, |ui| {

                    ui.horizontal(|ui| {

                        let height = ui.fonts().pixels_per_point() * 10.0;
                        let labels_width = ui.fonts().pixels_per_point() * 100.0;

                        ui.allocate_ui(Vec2::new(labels_width, height), |ui| {
                            ui.set_min_width(labels_width);
                            let frame = nes.ppu_mut().frame;
                            let label = egui::Label::new(&format!("Frame: {frame}")).sense(egui::Sense::click());
                            ui.add(label).context_menu(|ui| {
                                if ui.button("Copy").clicked() {
                                    ui.output().copied_text = format!("{frame}");
                                    ui.close_menu();
                                }
                            });
                        });

                        ui.allocate_ui(Vec2::new(labels_width, height), |ui| {
                            ui.set_min_width(labels_width);
                            let cycle = nes.cpu_clock();
                            let label = egui::Label::new(&format!("CPU Cycle: {cycle}")).sense(egui::Sense::click());
                            ui.add(label).context_menu(|ui| {
                                if ui.button("Copy").clicked() {
                                    ui.output().copied_text = format!("{cycle}");
                                    ui.close_menu();
                                }
                            });
                        });

                        ui.allocate_ui(Vec2::new(labels_width, height), |ui| {
                            ui.set_min_width(labels_width);
                            let dot = nes.ppu_mut().dot;
                            let label = egui::Label::new(&format!("Line Dot: {dot}")).sense(egui::Sense::click());
                            ui.add(label).context_menu(|ui| {
                                if ui.button("Copy").clicked() {
                                    ui.output().copied_text = format!("{dot}");
                                    ui.close_menu();
                                }
                            });
                        });

                        ui.allocate_ui(Vec2::new(labels_width, height), |ui| {
                            ui.set_min_width(labels_width);
                            let crc = self.hook_state.borrow().crc32;
                            let crc_text = format!("{crc:08x}");
                            let label = egui::Label::new(&format!("Frame CRC: {crc_text}")).sense(egui::Sense::click());
                            ui.add(label).context_menu(|ui| {
                                if ui.button("Copy").clicked() {
                                    ui.output().copied_text = crc_text;
                                    ui.close_menu();
                                }
                            });
                        });
                    });
                });

                egui::SidePanel::left("macro_builder_options_panel")
                    //.resizable(false)
                    .min_width(panel_width)
                    .show_inside(ui, |ui| {

                            ui.horizontal(|ui| {
                                if ui.button("New Macro").clicked() {
                                    self.library.push(Macro {
                                        ..Default::default()
                                    });
                                    self.current_macro = self.library.len() - 1;
                                    let default_name = format!("Macro {}", self.library.len() - 1);
                                    if let Some(rom) = &self.default_rom {
                                        if let Ok(relative) = utils::find_shortest_rom_path(rom, &self.rom_dirs) {
                                            self.library[self.current_macro].rom = relative.to_string_lossy().to_string();
                                        } else {
                                            self.library[self.current_macro].rom = rom.to_string_lossy().to_string();
                                        }
                                        self.library[self.current_macro].name = macros::name_from_rom_path(rom, default_name);
                                    } else {
                                        self.library[self.current_macro].name = default_name;
                                    }
                                }

                                if self.library.len() > 0 {
                                    let current_macro = self.current_macro;
                                    egui::ComboBox::from_id_source("macro_name_drop_down")
                                        .width(250.0)
                                        .selected_text(&self.library[current_macro].name)
                                        .show_ui(ui, |ui| {
                                            for i in 0..self.library.len() {
                                                ui.selectable_value(&mut self.current_macro, i, &self.library[i].name);
                                            }
                                        }
                                    );
                                }
                            });
                        ui.separator();

                        if self.current_macro < self.library.len() {
                            let current_macro = &mut self.library[self.current_macro];

                            ui.add(TextEdit::singleline(&mut current_macro.name).hint_text("Name"));
                            ui.horizontal(|ui| {

                                ui.add(TextEdit::singleline(&mut current_macro.rom).hint_text("ROM Path"));
                                if ui.button("📁").clicked() {
                                    if let Some(path) = EmulatorUi::pick_rom_dialog() {
                                        if let Ok(relative) = utils::find_shortest_rom_path(&path, &self.rom_dirs) {
                                            current_macro.rom = relative.to_string_lossy().to_string();
                                            current_macro.name = macros::name_from_rom_path(&relative, current_macro.name.clone());
                                        } else {
                                            current_macro.rom = path.to_string_lossy().to_string();
                                        }
                                    }
                                }
                            });

                            //ui.text_edit_multiline(&mut self.test.notes);
                            ui.separator();
                            ui.label("Tags");
                            ui.group(|ui| {
                                ui.with_layout(Layout::top_down_justified(Align::Min), |ui| {
                                    const TAGS: [&str; 7] = ["cpu", "dma", "apu", "ppu", "mapper", "input", "test_failure"];
                                    for tag in TAGS.into_iter() {
                                        let mut tagged = current_macro.tags.contains(tag);
                                        if ui.toggle_value(&mut tagged, tag).changed() {
                                            if tagged {
                                                current_macro.tags.insert(tag.to_string());
                                            } else {
                                                current_macro.tags.remove(tag);
                                            }
                                        }
                                    }
                                });
                            });
                            ui.separator();
                            ui.add_enabled_ui(self.recording, |ui| {
                                ui.label("Commands");
                                ui.group(|ui| {
                                    ui.with_layout(Layout::top_down_justified(Align::Min), |ui| {

                                        if ui.button("Add Frame CRC32 Check").clicked() {
                                            let wait = MacroWait {
                                                frame: Some(nes.ppu_mut().frame),
                                                line: Some(nes.ppu_mut().line),
                                                dot: nes.ppu_mut().dot
                                            };
                                            if self.last_wait.less_than(&wait) {
                                                current_macro.commands.push(MacroCommand::WaitForDot(wait));
                                                self.last_wait = wait;
                                            }
                                            current_macro.commands.push(MacroCommand::CheckFrameCRC32(self.hook_state.borrow().crc32));
                                        }
                                        if ui.button("Add Reset").clicked() {
                                            let wait = MacroWait {
                                                frame: Some(nes.ppu_mut().frame),
                                                line: Some(nes.ppu_mut().line),
                                                dot: nes.ppu_mut().dot
                                            };
                                            if self.last_wait.less_than(&wait) {
                                                current_macro.commands.push(MacroCommand::WaitForDot(wait));
                                                self.last_wait = wait;
                                            }
                                            current_macro.commands.push(MacroCommand::Reset);
                                            nes.reset();
                                        }
                                        ui.separator();
                                        ui.add_enabled_ui(self.paused, |ui| {
                                            ui.horizontal(|ui| {
                                                let button = ControllerButton::Left;
                                                let mut pressed = self.pending_button_input.get(&button).copied().unwrap_or_else(|| {
                                                    nes.system_mut().port1.peek_button(button)
                                                });
                                                if ui.toggle_value(&mut pressed, "🡄").changed() {
                                                    self.pending_button_input.insert(button, pressed);
                                                }
                                                let button = ControllerButton::Up;
                                                let mut pressed = self.pending_button_input.get(&button).copied().unwrap_or_else(|| {
                                                    nes.system_mut().port1.peek_button(button)
                                                });
                                                if ui.toggle_value(&mut pressed, "🡅").changed() {
                                                    self.pending_button_input.insert(button, pressed);
                                                }
                                                let button = ControllerButton::Down;
                                                let mut pressed = self.pending_button_input.get(&button).copied().unwrap_or_else(|| {
                                                    nes.system_mut().port1.peek_button(button)
                                                });
                                                if ui.toggle_value(&mut pressed, "🡇").changed() {
                                                    self.pending_button_input.insert(button, pressed);
                                                }
                                                let button = ControllerButton::Right;
                                                let mut pressed = self.pending_button_input.get(&button).copied().unwrap_or_else(|| {
                                                    nes.system_mut().port1.peek_button(button)
                                                });
                                                if ui.toggle_value(&mut pressed, "🡆").changed() {
                                                    self.pending_button_input.insert(button, pressed);
                                                }
                                                let button = ControllerButton::Select;
                                                let mut pressed = self.pending_button_input.get(&button).copied().unwrap_or_else(|| {
                                                    nes.system_mut().port1.peek_button(button)
                                                });
                                                if ui.toggle_value(&mut pressed, "Select").changed() {
                                                    self.pending_button_input.insert(button, pressed);
                                                }
                                                let button = ControllerButton::Start;
                                                let mut pressed = self.pending_button_input.get(&button).copied().unwrap_or_else(|| {
                                                    nes.system_mut().port1.peek_button(button)
                                                });
                                                if ui.toggle_value(&mut pressed, "Start").changed() {
                                                    self.pending_button_input.insert(button, pressed);
                                                }
                                                let button = ControllerButton::A;
                                                let mut pressed = self.pending_button_input.get(&button).copied().unwrap_or_else(|| {
                                                    nes.system_mut().port1.peek_button(button)
                                                });
                                                if ui.toggle_value(&mut pressed, "Ⓐ").changed() {
                                                    self.pending_button_input.insert(button, pressed);
                                                }
                                                let button = ControllerButton::B;
                                                let mut pressed = self.pending_button_input.get(&button).copied().unwrap_or_else(|| {
                                                    nes.system_mut().port1.peek_button(button)
                                                });
                                                if ui.toggle_value(&mut pressed, "Ⓑ").changed() {
                                                    self.pending_button_input.insert(button, pressed);
                                                }
                                            });
                                        });
                                    });
                                });
                            });
                        }

                        //ui.checkbox(&mut view.show_scroll, "Show Scroll Position");
                    });

                egui::TopBottomPanel::top("macro_header").frame(egui::Frame::none()).show_inside(ui, |ui| {
                    if self.current_macro < self.library.len() {
                        ui.horizontal(|ui| {
                            if !self.recording {
                                if self.library[self.current_macro].commands.len() > 0 {
                                    if ui.button("Play").clicked() {
                                        self.view_request_sender.send(ViewRequest::RunMacro(self.library[self.current_macro].clone()));
                                    }
                                }

                                // XXX: to avoid losing data maybe create a new macro based on the current macro
                                // instead of over writing the current macro
                                if ui.button("Record").clicked() {
                                    self.start_recording(nes, true);
                                }
                                if self.can_append {
                                    if ui.button("Append").clicked() {
                                        self.start_recording(nes, false);
                                    }
                                }

                                if ui.button("Delete").clicked() {
                                    self.library.remove(self.current_macro);
                                    if self.current_macro >= self.library.len() {
                                        self.current_macro = 0;
                                    }
                                }
                            } else {
                                if ui.button("Stop").clicked() {
                                    self.stop_recording();
                                }
                            }
                        });
                    }
                });

                egui::CentralPanel::default()
                    //.frame(frame)
                    .show_inside(ui, |ui| {

                        if self.current_macro < self.library.len() {
                            let current_macro = &mut self.library[self.current_macro];

                            let mut to_delete = vec![];

                            TableBuilder::new(ui)
                                .column(Size::exact(300.0))
                                .column(Size::exact(15.0)) // delete icon
                                .body(|body| {
                                    body.rows(15.0, current_macro.commands.len(), |row_index, mut row| {
                                        row.col(|ui| {
                                            match &current_macro.commands[row_index] {
                                                MacroCommand::Reset => {
                                                    ui.label("Reset NES");
                                                }
                                                MacroCommand::WaitForDot(wait) => {
                                                    if let Some(frame) = wait.frame {
                                                        ui.label(format!("Wait for frame {frame}, line = {}, dot = {}",
                                                            wait.line.map(|l| l.to_string()).unwrap_or_else(|| "any".to_string()),
                                                            wait.dot,
                                                        ));
                                                    } else if let Some(line) = wait.line {
                                                        ui.label(format!("Wait for line {line}, dot = {}", wait.dot));
                                                    } else {
                                                        ui.label(format!("Wait for scan line dot {}", wait.dot));
                                                    }
                                                }
                                                MacroCommand::Input(event) => {
                                                    match event {
                                                        InputEvent::Pad { i, b, p } => {
                                                            if let Ok(button) =  ControllerButton::try_from(*b) {
                                                                if *p { // pressed
                                                                    ui.label(format!("Port {i}, Press {:#?}", button));
                                                                } else {
                                                                    ui.label(format!("Port {i}, Release {:#?}", button));
                                                                }
                                                            } else {
                                                                ui.label("Invalid input event");
                                                            }
                                                        }
                                                        InputEvent::Zap { i, x, y, t, l } => {
                                                            let triggered = *t == 1;
                                                            ui.label(format!("Port {i}, Zapper: x = {}, y = {}, trig = {}, light = {}", *x, *y, triggered, *l));
                                                        },
                                                    }
                                                }
                                                MacroCommand::CheckFrameCRC32(crc) => {
                                                    ui.label(format!("Check framebuffer CRC32 == {crc:08x}"));
                                                }
                                            }
                                        });

                                        row.col(|ui| {
                                            if ui.button("🗑️").clicked() {
                                                to_delete.push(row_index);
                                                println!("Delete command {row_index}");
                                            }
                                        });

                                    });
                                });

                            for i in to_delete.into_iter() {
                                current_macro.commands.remove(i);
                            }
                        }
                });
        });
    }
}