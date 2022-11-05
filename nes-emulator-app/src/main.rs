

use anyhow::Result;
use clap::Parser;

use nes_emulator_shell as nes_shell;

#[cfg(not(target_arch = "wasm32"))]
fn main() -> Result<()> {
    env_logger::builder()
        .filter_level(log::LevelFilter::Debug) // Default Log Level
        .filter(Some("naga"), log::LevelFilter::Warn)
        .filter(Some("wgpu"), log::LevelFilter::Warn)
        .parse_default_env()
        .init();

    let args = nes_shell::Args::parse();

    if args.headless {
        nes_shell::headless::headless_main(args)?;
    } else {
        nes_shell::ui::eframe::native_ui_main(args, eframe::NativeOptions::default())?;
    }

    Ok(())
}


// when compiling to web using trunk.
#[cfg(target_arch = "wasm32")]
fn main() {
    // Make sure panics are logged using `console.error`.
    console_error_panic_hook::set_once();

    // Redirect tracing to console.log and friends:
    tracing_wasm::set_as_global_default();
    wasm_logger::init(wasm_logger::Config::default());
    log::debug!("Test 1");

    let args = nes_shell::Args::parse();

    nes_shell::ui::eframe::web_ui_main(args, "nes_emulator_canvas");
}