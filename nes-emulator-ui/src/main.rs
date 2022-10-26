//#![allow(unused)]
// There's some kind of compiler bug going on, causing a crazy amount of false
// positives atm :(
#![allow(dead_code)]


use clap::Parser;

use anyhow::Result;

mod utils;
mod benchmark;
mod ui;
mod ui_winit;
mod headless;
mod macros;
mod view;

#[derive(Parser, Debug)]
#[clap(version, about, long_about = None)]
pub struct Args {
    rom: Option<String>,

    #[clap(short='t', long="trace", help="Record a trace of CPU instructions executed")]
    trace: Option<String>,

    #[clap(short='r', long="relative-time", help="Step emulator by relative time intervals, not necessarily keeping up with real time")]
    relative_time: bool,

    #[clap(short='q', long="headless", help="Disables any IO and all synchronization (i.e. emulates frames as quickly as possible; good for benchmarking and running tests)")]
    headless: bool,

    #[clap(short='m', long="macros", help="Load the macros in the given library")]
    macros: Option<String>,

    #[clap(short='p', long="play", help="Play a single macro or \"all\" to execute all loaded macros")]
    play_macros: Vec<String>,

    #[clap(short='d', long="rom-dir", help="Add a directory to find macro roms that are specified with a relative path")]
    rom_dir: Vec<String>,

    #[clap(short='g', long="genie", help="Game Genie Code")]
    genie_codes: Vec<String>
}

fn dispatch_main() -> Result<()> {
    let args = Args::parse();

    if args.headless {
        headless::headless_main(args)?;
    } else {
        ui_winit::ui_winit_main(args)?;
    }

    Ok(())
}

fn main() -> Result<()> {
    env_logger::builder().filter_level(log::LevelFilter::Debug) // Default Log Level
        .filter(Some("naga"), log::LevelFilter::Warn)
        .filter(Some("wgpu"), log::LevelFilter::Warn)
        .parse_default_env()
        .init();

    dispatch_main()
}