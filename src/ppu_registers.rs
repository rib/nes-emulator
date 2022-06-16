use crate::ppu::Ppu;

use super::system::*;

use bitflags::bitflags;

pub const PPU_OAMADDR_OFFSET: usize = 0x03;
pub const PPU_OAMDATA_OFFSET: usize = 0x04;
pub const PPU_SCROLL_OFFSET: usize = 0x05;
pub const PPU_ADDR_OFFSET: usize = 0x06;
pub const PPU_DATA_OFFSET: usize = 0x07;
pub const APU_IO_OAM_DMA_OFFSET: usize = 0x14;

bitflags! {
    pub struct Control1Flags: u8 {
        const NMI_ENABLE            = 0b1000_0000;
        const IS_MASTER             = 0b0100_0000;
        const SPRITE_HEIGHT_16      = 0b0010_0000;

        const BG_IN_PATTERN_TABLE_1         = 0b0001_0000;
        const SPRITES_IN_PATTERN_TABLE_1    = 0b0000_1000;

        const ADDRESS_INC_32        = 0b0000_0100;

        const NAME_TABLE_MASK       = 0b0000_0011;
        const NAME_TABLE_0          = 0b0000_0000;
        const NAME_TABLE_1          = 0b0000_0001;
        const NAME_TABLE_2          = 0b0000_0010;
        const NAME_TABLE_3          = 0b0000_0011;
    }
}

bitflags! {
    pub struct Control2Flags: u8 {
        const COLOR_INTENSITY_MASK  = 0b1110_0000;
        const SPRITES_LEFT_COL_SHOW = 0b0001_0000;
        const BG_LEFT_COL_SHOW      = 0b0000_1000;
        const SHOW_SPRITES          = 0b0000_0100;
        const SHOW_BG               = 0b0000_0010;
        const MONOCHROME            = 0b0000_0001;
    }
}

bitflags! {
    pub struct StatusFlags: u8 {
        const IN_VBLANK             = 0b1000_0000;
        const SPRITE0_HIT           = 0b0100_0000;
        const SPRITE_OVERFLOW       = 0b0010_0000;
        //const VRAM_WRITE_DISABLE  = 0b0001_0000;

        // When reading, the non-status bits will come from the io latch value
        const UNDEFINED_BITS         = 0b0001_111;
    }
}


/// PPU Registers
/// 0x2000 - 0x2007
impl Ppu {
    /*************************** 0x2000: PPUCTRL ***************************/

    pub fn sprite_height(&self) -> u8 {
        if self.control1.contains(Control1Flags::SPRITE_HEIGHT_16) { 16 } else { 8 }
    }

    pub fn bg_pattern_table_addr(&self) -> u16 {
        if self.control1.contains(Control1Flags::BG_IN_PATTERN_TABLE_1) { 0x1000 } else { 0x0000 }
    }
    pub fn sprites_pattern_table_addr(&self) -> u16 {
        if self.control1.contains(Control1Flags::SPRITES_IN_PATTERN_TABLE_1) { 0x1000 } else { 0x0000 }
    }
    pub fn name_table_base_addr(&self) -> u16 {
        match self.control1 & Control1Flags::NAME_TABLE_MASK {
            Control1Flags::NAME_TABLE_0 => 0x2000,
            Control1Flags::NAME_TABLE_1 => 0x2400,
            Control1Flags::NAME_TABLE_2 => 0x2800,
            Control1Flags::NAME_TABLE_3 => 0x2c00,
            _ => panic!("invalid name table addr index"),
        }
    }

    pub fn address_increment(&self) -> u16 {
        if self.control1.contains(Control1Flags::ADDRESS_INC_32) { 32 } else { 1 }
    }

    /*************************** 0x2003: OAMADDR ***************************/
    pub fn read_ppu_oam_addr(&self) -> u8 {
        self.ppu_reg[PPU_OAMADDR_OFFSET]
    }
    /*************************** 0x2004: OAMDATA ***************************/
    /// A flag indicating whether OAM_DATA has been rewritten is also attached
    /// (it will volatilize automatically).
    /// is_read, is_write, data
    pub fn read_oam_data(&mut self) -> (bool, bool, u8) {
        // Write優先でフラグ管理して返してあげる
        if self.written_oam_data {
            self.written_oam_data = false;
            (false, true, self.ppu_reg[PPU_OAMDATA_OFFSET])
        } else if self.read_oam_data {
            self.read_oam_data = false;
            (true, false, self.ppu_reg[PPU_OAMDATA_OFFSET])
        } else {
            (false, false, self.ppu_reg[PPU_OAMDATA_OFFSET])
        }
    }

    pub fn write_oam_data(&mut self, data: u8) {
        self.ppu_reg[PPU_OAMDATA_OFFSET] = data;
    }

    /*************************** 0x2005: PPUSCROLL ***************************/
    /// (Flag indicating if there was an x, y update, x, y)
    pub fn read_ppu_scroll(&mut self) -> (bool, u8, u8) {
        if self.written_ppu_scroll {
            self.written_ppu_scroll = false;
            (true, self.ppu_reg[PPU_SCROLL_OFFSET], self.ppu_scroll_y_reg)
        } else {
            (
                false,
                self.ppu_reg[PPU_SCROLL_OFFSET],
                self.ppu_scroll_y_reg,
            )
        }
    }
    /*************************** 0x2006: PPUADDR ***************************/
    pub fn read_ppu_addr(&mut self) -> (bool, u16) {
        let addr =
            (u16::from(self.ppu_reg[PPU_ADDR_OFFSET]) << 8) | u16::from(self.ppu_addr_lower_reg);
        if self.written_ppu_addr {
            self.written_ppu_addr = false;
            (true, addr)
        } else {
            (false, addr)
        }
    }
    /*************************** 0x2007: PPUDATA ***************************/
    /// returns: is_read, is_write, data
    /// read/write is not true at the same time
    /// read: Put the value indicated by PPU_ADDR in PPU_DATA non-destructively and increment the address (it will naturally become post-fetch)
    /// write: Assign the value of PPU_DATA to PPU_ADDR (PPU space) and increment the address
    pub fn read_ppu_data(&mut self) -> (bool, bool, u8) {
        // Write優先でフラグ管理して返してあげる
        if self.written_ppu_data {
            self.written_ppu_data = false;
            (false, true, self.ppu_reg[PPU_DATA_OFFSET])
        } else if self.read_ppu_data {
            self.read_ppu_data = false;
            (true, false, self.ppu_reg[PPU_DATA_OFFSET])
        } else {
            (false, false, self.ppu_reg[PPU_DATA_OFFSET])
        }
    }

    /// Rewrite but do not auto-increment
    pub fn write_ppu_data(&mut self, data: u8) {
        self.ppu_reg[PPU_DATA_OFFSET] = data;
    }

    /// Performs PPU_ADDR automatic addition when reading and writing to PPU_DATA
    pub fn increment_ppu_addr(&mut self) {
        let current_addr =
            (u16::from(self.ppu_reg[PPU_ADDR_OFFSET]) << 8) | u16::from(self.ppu_addr_lower_reg);
        let add_val = self.address_increment();
        let dst_addr = current_addr.wrapping_add(add_val);
        // 分解して入れておく
        self.ppu_addr_lower_reg = (dst_addr & 0xff) as u8;
        self.ppu_reg[PPU_ADDR_OFFSET] = (dst_addr >> 8) as u8;
    }

}
