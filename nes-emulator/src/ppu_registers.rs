use crate::ppu::Ppu;

use bitflags::bitflags;

bitflags! {
    #[derive(Default)]
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
    #[derive(Default)]
    pub struct Control2Flags: u8 {
        const EMPHASIZE_BLUE        = 0b1000_0000;
        const EMPHASIZE_GREEN       = 0b0100_0000;
        const EMPHASIZE_RED         = 0b0010_0000;
        const SHOW_SPRITES          = 0b0001_0000;
        const SHOW_BG               = 0b0000_1000;
        const SPRITES_LEFT_COL_SHOW = 0b0000_0100;
        const BG_LEFT_COL_SHOW      = 0b0000_0010;
        const MONOCHROME            = 0b0000_0001;
    }
}

bitflags! {
    #[derive(Default)]
    pub struct StatusFlags: u8 {
        const IN_VBLANK             = 0b1000_0000;
        const SPRITE0_HIT           = 0b0100_0000;
        const SPRITE_OVERFLOW       = 0b0010_0000;

        // For open bus handling
        const UNDEFINED_BITS        = 0b0001_1111;
    }
}

impl Ppu {
    pub fn sprite_height(&self) -> u8 {
        if self.control1.contains(Control1Flags::SPRITE_HEIGHT_16) { 16 } else { 8 }
    }

    pub fn bg_pattern_table_addr(&self) -> u16 {
        if self.control1.contains(Control1Flags::BG_IN_PATTERN_TABLE_1) { 0x1000 } else { 0x0000 }
    }
    pub fn sprites_pattern_table_addr(&self) -> u16 {
        if self.control1.contains(Control1Flags::SPRITES_IN_PATTERN_TABLE_1) { 0x1000 } else { 0x0000 }
    }

    pub fn address_increment(&self) -> u16 {
        if self.control1.contains(Control1Flags::ADDRESS_INC_32) { 32 } else { 1 }
    }

}
