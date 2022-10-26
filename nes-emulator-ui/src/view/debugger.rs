use egui::vec2;
use egui_extras::{Size, StripBuilder};
use nes_emulator::nes::Nes;

use crate::ui::{ViewRequest, ViewRequestSender};

pub struct InstructionLine {
    disassembly: String,
}

pub struct DebuggerView {
    pub visible: bool,
    paused: bool,
    view_request_sender: ViewRequestSender,
}

impl DebuggerView {
    pub fn new(view_request_sender: ViewRequestSender, paused: bool) -> Self {
        Self {
            visible: false,
            paused,
            view_request_sender,
        }
    }

    pub fn set_paused(&mut self, paused: bool, _nes: &mut Nes) {
        self.paused = paused;
    }

    pub fn draw_disassembly(&mut self, _nes: &mut Nes, _ctx: &egui::Context) {}
    pub fn draw(&mut self, nes: &mut Nes, ctx: &egui::Context) {
        egui::Window::new("Debugger")
            .default_size(vec2(1024.0, 1024.0))
            .resizable(true)
            .show(ctx, |ui| {
                egui::SidePanel::right("debugger_tools_panel").show_inside(ui, |ui| {
                    ui.vertical_centered_justified(|ui| {
                        if self.paused {
                            if ui.button("Step In").clicked() {
                                self.view_request_sender
                                    .send(ViewRequest::InstructionStepIn);
                            }
                            if ui.button("Step Over").clicked() {
                                self.view_request_sender
                                    .send(ViewRequest::InstructionStepOver);
                            }
                            if ui.button("Step Out").clicked() {
                                self.view_request_sender
                                    .send(ViewRequest::InstructionStepOut);
                            }
                        }
                    });
                });

                egui::CentralPanel::default().show_inside(ui, |ui| {
                    StripBuilder::new(ui)
                        .size(Size::relative(0.85))
                        .size(Size::relative(0.15))
                        .vertical(|mut strip| {
                            strip.strip(|builder| {
                                builder
                                    //.size(Size::remainder())
                                    .size(Size::relative(0.5))
                                    .size(Size::relative(0.5))
                                    .horizontal(|mut strip| {
                                        // Disassembly
                                        strip.cell(|ui| {
                                            egui::ScrollArea::vertical().show(ui, |ui| {
                                                // Add a lot of widgets here.
                                                let pc = nes.cpu_mut().pc;
                                                let (instruction, operand) =
                                                    nes.peek_instruction(pc);

                                                ui.label(format!(
                                                    "{}",
                                                    instruction.disassemble(
                                                        operand.raw_operand,
                                                        operand.operand
                                                    )
                                                ));
                                            });
                                        });

                                        // State views
                                        strip.cell(|_ui| {
                                            //ui.group(|ui| {
                                            //ui.vertical(|ui| {

                                            //});
                                        });
                                    });
                            });

                            strip.strip(|builder| {
                                builder
                                    .cell_layout(egui::Layout::top_down(egui::Align::Min))
                                    .size(Size::relative(0.33))
                                    .size(Size::remainder())
                                    .size(Size::relative(0.33))
                                    .horizontal(|mut strip| {
                                        // Watch points
                                        strip.cell(|ui| {
                                            ui.heading("Watch points");
                                            ui.push_id("debugger_watchpoints", |ui| {
                                                ui.group(|ui| {
                                                    egui::ScrollArea::vertical().show(ui, |ui| {
                                                        ui.set_min_size(ui.available_size());
                                                        for wp in nes
                                                            .system_mut()
                                                            .debug
                                                            .watch_points
                                                            .iter()
                                                        {
                                                            let addr = wp.address;
                                                            ui.label(format!("{addr:04x}"));
                                                        }
                                                    });
                                                });
                                            });
                                        });

                                        // Breakpoints
                                        strip.cell(|ui| {
                                            ui.heading("Breakpoints");
                                            ui.push_id("debugger_breakpoints", |ui| {
                                                ui.group(|ui| {
                                                    egui::ScrollArea::vertical().show(ui, |ui| {
                                                        ui.set_min_size(ui.available_size());
                                                        for bp in nes.cpu_mut().breakpoints() {
                                                            let addr = bp.address();
                                                            ui.label(format!("{addr:04x}"));
                                                        }
                                                    });
                                                });
                                            });
                                        });

                                        // Stack
                                        strip.cell(|ui| {
                                            ui.heading("Stack");
                                            ui.push_id("debugger_backtrace", |ui| {
                                                ui.group(|ui| {
                                                    egui::ScrollArea::vertical().show(ui, |ui| {
                                                        ui.set_min_size(ui.available_size());
                                                        //ui.set_min_width(ui.available_width());
                                                        for (addr, _tag) in nes.backtrace() {
                                                            ui.label(format!("{addr:04x}"));
                                                        }
                                                    });
                                                });
                                            });
                                        });
                                    });
                            });
                        });
                });
            });
    }
}
