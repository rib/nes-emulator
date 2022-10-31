

use anyhow::Result;
use clap::Parser;

use nes_emulator_shell as nes_shell;

fn main() -> Result<()> {
    env_logger::builder()
        .filter_level(log::LevelFilter::Debug) // Default Log Level
        .filter(Some("naga"), log::LevelFilter::Warn)
        .filter(Some("wgpu"), log::LevelFilter::Warn)
        .parse_default_env()
        .init();

    let args = nes_shell::Args::parse();

    let options = if !args.headless {
        Some(eframe::NativeOptions::default())
    } else {
        None
    };

    nes_shell::dispatch_main(args, options)
}