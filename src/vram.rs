use crate::cartridge;

use super::cartridge::*;
use super::interface::*;

pub const PATTERN_TABLE_BASE_ADDR: u16 = 0x0000;
pub const NAME_TABLE_BASE_ADDR: u16 = 0x2000;
pub const NAME_TABLE_MIRROR_BASE_ADDR: u16 = 0x3000;
pub const PALETTE_TABLE_BASE_ADDR: u16 = 0x3f00;
pub const VIDEO_ADDRESS_SIZE: u16 = 0x4000;

pub const NAME_TABLE_SIZE: usize = 0x0400;
pub const NUM_OF_NAME_TABLE: usize = 2;
pub const ATTRIBUTE_TABLE_SIZE: u16 = 0x0040;
pub const ATTRIBUTE_TABLE_OFFSET: u16 = 0x03c0; // NameTable+3c0で属性テーブル

pub const PALETTE_SIZE: usize = 0x20;
pub const PALETTE_ENTRY_SIZE: u16 = 0x04;
pub const PALETTE_BG_OFFSET: u16 = 0x00;
pub const PALETTE_SPRITE_OFFSET: u16 = 0x10;

#[derive(Clone)]
pub struct VRam {
    pub nametables: [[u8; NAME_TABLE_SIZE]; NUM_OF_NAME_TABLE],
}

impl Default for VRam {
    fn default() -> Self {
        Self {
            nametables: [[0; NAME_TABLE_SIZE]; NUM_OF_NAME_TABLE],
        }
    }
}

impl EmulateControl for VRam {
    fn poweron(&mut self) {
        self.nametables = [[0; NAME_TABLE_SIZE]; NUM_OF_NAME_TABLE];
    }
}

impl VRam {
    fn mirror_name_table_addr(&self, mirror_mode: NameTableMirror, addr: u16) -> (usize, usize) {
        debug_assert!(addr >= NAME_TABLE_BASE_ADDR);

        let offset = usize::from(addr - NAME_TABLE_BASE_ADDR) % NAME_TABLE_SIZE;
        let table_index = match mirror_mode {
            NameTableMirror::Horizontal => {
                // [A, A]
                // [B, B]
                if addr < 0x2800 {
                    0
                } else {
                    1
                }
            }
            NameTableMirror::Vertical => {
                // [A, B]
                // [A, B]
                let tmp_addr = if addr >= 0x2800 { addr - 0x800 } else { addr };
                if tmp_addr < 0x2400 {
                    0
                } else {
                    1
                }
            }
            NameTableMirror::SingleScreen => {
                // [A, A]
                // [A, A]
                0
            }
            NameTableMirror::FourScreen => {
                // [A, B]
                // [C, D]
                usize::from((addr - 0x2000) / 4)
            }
            _ => {
                unimplemented!();
            }
        };
        (table_index, offset)
    }

    pub fn read_u8(&self, cartridge: &mut Cartridge, addr: u16) -> u8 {
        debug_assert!(addr < VIDEO_ADDRESS_SIZE);

        match addr {
            0x0000..=0x1fff => {
                cartridge.read_video_u8(addr)
            }
            0x2000..=0x3fff => {
                let mirror_mode = cartridge.nametable_mirror;
                let (index, offset) = self.mirror_name_table_addr(mirror_mode, addr);
                self.nametables[index][offset]
            }
            _ => unreachable!()
        }
    }

    pub fn write_u8(&mut self, cartridge: &mut Cartridge, addr: u16, data: u8) {
        debug_assert!(addr < VIDEO_ADDRESS_SIZE);

        match addr {
            0x0000..=0x1fff => {
                cartridge.write_video_u8(addr, data);
            }
            0x2000..=0x3fff => {
                let mirror_mode = cartridge.nametable_mirror;
                let (index, offset) = self.mirror_name_table_addr(mirror_mode, addr);
                self.nametables[index][offset] = data;
            }
            _ => unreachable!()
        }
    }
}
