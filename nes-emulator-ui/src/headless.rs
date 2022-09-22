use std::{time::{Instant, Duration}, rc::Rc, cell::RefCell, fs::File, io::{Write, BufWriter}, path::{Path, PathBuf}};

use anyhow::Result;
use nes_emulator::{nes::{ProgressTarget, Nes}, framebuffer::FramebufferInfo, system::Model};

use crate::{utils, benchmark::BenchmarkState, macros::{MacroPlayer, self}};

const DUMMY_AUDIO_SAMPLE_RATE: u32 = 48000;

fn progress_nes_emulation(nes: &mut Nes, stats: &mut BenchmarkState) -> bool {
    stats.start_update(&nes, Instant::now());

    let mut breakpoint = false;
    match nes.progress(ProgressTarget::FrameReady) {
        nes_emulator::nes::ProgressStatus::FrameReady => {
            stats.end_frame();
        },
        nes_emulator::nes::ProgressStatus::ReachedTarget => unreachable!(), // Should hit FrameReady first
        nes_emulator::nes::ProgressStatus::Breakpoint => {
            breakpoint = true;
        }
    }

    stats.end_update(&nes);

    breakpoint
}

fn save_check_failed_image(nes: &mut Nes, name: &String, expected_failure: bool) {
    let front = &nes.ppu_mut().framebuffer;
    let fb_width = front.width();
    let fb_height = front.height();

    let fb_buf = &front.data;
    let stride = fb_width * 4;
    let mut imgbuf = image::ImageBuffer::new(fb_width as u32, fb_height as u32);
    for (x, y, pixel) in imgbuf.enumerate_pixels_mut() {
        let x = x as usize;
        let y = y as usize;
        let r = fb_buf[stride * y + x * 4];
        let g = fb_buf[stride * y + x * 4 + 1];
        let b = fb_buf[stride * y + x * 4 + 2];
        let _a = fb_buf[stride * y + x * 4 + 3];
        *pixel = image::Rgb([r, g, b]);
    }
    let status = if expected_failure { "changed" } else { "failed" };
    let frame = nes.ppu_mut().frame;
    let line = nes.ppu_mut().line;
    let dot = nes.ppu_mut().dot;
    let filename = format!("test-{name}-{status}-frame-{}-line-{}-dot-{}.png", frame, line, dot);

    log::warn!("{} {}: Saving debug image: {}", name, status, filename);
    imgbuf.save(filename).unwrap();
}

fn setup_new_nes(rom_path: impl AsRef<Path>, rom_dirs: &Vec<PathBuf>, audio_sample_rate: u32, trace_file: Option<&String>) -> Result<Nes> {
    let rom_path = match utils::find_rom(&rom_path, rom_dirs) {
        Some(rom) => rom,
        None => {
            eprintln!("Failed to find ROM {}", rom_path.as_ref().to_string_lossy());
            std::process::exit(1);
        }
    };

    let mut nes = utils::create_nes_from_binary(rom_path, audio_sample_rate, Instant::now())?;

    if let Some(trace) = trace_file {
        if trace == "-" {
            nes.add_cpu_instruction_trace_hook(Box::new(move |_nes, trace_state| {
                println!("{trace_state}");
            }));
        } else {
            let f = File::create(trace)?;
            let mut writer = BufWriter::new(f);
            nes.add_cpu_instruction_trace_hook(Box::new(move |_nes, trace_state| {
                if let Err(err) = writeln!(writer, "{trace_state}") {
                    eprintln!("Failed to write to trace file: {err:?}");
                }
            }));
        }
    }

    Ok(nes)
}

pub fn run_macros(args: &crate::Args, rom_dirs: &Vec<PathBuf>, library: &String) -> Result<()> {
    let shared_crc32 = Rc::new(RefCell::new(0u32));

    let mut nes = Nes::new(Model::Ntsc, DUMMY_AUDIO_SAMPLE_RATE, Instant::now());
    let mut stats = BenchmarkState::new(&nes, Duration::from_secs(3));

    let mut macro_player = None;

    let mut macro_queue = macros::read_macro_library_from_file(library)?;
    macro_queue.reverse(); // We'll be playing by popping off the end


    loop {
        if macro_player.is_none() {
            if let Some(next_macro) = macro_queue.pop() {
                log::debug!("Starting macro {}", next_macro.name);

                nes = setup_new_nes(&next_macro.rom, &rom_dirs, DUMMY_AUDIO_SAMPLE_RATE, args.trace.as_ref())?;
                // To handle any CRC32 checks in the macro we register a hook that continuously tracks the CRC32 for every frame
                let _crc_hook_handle = macros::register_frame_crc_hasher(&mut nes, shared_crc32.clone());
                stats = BenchmarkState::new(&nes, Duration::from_secs(3));

                // macros run in headless mode are treated like tests and check failures are considered fatal
                let mut player = MacroPlayer::new(next_macro, &mut nes, shared_crc32.clone());
                player.set_check_failure_callback(Box::new(|nes, name, tags, _err| {
                    let expected_failure = tags.contains("test_failure");
                    save_check_failed_image(nes, name, expected_failure);
                    //panic!("{}", err);
                }));
                macro_player = Some(player);


            } else {
                log::debug!("Macro queue empty");
                break;
            }
        }

        let hit_breakpoint = progress_nes_emulation(&mut nes, &mut stats);

        if let Some(player) = &mut macro_player {
            if hit_breakpoint {
                player.check_breakpoint(&mut nes);
            }

            player.update(&mut nes);
        }

        if let Some(player) = &mut macro_player {
            if !player.playing() {
                if player.all_checks_passed() {
                    if player.checks_for_failure() {
                        log::warn!("FAILED (as expected): {}", player.name());
                        println!("FAILED (as expected): {}", player.name());
                    } else {
                        log::debug!("PASSED: {}", player.name());
                        println!("PASSED: {}", player.name());
                    }
                } else {
                    if player.checks_for_failure() {
                        log::warn!("UNKNOWN (didn't hit expected failure): {}", player.name());
                        println!("UNKNOWN (didn't hit expected failure): {}", player.name());
                    } else {
                        log::error!("FAILED: {}", player.name());
                        println!("FAILED: {}", player.name());
                    }
                }
                macro_player = None;
            }
        }
    }

    Ok(())
}

pub fn run_single_rom(args: &crate::Args, rom_dirs: &Vec<PathBuf>) -> Result<()> {
    let rom_path = match &args.rom {
        Some(rom) => utils::find_rom(rom, &rom_dirs),
        None => None
    };
    let rom_path = match rom_path {
        Some(path) => path,
        None => {
            eprintln!("A path to a ROM must be specified for benchmark mode");
            std::process::exit(1);
        }
    };

    let mut nes = setup_new_nes(rom_path, &rom_dirs, DUMMY_AUDIO_SAMPLE_RATE, args.trace.as_ref())?;
    let mut stats = BenchmarkState::new(&nes, Duration::from_secs(3));

    loop {
        progress_nes_emulation(&mut nes, &mut stats);
    }
}

pub fn headless_main(args: crate::Args) -> Result<()> {
    let rom_dirs = utils::canonicalize_rom_dirs(&args.rom_dir);

    if let Some(library) = &args.macros {
        run_macros(&args, &rom_dirs, library)?;
    } else {
        run_single_rom(&args, &rom_dirs)?;
    }

    Ok(())
}