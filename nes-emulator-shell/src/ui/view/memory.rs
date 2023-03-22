use std::fmt::Debug;

use egui_extras::{Column, TableBuilder};
use nes_emulator::nes::Nes;

#[derive(PartialEq, Eq)]
enum AddressSpace {
    System,
    Ppu,
    Oam,
}
impl Debug for AddressSpace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::System => write!(f, "System Bus"),
            Self::Ppu => write!(f, "PPU Bus"),
            Self::Oam => write!(f, "OAM Memory"),
        }
    }
}

pub struct MemView {
    pub visible: bool,
    selected_space: AddressSpace,
    tmp_row_values: Vec<u8>,
}
impl MemView {
    pub fn new() -> Self {
        Self {
            visible: false,
            selected_space: AddressSpace::System,
            tmp_row_values: vec![],
        }
    }

    pub fn draw(&mut self, nes: &mut Nes, ctx: &egui::Context) {
        egui::Window::new("Memory View")
            .resizable(true)
            .show(ctx, |ui| {
                egui::SidePanel::left("memview_options_panel").show_inside(ui, |ui| {
                    egui::ComboBox::from_label("address_space")
                        .selected_text(format!("{:?}", self.selected_space))
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut self.selected_space,
                                AddressSpace::System,
                                "System Bus",
                            );
                            ui.selectable_value(
                                &mut self.selected_space,
                                AddressSpace::Ppu,
                                "PPU Bus",
                            );
                            ui.selectable_value(
                                &mut self.selected_space,
                                AddressSpace::Oam,
                                "OAM Memory",
                            );
                        });
                });

                let bytes_per_row = 16;
                let num_rows: usize = (1 << 16) / bytes_per_row;
                let n_val_cols = bytes_per_row;

                let addr_col_width = Column::exact(60.0);
                let val_col_width = Column::exact(30.0);
                let char_col_width = Column::exact(10.0);
                let text_view_padding = Column::exact(100.0);
                let row_height_sans_spacing = 30.0;
                //let num_rows = 20;
                let mut tb = TableBuilder::new(ui).column(addr_col_width);

                for _ in 0..n_val_cols {
                    tb = tb.column(val_col_width);
                }

                tb = tb.column(text_view_padding);

                for _ in 0..n_val_cols {
                    tb = tb.column(char_col_width);
                }

                tb.header(30.0, |mut header| {
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

                        self.tmp_row_values.clear();
                        for i in 0..n_val_cols {
                            let addr = row_index * bytes_per_row + i;
                            let val = match self.selected_space {
                                AddressSpace::System => nes.peek_system_bus(addr as u16),
                                AddressSpace::Ppu => nes.peek_ppu_bus(addr as u16),
                                AddressSpace::Oam => nes.ppu_mut().peek_oam_data(addr as u8),
                            };
                            self.tmp_row_values.push(val);
                            row.col(|ui| {
                                ui.label(format!("{:02x}", val));
                            });
                        }
                        row.col(|ui| {
                            ui.label(" ");
                        });
                        for i in 0..n_val_cols {
                            let val = self.tmp_row_values[i];
                            row.col(|ui| {
                                if val.is_ascii_alphanumeric() {
                                    ui.label(format!("{}", val as char));
                                } else {
                                    ui.label(".");
                                }
                            });
                        }
                    });
                });
            });
    }
}
