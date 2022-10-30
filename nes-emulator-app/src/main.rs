

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

    let event_loop = if !args.headless {
        let event_loop: winit::event_loop::EventLoop<nes_shell::ui::winit::Event> =
            winit::event_loop::EventLoopBuilder::with_user_event().build();
        Some(event_loop)
    } else {
        None
    };

    nes_shell::dispatch_main(args, event_loop)
}