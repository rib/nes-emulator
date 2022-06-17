use crate::ppu::Ppu;

use super::system::*;

pub const PPU_CTRL_OFFSET: usize = 0x00;
pub const PPU_MASK_OFFSET: usize = 0x01;
pub const PPU_STATUS_OFFSET: usize = 0x02;
pub const PPU_OAMADDR_OFFSET: usize = 0x03;
pub const PPU_OAMDATA_OFFSET: usize = 0x04;
pub const PPU_SCROLL_OFFSET: usize = 0x05;
pub const PPU_ADDR_OFFSET: usize = 0x06;
pub const PPU_DATA_OFFSET: usize = 0x07;
pub const APU_IO_OAM_DMA_OFFSET: usize = 0x14;

/// PPU Registers
/// 0x2000 - 0x2007
impl Ppu {
    /*************************** 0x2000: PPUCTRL ***************************/
    /// VBLANK発生時にNMI割り込みを出す
    /// oneshotではなく0x2002のVLANKフラグがある限り
    pub fn read_ppu_nmi_enable(&self) -> bool {
        (self.ppu_reg[PPU_CTRL_OFFSET] & 0x80u8) == 0x80u8
    }
    /// 多分エミュだと使わない
    pub fn read_ppu_is_master(&self) -> bool {
        (self.ppu_reg[PPU_CTRL_OFFSET] & 0x40u8) == 0x40u8
    }
    /// 8もしくは16
    pub fn read_ppu_sprite_height(&self) -> u8 {
        if (self.ppu_reg[PPU_CTRL_OFFSET] & 0x20u8) == 0x20u8 {
            16
        } else {
            8
        }
    }
    pub fn read_ppu_bg_pattern_table_addr(&self) -> u16 {
        if (self.ppu_reg[PPU_CTRL_OFFSET] & 0x10u8) == 0x10u8 {
            0x1000u16
        } else {
            0x0000u16
        }
    }
    pub fn read_ppu_sprite_pattern_table_addr(&self) -> u16 {
        if (self.ppu_reg[PPU_CTRL_OFFSET] & 0x08u8) == 0x08u8 {
            0x1000u16
        } else {
            0x0000u16
        }
    }
    /// PPUのアドレスインクリメント数 0:+1, horizontal, 1:+32 vertical
    pub fn read_ppu_addr_increment(&self) -> u8 {
        if (self.ppu_reg[PPU_CTRL_OFFSET] & 0x04u8) == 0x04u8 {
            32u8
        } else {
            1u8
        }
    }
    pub fn read_ppu_name_table_base_addr(&self) -> u16 {
        match self.ppu_reg[PPU_CTRL_OFFSET] & 0x03u8 {
            0 => 0x2000,
            1 => 0x2400,
            2 => 0x2800,
            3 => 0x2c00,
            _ => panic!("invalid name table addr index"),
        }
    }
    /*************************** 0x2001: PPUMASK ***************************/
    // 論理が逆っぽいね。0がhide

    /// sprite描画有効判定
    pub fn read_ppu_is_write_sprite(&self) -> bool {
        (self.ppu_reg[PPU_MASK_OFFSET] & 0x10u8) == 0x10u8
    }
    /// bg描画有効判定
    pub fn read_ppu_is_write_bg(&self) -> bool {
        (self.ppu_reg[PPU_MASK_OFFSET] & 0x08u8) == 0x08u8
    }
    /// 左端8pxでスプライトクリッピング
    pub fn read_ppu_is_clip_sprite_leftend(&self) -> bool {
        (self.ppu_reg[PPU_MASK_OFFSET] & 0x04u8) != 0x04u8
    }
    /// 左端8pxでbgクリッピング
    pub fn read_ppu_is_clip_bg_leftend(&self) -> bool {
        (self.ppu_reg[PPU_MASK_OFFSET] & 0x02u8) != 0x02u8
    }
    pub fn read_is_monochrome(&self) -> bool {
        (self.ppu_reg[PPU_MASK_OFFSET] & 0x01u8) == 0x01u8
    }
    /*************************** 0x2002: PPU_STATUS ***************************/
    /// VBlankフラグをみて、NMI割り込みしようね
    /// CPUからPPU_STATUSを読みだした際の自動クリアなので、この関数を呼んでもクリアされない
    pub fn ppu_status_is_vblank(&self) -> bool {
        (self.ppu_reg[PPU_STATUS_OFFSET] & 0x80u8) == 0x80u8
    }
    /// Set the VBlank flag and NMI interrupt
    pub fn ppu_status_set_is_vblank(&mut self, is_set: bool) {
        if is_set {
            self.ppu_reg[PPU_STATUS_OFFSET] = self.ppu_reg[PPU_STATUS_OFFSET] | 0x80u8;
        } else {
            self.ppu_reg[PPU_STATUS_OFFSET] = self.ppu_reg[PPU_STATUS_OFFSET] & (!0x80u8);
        }
    }
    /// Sprite0描画中かどうか
    pub fn ppu_status_is_hit_sprite0(&self) -> bool {
        (self.ppu_reg[PPU_STATUS_OFFSET] & 0x40u8) == 0x40u8
    }
    pub fn ppu_status_set_is_hit_sprite0(&mut self, is_set: bool) {
        if is_set {
            self.ppu_reg[PPU_STATUS_OFFSET] = self.ppu_reg[PPU_STATUS_OFFSET] | 0x40u8;
        } else {
            self.ppu_reg[PPU_STATUS_OFFSET] = self.ppu_reg[PPU_STATUS_OFFSET] & (!0x40u8);
        }
    }

    /// Is the number of Sprites on the scanline greater than 8?
    pub fn ppu_status_sprite_overflow(&self) -> bool {
        (self.ppu_reg[PPU_STATUS_OFFSET] & 0x20u8) == 0x20u8
    }
    pub fn ppu_status_set_sprite_overflow(&mut self, is_set: bool) {
        if is_set {
            self.ppu_reg[PPU_STATUS_OFFSET] = self.ppu_reg[PPU_STATUS_OFFSET] | 0x20u8;
        } else {
            self.ppu_reg[PPU_STATUS_OFFSET] = self.ppu_reg[PPU_STATUS_OFFSET] & (!0x20u8);
        }
    }

    /// For reset when line 261 is reached
    pub fn clear_ppu_status(&mut self) {
        self.ppu_reg[PPU_STATUS_OFFSET] = 0x00u8;
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
        // PPU_CTRLのPPU Addr Incrementに従う
        let add_val = u16::from(self.read_ppu_addr_increment());
        let dst_addr = current_addr.wrapping_add(add_val);
        // 分解して入れておく
        self.ppu_addr_lower_reg = (dst_addr & 0xff) as u8;
        self.ppu_reg[PPU_ADDR_OFFSET] = (dst_addr >> 8) as u8;
    }

}
