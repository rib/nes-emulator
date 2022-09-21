use std::fmt::Debug;

use egui::{RichText, vec2};
use egui_extras::{Size, StripBuilder};
use nes_emulator::{nes::Nes, apu::channel::square_channel::SquareChannel};

pub struct ApuView {
    pub visible: bool,
}
impl ApuView {
    pub fn new() -> Self {
        Self {
            visible: false,
        }
    }

    fn draw_square_channel_props(&mut self,
        props_id_src: impl std::hash::Hash,
        sweep_props_id_src: impl std::hash::Hash,
        len_props_id_src: impl std::hash::Hash,
        ui: &mut egui::Ui, channel: &mut SquareChannel) {

        egui::Grid::new(props_id_src).show(ui, |ui| {
            ui.label("Period");
            let mut period = channel.period();
            if ui.add(egui::DragValue::new(&mut period).clamp_range(0..=(1<<11))).changed() {
                channel.set_period(period);
            }
            ui.end_row();

            ui.label("Timer");
            let mut timer = channel.timer();
            if ui.add(egui::DragValue::new(&mut timer).clamp_range(0..=period)).changed() {
                channel.set_timer(timer);
            }
            ui.end_row();

            ui.label("Frequency");
            let mut freq = channel.frequency();
            if ui.add(egui::DragValue::new(&mut freq)).changed() {
                channel.set_frequency(freq);
            }
            ui.end_row();

            ui.label("Duty");
            ui.add(egui::DragValue::new(&mut channel.duty).clamp_range(0..=3));
            ui.end_row();
            ui.label("Duty Position");
            ui.add(egui::DragValue::new(&mut channel.duty_offset ).clamp_range(0..=3));
            ui.end_row();
        });
        ui.separator();
        ui.heading("Sweep");
        ui.checkbox(&mut channel.sweep_enabled, "Enabled");
        ui.checkbox(&mut channel.sweep_negate, "Negate");
        egui::Grid::new(sweep_props_id_src).show(ui, |ui| {
            ui.label("Period");
            ui.add(egui::DragValue::new(&mut channel.sweep_divider_period).clamp_range(1..=8));
            ui.end_row();
            ui.label("Shift");
            ui.add(egui::DragValue::new(&mut channel.sweep_shift).clamp_range(0..=7));
            ui.end_row();
        });
        ui.separator();
        ui.heading("Length Counter");
        let mut enabled = channel.length_counter.enabled();
        if ui.checkbox(&mut enabled, "Enabled").changed() {
            channel.length_counter.set_enabled(enabled);
        }
        ui.checkbox(&mut channel.length_counter.halt, "Halt");
        egui::Grid::new(len_props_id_src).show(ui, |ui| {
            ui.label("Counter");
            ui.label(format!("{}", channel.length_counter.length()));
            ui.end_row();

        });
    }

    pub fn draw(&mut self, nes: &mut Nes, ctx: &egui::Context) {
        egui::Window::new("APU")
            .fixed_size(vec2(800.0, 300.0))
            //.resizable(true)
            .show(ctx, |ui| {
                StripBuilder::new(ui)
                    .size(Size::relative(0.2).at_least(60.0))
                    .size(Size::relative(0.2).at_least(60.0))
                    .size(Size::relative(0.2).at_least(60.0))
                    .size(Size::relative(0.2).at_least(60.0))
                    .size(Size::relative(0.2).at_least(60.0))
                    .horizontal(|mut strip| {
                //ui.horizontal(|ui| {

                //})
                //egui::Grid::new("apu_units_grid").show(ui, |ui| {
                    strip.cell(|ui| {
                        ui.vertical(|ui| {
                            let mut name_text = egui::WidgetText::from("Square 1").heading();
                            if nes.system_mut().apu.square_channel1.length_counter.enabled() {
                                name_text = name_text.background_color(egui::Color32::YELLOW);
                            }
                            ui.add(egui::Label::new(name_text));
                            //ui.heading("Square 1");
                            ui.toggle_value(&mut nes.system_mut().apu.mixer.square1_muted, "Mute");
                            self.draw_square_channel_props(
                                "apu_square1_props",
                                "apu_square1_sweep_props",
                                "apu_square1_length_props",
                                ui, &mut nes.system_mut().apu.square_channel1);
                        });
                    });

                    strip.cell(|ui| {
                    //ui.group(|ui| {
                        ui.vertical(|ui| {
                            let mut name_text = egui::WidgetText::from("Square 2").heading();
                            if nes.system_mut().apu.square_channel2.length_counter.enabled() {
                                name_text = name_text.background_color(egui::Color32::YELLOW);
                            }
                            ui.add(egui::Label::new(name_text));
                            //ui.heading("Square 1");
                            ui.toggle_value(&mut nes.system_mut().apu.mixer.square2_muted, "Mute");
                            self.draw_square_channel_props(
                                "apu_square2_props",
                                "apu_square2_sweep_props",
                                "apu_square2_length_props",
                                ui, &mut nes.system_mut().apu.square_channel2);
                        });
                    });

                    strip.cell(|ui| {
                    //ui.group(|ui| {
                        ui.vertical(|ui| {
                            ui.heading("Triangle");
                            ui.toggle_value(&mut nes.system_mut().apu.mixer.triangle_muted, "Mute");
                            let mut enabled = nes.system_mut().apu.triangle_channel.length_counter.enabled();
                            if ui.checkbox(&mut enabled, "Enabled").changed() {
                                nes.system_mut().apu.triangle_channel.length_counter.set_enabled(enabled);
                            }
                            egui::Grid::new("apu_triangle_props").show(ui, |ui| {
                                ui.label("Period");
                                let mut period = nes.system_mut().apu.triangle_channel.period();
                                if ui.add(egui::DragValue::new(&mut period).clamp_range(0..=(1<<11))).changed() {
                                    nes.system_mut().apu.triangle_channel.set_period(period);
                                }
                                ui.end_row();
                                ui.label("Frequency");
                                let mut freq = nes.system_mut().apu.triangle_channel.frequency();
                                if ui.add(egui::DragValue::new(&mut freq)).changed() {
                                    nes.system_mut().apu.triangle_channel.set_frequency(freq);
                                }
                                ui.end_row();
                            });
                        });
                    });

                    strip.cell(|ui| {
                    //ui.group(|ui| {
                        ui.vertical(|ui| {
                            ui.heading("Noise");
                            ui.toggle_value(&mut nes.system_mut().apu.mixer.noise_muted, "Mute");
                            let mut enabled = nes.system_mut().apu.noise_channel.length_counter.enabled();
                            if ui.checkbox(&mut enabled, "Enabled").changed() {
                                nes.system_mut().apu.noise_channel.length_counter.set_enabled(enabled);
                            }
                            egui::Grid::new("apu_noise_props").show(ui, |ui| {

                            });
                        });
                    });

                    strip.cell(|ui| {
                    //ui.group(|ui| {
                        ui.vertical(|ui| {
                            let mut name_text = egui::WidgetText::from("DMC").heading();
                            if nes.system_mut().apu.dmc_channel.is_active() {
                                name_text = name_text.background_color(egui::Color32::YELLOW);
                            }
                            ui.add(egui::Label::new(name_text));

                            ui.toggle_value(&mut nes.system_mut().apu.mixer.dmc_muted, "DMC Mute");
                            ui.checkbox(&mut nes.system_mut().apu.dmc_channel.loop_flag, "Loop");
                            ui.checkbox(&mut nes.system_mut().apu.dmc_channel.interrupt_enable, "IRQ Enabled");
                            egui::Grid::new("apu_dmc_props").show(ui, |ui| {

                            });
                        });
                    });
                    //ui.end_row();
                });
         });
    }
}