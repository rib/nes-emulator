use std::fmt::Debug;

use egui_extras::{Size, TableBuilder};
use nes_emulator::nes::Nes;

pub struct InstructionLine {
    disassembly: String
}

pub struct DebuggerView {
    pub visible: bool,
}
impl DebuggerView {
    pub fn new() -> Self {
        Self {
            visible: false,
        }
    }

    pub fn draw(&mut self, nes: &mut Nes, ctx: &egui::Context) {
        egui::Window::new("Memory View")
            .resizable(true)
            .show(ctx, |ui| {

                egui::SidePanel::right("debugger_state_panel").show_inside(ui, |ui| {


                });

                let bytes_per_row = 16;
                let num_rows: usize = (1<<16) / bytes_per_row;
                let n_val_cols = bytes_per_row;


                let gutter_col_width = Size::exact(10.0);
                let addr_col_width = Size::exact(60.0);
                let code_col_width = Size::exact(300.0);
                let text_view_padding = Size::exact(100.0);
                let row_height_sans_spacing = 30.0;
                //let num_rows = 20;
                let mut tb = TableBuilder::new(ui)
                    .column(gutter_col_width)
                    .column(addr_col_width)
                    .column(code_col_width);

                tb
                    .header(30.0, |mut header| {
                        header.col(|ui| {
                            ui.heading("    ");
                        });

                        for i in 0..n_val_cols {
                            header.col(|ui| {
                                ui.heading(format!("{i:02x}"));
                            });
                        }

                        header.col(|ui| {
                            ui.heading("     ");
                        });

                        for _ in 0..n_val_cols {
                            header.col(|ui| {
                                ui.heading(" ");
                            });
                        }
                    })
                    .body(|body| {
                        body.rows(row_height_sans_spacing, num_rows, |row_index, mut row| {
                            row.col(|ui| {
                                ui.heading(format!("{:04x}", row_index * bytes_per_row));
                            });


                        });
                    });
         });
    }
}