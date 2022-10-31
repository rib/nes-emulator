use anyhow::Result;

use eframe::egui;
use eframe::{NativeOptions, Renderer};

use crate::ui;
use crate::Args;

const INITIAL_WIDTH: u32 = 1920;
const INITIAL_HEIGHT: u32 = 1080;

pub fn ui_main(args: Args, mut options: NativeOptions) -> Result<()> {

    options.renderer = Renderer::Wgpu;
    eframe::run_native("NES Emulator", options, Box::new(|cc| {
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
    }));

    Ok(())
}
