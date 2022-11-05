use anyhow::Result;

use eframe::egui;
#[cfg(not(target_arch = "wasm32"))]
use eframe::{NativeOptions, Renderer};

use crate::ui;
use crate::Args;

const INITIAL_WIDTH: u32 = 1920;
const INITIAL_HEIGHT: u32 = 1080;

// TODO: just move the font loading into EmulatorUi::new()
fn app_creator(args: Args) -> eframe::AppCreator {
    log::debug!("app_creator (build)");
    Box::new(|cc| {

        log::debug!("AppCreator call");
        let mut fonts = egui::FontDefinitions::default();

        fonts.font_data.insert(
            "controller_emoji".to_owned(),
            egui::FontData::from_static(include_bytes!("../../../assets/fonts/controller-emoji.ttf")),
        );
        fonts
            .families
            .entry(egui::FontFamily::Name("Emoji".into()))
            .or_default()
            .insert(0, "controller_emoji".to_owned());
        // Set emoji font as fallback for proportional and monospace text:
        fonts
            .families
            .entry(egui::FontFamily::Proportional)
            .or_default()
            .push("controller_emoji".to_owned());
        fonts
            .families
            .entry(egui::FontFamily::Monospace)
            .or_default()
            .push("controller_emoji".to_owned());

        cc.egui_ctx.set_fonts(fonts);

        let emulator = ui::EmulatorUi::new(args, &cc.egui_ctx).expect("Failed to initialize Emulator UI");
        Box::new(emulator)
    })
}

#[cfg(not(target_arch = "wasm32"))]
pub fn native_ui_main(
    args: Args,
    mut options: NativeOptions,
) -> Result<()> {
    options.renderer = Renderer::Wgpu;
    eframe::run_native(
        "NES Emulator",
        options,
        app_creator(args));
    Ok(())
}

#[cfg(target_arch = "wasm32")]
pub fn web_ui_main(
    args: Args,
    canvas_id: &str,
) -> Result<()> {
    log::debug!("web_ui_main");
    let web_options = eframe::WebOptions::default();
    eframe::start_web(
        canvas_id,
        web_options,
        app_creator(args),
    ).expect("failed to start eframe");
    Ok(())
}