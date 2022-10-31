//#![allow(unused)]
// There's some kind of compiler bug going on, causing a crazy amount of false
// positives atm :(
#![allow(dead_code)]

use clap::Parser;

use anyhow::Result;

pub mod headless;
pub mod ui;
mod benchmark;
mod macros;
mod utils;

#[derive(Parser, Debug, Default)]
#[clap(version, about, long_about = None)]
pub struct Args {
    pub rom: Option<String>,

    #[clap(
        short = 't',
        long = "trace",
        help = "Record a trace of CPU instructions executed"
    )]
    pub trace: Option<String>,

    #[clap(
        short = 'r',
        long = "relative-time",
        help = "Step emulator by relative time intervals, not necessarily keeping up with real time"
    )]
    pub relative_time: bool,

    #[clap(
        short = 'q',
        long = "headless",
        help = "Disables any IO and all synchronization (i.e. emulates frames as quickly as possible; good for benchmarking and running tests)"
    )]
    pub headless: bool,

    #[clap(
        short = 'm',
        long = "macros",
        help = "Load the macros in the given library"
    )]
    pub macros: Option<String>,

    #[clap(
        short = 'p',
        long = "play",
        help = "Play a single macro or \"all\" to execute all loaded macros"
    )]
    pub play_macros: Vec<String>,

    #[clap(
        long = "results",
        help = "Write the results of running macros to the given file in JSON format"
    )]
    pub results_json: Option<String>,

    #[clap(
        short = 'd',
        long = "rom-dir",
        help = "Add a directory to find macro roms that are specified with a relative path"
    )]
    pub rom_dir: Vec<String>,

    #[clap(short = 'g', long = "genie", help = "Game Genie Code")]
    pub genie_codes: Vec<String>,
}

pub fn dispatch_main(args: Args, options: Option<eframe::NativeOptions>) -> Result<()> {
    if args.headless {
        headless::headless_main(args)?;
    } else {
        crate::ui::eframe::ui_main(args, options.unwrap())?;
    }

    Ok(())
}


