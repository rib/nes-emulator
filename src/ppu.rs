use crate::ppu_registers;
use crate::ppu_registers::Control1Flags;
use crate::ppu_registers::Control2Flags;
use crate::ppu_registers::StatusFlags;
use crate::prelude::Cartridge;

use super::cpu::*;
use super::interface::*;
use super::system::*;
use super::vram::*;

/// 1lineあたりかかるCPUサイクル
pub const CPU_CYCLE_PER_LINE: usize = 341 / 3; // ppu cyc -> cpu cyc
/// 色の種類(RGB)
pub const NUM_OF_COLOR: usize = 4;
/// ユーザーに表示される領域幅
pub const VISIBLE_SCREEN_WIDTH: usize = 256;
/// ユーザーに表示される領域高さ
pub const VISIBLE_SCREEN_HEIGHT: usize = 240;
/// 実際に描画する幅(これは表示幅に等しい)
pub const RENDER_SCREEN_WIDTH: u16 = VISIBLE_SCREEN_WIDTH as u16;
/// VBlank期間を考慮した描画領域高さ
pub const RENDER_SCREEN_HEIGHT: u16 = 262; // 0 ~ 261
/// 1tileあたりのpixel数
pub const PIXEL_PER_TILE: u16 = 8; // 1tile=8*8
/// 横タイル数 32
pub const SCREEN_TILE_WIDTH: u16 = (VISIBLE_SCREEN_WIDTH as u16) / PIXEL_PER_TILE; // 256/8=32
/// 縦タイル数 30
pub const SCREEN_TILE_HEIGHT: u16 = (VISIBLE_SCREEN_HEIGHT as u16) / PIXEL_PER_TILE; // 240/8=30
/// 1属性テーブルエントリに対する横, 縦タイル数
pub const BG_NUM_OF_TILE_PER_ATTRIBUTE_TABLE_ENTRY: u16 = 4;
/// 属性テーブルの横エントリ数 8
pub const ATTRIBUTE_TABLE_WIDTH: u16 = SCREEN_TILE_WIDTH / BG_NUM_OF_TILE_PER_ATTRIBUTE_TABLE_ENTRY;

/// PPU内部のOAMの容量 dmaの転送サイズと等しい
pub const OAM_SIZE: usize = 0x100;
/// DMA転送を2line処理で終えようと思ったときの1回目で転送するバイト数
/// 341cyc/513cyc*256byte=170.1byte
pub const OAM_DMA_COPY_SIZE_PER_PPU_STEP: u8 = 0xaa;
/// pattern1個あたりのエントリサイズ
pub const PATTERN_TABLE_ENTRY_BYTE: u16 = 16;

/// スプライトテンポラリレジスタ数
pub const SPRITE_TEMP_SIZE: usize = 8;
/// スプライト総数
pub const NUM_OF_SPRITE: usize = 64;
/// スプライト1個あたり4byte
pub const SPRITE_SIZE: usize = 4;
/// スプライトの横幅
pub const SPRITE_WIDTH: usize = 8;
pub const SPRITE_NORMAL_HEIGHT: usize = 8;
pub const SPRITE_LARGE_HEIGHT: usize = 16;
/// 1frame書くのにかかるサイクル数
pub const CYCLE_PER_DRAW_FRAME: usize = CPU_CYCLE_PER_LINE * ((RENDER_SCREEN_HEIGHT + 1) as usize);

#[derive(Copy, Clone)]
pub struct Position(pub u8, pub u8);

#[derive(Copy, Clone, Eq, PartialEq)]
/// R,G,B
pub struct Color(pub u8, pub u8, pub u8);
impl Color {
    /// 2C02の色情報をRGBに変換します
    /// ..VV_HHHH 形式
    /// V - 明度
    /// H - 色相
    pub fn from(src: u8) -> Color {
        let index = src & 0x3f;
        let table: [Color; 0x40] = include!("ppu_palette_table.rs");
        table[index as usize]
    }
    pub fn is_black(&self) -> bool {
        self.0 == 0x0 && self.1 == 0x0 && self.2 == 0x0
    }
}

/// sprite.tile_idのu8から変換する
#[derive(Copy, Clone)]
pub enum TileId {
    /// 8*8 spriteの場合
    Normal { id: u8 },
    /// 8*16 spriteの場合
    /// TTTTTTTP
    /// P - pattern table addr(0:0x0000, 1: 0x1000)
    /// T - Tile Id
    Large {
        /// P
        pattern_table_addr: u16,
        /// 2*T
        upper_tile_id: u8,
        /// 2*T+1
        lower_tile_id: u8,
    },
}
impl TileId {
    pub fn normal(src: u8) -> TileId {
        TileId::Normal { id: src }
    }
    pub fn large(src: u8) -> TileId {
        TileId::Large {
            pattern_table_addr: (if (src & 0x01) == 0x01 {
                0x1000
            } else {
                0x0000u16
            }),
            upper_tile_id: src & 0xfe,
            lower_tile_id: (src & 0xfe) + 1,
        }
    }
}
/// 描画に必要な補足情報とか
/// VHP___CC
#[derive(Copy, Clone)]
pub struct SpriteAttr {
    /// V 垂直反転
    is_vert_flip: bool,
    /// H 垂直反転
    is_hor_flip: bool,
    /// P 描画優先度
    is_draw_front: bool,
    /// CC pattele指定(2bit)
    palette_id: u8,
}
impl SpriteAttr {
    pub fn from(src: u8) -> SpriteAttr {
        SpriteAttr {
            is_vert_flip: (src & 0x80) == 0x80,
            is_hor_flip: (src & 0x40) == 0x40,
            is_draw_front: (src & 0x20) != 0x20,
            palette_id: (src & 0x03),
        }
    }
}

#[derive(Copy, Clone)]
pub struct Sprite {
    /// Actually, it is displayed in the place where +1 is added
    y: u8,
    tile_id: TileId,
    attr: SpriteAttr,
    x: u8,
}

impl Sprite {
    /// Generate Sprite from OAM information
    /// `is_large` -true if sprite size is 8 * 16, false if 8 * 8
    pub fn from(is_large: bool, byte0: u8, byte1: u8, byte2: u8, byte3: u8) -> Sprite {
        Sprite {
            y: byte0,
            tile_id: (if is_large {
                TileId::large(byte1)
            } else {
                TileId::normal(byte1)
            }),
            attr: SpriteAttr::from(byte2),
            x: byte3,
        }
    }
}

#[derive(Copy, Clone)]
enum LineStatus {
    Visible,                // 0~239
    PostRender,             // 240
    VerticalBlanking(bool), // 241~260
    PreRender,              // 261
}

impl LineStatus {
    fn from(line: u16) -> LineStatus {
        if line < 240 {
            LineStatus::Visible
        } else if line == 240 {
            LineStatus::PostRender
        } else if line < 261 {
            LineStatus::VerticalBlanking(line == 241)
        } else if line == 261 {
            LineStatus::PreRender
        } else {
            panic!("invalid line status");
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub enum PixelFormat {
    RGBA8888,
    BGRA8888,
    ARGB8888,
}

#[derive(Copy, Clone)]
pub struct DrawOption {
    /// Frame Buffer全体の幅
    pub fb_width: u32,
    /// Frame Buffer全体の高さ
    pub fb_height: u32,
    /// PPUのデータを書き出す左上座標
    pub offset_x: i32,
    /// PPUのデータを書き出す左上座標
    pub offset_y: i32,
    /// PPU 1dotをFrameBufferのpixel数に換算する
    pub scale: u32,
    /// Frame Bufferの色設定
    pub pixel_format: PixelFormat,
}

impl Default for DrawOption {
    fn default() -> Self {
        Self {
            fb_width: VISIBLE_SCREEN_WIDTH as u32,
            fb_height: VISIBLE_SCREEN_HEIGHT as u32,
            offset_x: 0,
            offset_y: 0,
            scale: 1,
            pixel_format: PixelFormat::RGBA8888,
        }
    }
}

#[derive(Clone)]
pub struct Ppu {
    //  0x2000 - 0x2007: PPU I/O
    //  0x2008 - 0x3fff: PPU I/O Mirror x1023
    //pub ppu_reg: [u8; PPU_REG_SIZE],

    pub palette: [u8; PALETTE_SIZE],

    pub io_latch_value: u8,

    pub read_buffer: u8,

    pub status: StatusFlags,
    pub control1: Control1Flags,
    pub control2: Control2Flags,

    pub write_toggle: bool, // Latch for PPU_SCROLL and PPU_ADDR
    pub shared_temp: u16, // Shared temp for PPU_SCROLL and PPU_ADDR
    // Either configured directly via PPU_ADDR, or indirectly via PPU_SCROLL
    pub shared_vram_addr: u16,

    pub scroll_x_fine3: u8,

    pub vram: VRam,

    /// Object Attribute Memoryの実態
    pub oam: [u8; OAM_SIZE],
    pub oam_offset: u8,

    /// 次の描画で使うスプライトを格納する
    pub sprite_temps: [Option<Sprite>; SPRITE_TEMP_SIZE],

    pub cumulative_cpu_cyc: usize,
    /// 次処理するy_index
    pub current_line: u16,

    // scrollレジスタは1lineごとに更新
    //pub fetch_scroll_x: u8,
    //pub fetch_scroll_y: u8,
    pub current_scroll_x: u8,
    pub current_scroll_y: u8,

    /// PPUの描画設定(step時に渡したかったが、毎回渡すのも無駄なので)
    pub draw_option: DrawOption,
}

impl Default for Ppu {
    fn default() -> Self {
        Self {
            io_latch_value: 0,

            palette: [0; PALETTE_SIZE],

            read_buffer: 0,

            status: StatusFlags::empty(),
            control1: Control1Flags::empty(),
            control2: Control2Flags::empty(),

            write_toggle: false,
            shared_temp: 0,
            shared_vram_addr: 0,
            scroll_x_fine3: 0,

            vram: Default::default(),

            oam: [0; OAM_SIZE],
            oam_offset: 0,
            sprite_temps: [None; SPRITE_TEMP_SIZE],

            cumulative_cpu_cyc: 0,
            current_line: 241,

            current_scroll_x: 0,
            current_scroll_y: 0,

            draw_option: DrawOption::default(),
        }
    }
}

impl EmulateControl for Ppu {
    fn poweron(&mut self) {

        self.vram.poweron();

        self.palette = [0; PALETTE_SIZE];

        self.status = StatusFlags::empty();
        self.control1 = Control1Flags::empty();
        self.control2 = Control2Flags::empty();

        self.write_toggle = false;
        self.shared_temp = 0;
        self.shared_vram_addr = 0;
        self.scroll_x_fine3 = 0;

        self.oam = [0; OAM_SIZE];
        self.sprite_temps = [None; SPRITE_TEMP_SIZE];

        self.current_line = 241;
        self.cumulative_cpu_cyc = 0;

        self.current_scroll_x = 0;
        self.current_scroll_y = 0;
    }
}

impl Ppu {

    // TODO: decay the latch value over time
    pub fn read_with_latch(&mut self, value: u8, undefined_bits: u8) -> u8 {
        let read = (value & !undefined_bits) | (self.io_latch_value & undefined_bits);
        self.io_latch_value = (self.io_latch_value & undefined_bits) | (value & !undefined_bits);
        read
    }

    pub fn pallet_read_u8(&self, addr: u16) -> u8 {
        let index = usize::from(addr - PALETTE_TABLE_BASE_ADDR) % PALETTE_SIZE;
        match index {
            0x10 => self.palette[0x00],
            0x14 => self.palette[0x04],
            0x18 => self.palette[0x08],
            0x1c => self.palette[0x0c],
            _ => arr_read!(self.palette, index),
        }
    }

    pub fn pallet_write_u8(&mut self, addr: u16, data: u8) {
        // Palette with mirroring
        let index = usize::from(addr - PALETTE_TABLE_BASE_ADDR) % PALETTE_SIZE;
        match index {
            0x10 => self.palette[0x00] = data,
            0x14 => self.palette[0x04] = data,
            0x18 => self.palette[0x08] = data,
            0x1c => self.palette[0x0c] = data,
            _ => arr_write!(self.palette, index, data),
        };
    }

    /// Returns (value, undefined_bit_mask)
    pub fn data_read_u8(&mut self, cartridge: &mut Cartridge, addr: u16) -> (u8, u8) {
        if let 0x3f00..=0x3fff = addr { // Pallet reads bypass buffering
            self.read_buffer = self.vram.read_u8(cartridge, addr);
            (self.pallet_read_u8(addr), 0xc0)
        } else {
            let buffered = self.read_buffer;
            self.read_buffer = self.vram.read_u8(cartridge, addr);
            (buffered, 0)
        }
    }

    pub fn data_write_u8(&mut self, cartridge: &mut Cartridge, addr: u16, data: u8) {
        if let 0x3f00..=0x3fff = addr {
            //println!("palette write: addr={addr:x}, data={data:x}");
            self.pallet_write_u8(addr, data);
        } else {
            //println!("data write: addr={addr:x}, data={data:x}");
            self.vram.write_u8(cartridge, addr, data);
        }
    }

    pub fn increment_data_addr(&mut self) {
        // FIXME: handle these details when rendering...
        //
        // Outside of rendering, reads from or writes to $2007 will add either
        // 1 or 32 to v depending on the VRAM increment bit set via $2000.
        // During rendering (on the pre-render line and the visible lines
        // 0-239, provided either background or sprite rendering is enabled),
        // it will update v in an odd way, triggering a coarse X increment and
        // a Y increment simultaneously (with normal wrapping behavior).
        //
        // Internally, this is caused by the carry inputs to various sections
        // of v being set up for rendering, and the $2007 access triggering a
        // "load next value" signal for all of v (when not rendering, the carry
        // inputs are set up to linearly increment v by either 1 or 32)

        self.shared_vram_addr = self.shared_vram_addr.wrapping_add(self.address_increment());
    }

    pub fn read_u8(&mut self, cartridge: &mut Cartridge, addr: u16) -> u8 {
        // mirror support
        let addr = ((addr - 0x2000) % 8) + 0x2000;
        let (value, undefined_bits) = match addr {
            0x2000 => { // Control 1 (Write-only)
                (0, 0xff)
            }
            0x2001 => {  // Control 2 (Write-only)
                (0, 0xff)
            }
            // PPU_STATUS (read-only) Resets double-write register status, clears VBLANK flag
            0x2002 => { // Status (Read-only)
                let data = self.status.bits();
                self.write_toggle = false;
                //self.ppu_is_second_write = false;
                self.status.set(StatusFlags::IN_VBLANK, false);
                (data, StatusFlags::UNDEFINED_BITS.bits())
            }
            0x2003 => { // OAMADDR (Write-only)
                (0, 0xff)
            }
            0x2004 => { // OAMDATA (Read/Write)
                (self.oam[self.oam_offset as usize], 0xff)
            }
            0x2005 => { // PPU_SCROLL (Write-only)
                (0, 0xff)
            }
            0x2006 => { // PPU_ADDR (Write-only)
                (0, 0xff)
            }
            0x2007 => { // PPU_DATA (Read/Write)
                let (data, undefined_mask) = self.data_read_u8(cartridge, self.shared_vram_addr);
                self.increment_data_addr();
                (data, undefined_mask)
            }
            _ => unreachable!()
        };

        self.read_with_latch(value, undefined_bits)
    }

    pub fn write_u8(&mut self, cartridge: &mut Cartridge, addr: u16, data: u8) {
        // mirror support
        let addr = ((addr - 0x2000) % 8) + 0x2000;
        self.io_latch_value = data;
        match addr {
            0x2000 => { // Control 1
                self.control1 = Control1Flags::from_bits_truncate(data);
                // The lower nametable bits become 10-11 of the shared (15 bit) temp register that's
                // used by PPU_SCROLL and PPU_ADDR
                self.shared_temp = (self.shared_temp & 0b0111_0011_1111_1111) | ((data as u16 & 0b11) << 10);
            }
            0x2001 => {  // Control 2
                self.control2 = Control2Flags::from_bits_truncate(data);
            }
            0x2002 => { // Status
                // Read Only
            }
            0x2003 => { // OAMADDR
                /* TODO: also corrupts OAM data...
                   https://forums.nesdev.org/viewtopic.php?t=10189

                    * Take old value from $2003 and AND it with $F8
                    * Read 8 bytes from OAM starting at this masked value
                    * Write them starting at $XX in OAM, where $XX is the high byte of the PPU register written to ($20-$3F) masked with $F8
                    * Use new value written to $2003 as OAM address

                    But this is just for the "preferred" CPU-PPU alignment. For another,
                    I get totally different corruptions at portions of OAM related to the
                    new value written. It's probably using a different value to write the
                    8-byte chunk to OAM

                    Seems like this has been more an issue for people writing tests, and
                    hopefully no games depend on this
                 */
                self.oam_offset = data;
            }
            0x2004 => {
                //self.written_oam_data = true;
                //arr_write!(self.ppu_reg, 4, data);
                self.oam[self.oam_offset as usize] = data;
                self.oam_offset = self.oam_offset.wrapping_add(1);
            }
            0x2005 => { // PPU_SCROLL
                // NB: This is the layout of the (15bit) shared temp register when used
                // for rendering / scrolling:
                // yyy NN YYYYY XXXXX
                // ||| || ||||| +++++-- coarse X scroll
                // ||| || +++++-------- coarse Y scroll
                // ||| ++-------------- nametable select
                // +++----------------- fine Y scroll
                if self.write_toggle {
                    let fine3_y = (data & 0b111) as u16;
                    let coarse5_y = ((data & 0b1111_1000) >> 3) as u16;
                    self.shared_temp = (self.shared_temp & 0b0000_1100_0001_1111) | (fine3_y << 12) | (coarse5_y << 5);

                    // FIXME this should be updated in PreRender...
                    let (scroll_x, scroll_y) = self.decode_scroll_xy();
                    self.current_scroll_x = scroll_x;
                    self.current_scroll_y = scroll_y;
                } else {
                    self.scroll_x_fine3 = data & 0b111;
                    self.shared_temp = (self.shared_temp & 0b0111_1111_1110_0000) | (((data >> 3) as u16) & 0b1_1111);
                }
                self.write_toggle = !self.write_toggle;
            }
            0x2006 => { // PPU_ADDR
                if self.write_toggle {
                    let lsb = data;
                    self.shared_temp = (self.shared_temp & 0xff00) | (lsb as u16);
                    self.shared_vram_addr = self.shared_temp;
                } else {
                    // NB: shared_temp (t) is a 15 bit register that's shared between
                    // PPU_ADDR and PPU_SCROLL. Also note the PPU only has a 14bit address
                    // space for vram and the first write to $2006 will set the upper
                    // bits of the shared_temp address except with the top bit of the
                    // address cleared (so we clear the top two bits since we're storing
                    // as a 16 bit value)
                    //
                    let msb = data & 0b0011_1111;
                    self.shared_temp = ((msb as u16) << 8) | (self.shared_temp & 0xff);
                }
                self.write_toggle = !self.write_toggle;
            }
            0x2007 => { // PPU_DATA
                //println!("data_write_u8: {:x}, {data:x}", self.shared_vram_addr);
                self.data_write_u8(cartridge, self.shared_vram_addr, data);

                //arr_write!(self.ppu_reg, 7, data);
                // PPUに書いてもらおう
                //self.written_ppu_data = true;
                self.increment_data_addr();
            }
            _ => unreachable!()
        };
    }

    /// Draw one line
    ///
    /// `tile_base`   - Current tile position without scroll offset addition
    /// `tile_global` - Tile position on 4 sides including scroll offset
    /// `tile_local`  - Converted `tile_global` to the position on the tile on 1Namespace
    /// All of the above should match without scroll
    fn draw_line(&mut self, cartridge: &mut Cartridge, fb: *mut u8) {
        let nametable_base_addr = self.name_table_base_addr();
        let pattern_table_addr = self.bg_pattern_table_addr();
        //println!("nt = {:x}, pt = {:x}", nametable_base_addr, pattern_table_addr);
        let is_clip_bg_leftend = self.control2.contains(Control2Flags::BG_LEFT_COL_SHOW) == false;
        let is_write_bg = self.control2.contains(Control2Flags::SHOW_BG);
        let is_monochrome = self.control2.contains(Control2Flags::MONOCHROME);
        let master_bg_color = Color::from(self.pallet_read_u8(
            PALETTE_TABLE_BASE_ADDR + PALETTE_BG_OFFSET,
        ));

        let raw_y = self.current_line + u16::from(self.current_scroll_y);
        let offset_y = raw_y & 0x07; // Actual pixel deviation (0 ~ 7) from y position in tile conversion
        let tile_base_y = raw_y >> 3; // Current position in tile conversion without offset
                                      // scroll reg shifts in tile conversion
        let tile_global_y = tile_base_y % (SCREEN_TILE_HEIGHT * 2); // y absolute coordinates in tile conversion
        let tile_local_y = tile_global_y % SCREEN_TILE_HEIGHT; // Absolute coordinates within 1 tile
                                                               // Of the 4 sides, if it is approaching the lower side, it is false
        let is_nametable_position_top = tile_global_y < SCREEN_TILE_HEIGHT;

        // pixel formatの決定
        let pixel_indexes = match self.draw_option.pixel_format {
            PixelFormat::RGBA8888 => (0, 1, 2, 3),
            PixelFormat::BGRA8888 => (2, 1, 0, 3),
            PixelFormat::ARGB8888 => (1, 2, 3, 0),
        };

        //println!("scroll_x = {}, y = {}", self.current_scroll_x, self.current_scroll_y);
        // Loop in the drawing coordinate system
        let pixel_y = usize::from(self.current_line);
        for pixel_x in 0..VISIBLE_SCREEN_WIDTH {
            // Sprite: Get the data to draw from the searched temporary register
            let (sprite_palette_data_back, sprite_palette_data_front) =
                self.get_sprite_draw_data(cartridge, pixel_x, pixel_y);

            // BG (Nametable): Get data from nametable and attribute table corresponding to coordinates
            let offset_x = ((pixel_x as u16) + u16::from(self.current_scroll_x)) & 0x07;
            let tile_base_x = ((pixel_x as u16) + u16::from(self.current_scroll_x)) >> 3;
            // scroll reg shifts in tile conversion
            let tile_global_x = tile_base_x % (SCREEN_TILE_WIDTH * 2); // X absolute coordinates in 4tile conversion
            let tile_local_x = tile_global_x % SCREEN_TILE_WIDTH; // Absolute coordinates within 1 tile
            let is_nametable_position_left = tile_global_x < SCREEN_TILE_WIDTH; // False if it is on the right side of the 4 sides

            // Since we know which of the four faces, we will return the base address of that face
            let target_nametable_base_addr = nametable_base_addr +
                (if is_nametable_position_left { 0x0000 } else { 0x0400 }) + // Wide area offset on the left and right sides
                (if is_nametable_position_top  { 0x0000 } else { 0x0800 }); // Wide area offset on top and bottom

            // Since the attribute table is 32 bytes after the nametable, the address is calculated and read.
            // It is 1 attr with 4 * 4 tiles in height and width.
            // Offset calculation uses global position for scroll support (maybe 1Nametable with clipping)
            let attribute_base_addr = target_nametable_base_addr + ATTRIBUTE_TABLE_OFFSET; // 23c0, 27c0, 2bc0, 2fc0のどれか
            let attribute_x_offset = (tile_global_x >> 2) & 0x7;
            let attribute_y_offset = tile_global_y >> 2;
            let attribute_addr =
                attribute_base_addr + (attribute_y_offset << 3) + attribute_x_offset;

            // Used for attribute reading and BG palette selection.
            // Change the palette information used at the 4 * 4 position
            let raw_attribute = self.vram.read_u8(cartridge, attribute_addr);
            let bg_palette_id = match (tile_local_x & 0x03 < 0x2, tile_local_y & 0x03 < 0x2) {
                (true, true) => (raw_attribute >> 0) & 0x03,  // top left
                (false, true) => (raw_attribute >> 2) & 0x03, // top right
                (true, false) => (raw_attribute >> 4) & 0x03, // bottom left
                (false, false) => (raw_attribute >> 6) & 0x03, // bottom right
            };

            // Read tile_id from Name table-> Build data from pattern table
            let nametable_addr = target_nametable_base_addr + (tile_local_y << 5) + tile_local_x;
            let bg_tile_id = u16::from(self.vram.read_u8(cartridge, nametable_addr));


            // pattern_table 1entry is 16 bytes, if it is the 0th line, use the 0th and 8th data
            let bg_pattern_table_base_addr = pattern_table_addr + (bg_tile_id << 4);
            let bg_pattern_table_addr_lower = bg_pattern_table_base_addr + offset_y;
            let bg_pattern_table_addr_upper = bg_pattern_table_addr_lower + 8;
            let bg_data_lower = self
                .vram
                .read_u8(cartridge, bg_pattern_table_addr_lower);
            let bg_data_upper = self
                .vram
                .read_u8(cartridge, bg_pattern_table_addr_upper);


            // Make the drawing color of bg
            let bg_palette_offset = (((bg_data_upper >> (7 - offset_x)) & 0x01) << 1)
                | ((bg_data_lower >> (7 - offset_x)) & 0x01);
            let bg_palette_addr = (PALETTE_TABLE_BASE_ADDR + PALETTE_BG_OFFSET) +   // 0x3f00
                (u16::from(bg_palette_id) << 2) + // Select BG Palette 0 ~ 3 in attribute
                u16::from(bg_palette_offset); // Color selection in palette

            // Create BG data considering the 8 pixel clip at the left end of BG
            let is_bg_clipping = is_clip_bg_leftend && (pixel_x < 8);
            let is_bg_tranparent = (bg_palette_addr & 0x03) == 0x00; // If the background color is selected, it will be processed here
            let bg_palette_data: Option<u8> = if is_bg_clipping || !is_write_bg || is_bg_tranparent
            {
                None
            } else {
                Some(self.pallet_read_u8(bg_palette_addr))
            };

            // transparent
            let mut draw_color = master_bg_color;

            'select_color: for palette_data in &[
                sprite_palette_data_front,
                bg_palette_data,
                sprite_palette_data_back,
            ] {
                if let Some(color_index) = palette_data {
                    let c = Color::from(*color_index);
                    draw_color = c;
                    break 'select_color;
                }
            }

            let draw_base_y =
                self.draw_option.offset_y + (pixel_y as i32) * (self.draw_option.scale as i32);
            let draw_base_x =
                self.draw_option.offset_x + (pixel_x as i32) * (self.draw_option.scale as i32);
            // Coordinate calculation, 1 dot needs to be reflected in scale ** 2 pixel
            for scale_y in 0..self.draw_option.scale {
                let draw_y = draw_base_y + (scale_y as i32);
                if (draw_y < 0) || ((self.draw_option.fb_height as i32) <= draw_y) {
                    continue;
                }

                for scale_x in 0..self.draw_option.scale {
                    let draw_x = draw_base_x + (scale_x as i32);
                    if (draw_x < 0) || ((self.draw_option.fb_width as i32) <= draw_x) {
                        continue;
                    }

                    // Calculate the corresponding coordinates from the size of FrameBuffer
                    // Use the width of FrameBuffer instead of 256 for the width when
                    // calculating the index corresponding to the Y position.
                    let base_index = ((draw_y as isize) * (self.draw_option.fb_width as isize)
                        + (draw_x as isize))
                        * (NUM_OF_COLOR as isize);

                    unsafe {
                        let base_ptr = fb.offset(base_index);

                        *base_ptr.offset(pixel_indexes.0) = draw_color.0; // R
                        *base_ptr.offset(pixel_indexes.1) = draw_color.1; // G
                        *base_ptr.offset(pixel_indexes.2) = draw_color.2; // B
                        *base_ptr.offset(pixel_indexes.3) = 0xff; // alpha blending

                        // Supports monochrome output (total average for the time being)
                        if is_monochrome {
                            let data = ((u16::from(*base_ptr.offset(pixel_indexes.0))
                                + u16::from(*base_ptr.offset(pixel_indexes.1))
                                + u16::from(*base_ptr.offset(pixel_indexes.2)))
                                / 3) as u8;
                            *base_ptr.offset(pixel_indexes.0) = data;
                            *base_ptr.offset(pixel_indexes.1) = data;
                            *base_ptr.offset(pixel_indexes.2) = data;
                        }
                    }
                }
            }
        }
    }

    /// Draws a sprite on the specified pixel
    /// Returns: (Data drawn after bg, data drawn before bg)
    fn get_sprite_draw_data(
        &mut self,
        cartridge: &mut Cartridge,
        pixel_x: usize,
        pixel_y: usize,
    ) -> (Option<u8>, Option<u8>) {
        if !self.control2.contains(Control2Flags::SHOW_SPRITES) {
            return (None, None);
        }

        // Search for Sprite (Sprite that must be drawn in y position is preloaded)
        let mut sprite_palette_data_back: Option<u8> = None;
        let mut sprite_palette_data_front: Option<u8> = None;
        'draw_sprite: for &s in self.sprite_temps.iter() {
            if let Some(sprite) = s {
                let sprite_x = usize::from(sprite.x);
                let sprite_y = usize::from(sprite.y);
                // Left edge Not displayed when sprite clipping is enabled
                let is_sprite_clipping = self.control2.contains(Control2Flags::SPRITES_LEFT_COL_SHOW) == false && (pixel_x < 8);
                if !is_sprite_clipping
                    && (sprite_x <= pixel_x)
                    && (pixel_x < usize::from(sprite_x + SPRITE_WIDTH))
                {
                    // Relative coordinates of sprite
                    let sprite_offset_x: usize = pixel_x - sprite_x; // 0-7
                    let sprite_offset_y: usize = pixel_y - sprite_y - 1; // 0-7 or 0-15 (largeの場合, tile参照前に0-7に詰める)
                    debug_assert!(sprite_offset_x < SPRITE_WIDTH);
                    debug_assert!(sprite_offset_y < usize::from(self.sprite_height()));

                    // pattern table addr and tile id are determined by size
                    let (sprite_pattern_table_addr, sprite_tile_id): (u16, u8) = match sprite
                        .tile_id
                    {
                        TileId::Normal { id } => (self.sprites_pattern_table_addr(), id),
                        // Since it is 8 * 16 sprite, the id is separated at the top and bottom.
                        TileId::Large {
                            pattern_table_addr,
                            upper_tile_id,
                            lower_tile_id,
                        } => {
                            let is_upper = sprite_offset_y < SPRITE_NORMAL_HEIGHT;
                            let is_vflip = sprite.attr.is_vert_flip;
                            let id = match (is_upper, is_vflip) {
                                (true, false) => upper_tile_id,  // Drawing coordinates are 8 pixels above, no Flip
                                (false, false) => lower_tile_id, // Drawing coordinates are 8 pixels below, no Flip
                                (true, true) => lower_tile_id,   // Drawing coordinates are 8 pixels above, with Flip
                                (false, true) => upper_tile_id,  // Drawing coordinates are 8 pixels below, with Flip
                            };
                            (pattern_table_addr, id)
                        }
                    };

                    // Determine the data position on the tile considering x, y flip
                    let tile_offset_x: usize = if !sprite.attr.is_hor_flip {
                        sprite_offset_x
                    } else {
                        SPRITE_WIDTH - 1 - sprite_offset_x
                    };
                    let tile_offset_y: usize = if !sprite.attr.is_vert_flip {
                        sprite_offset_y % SPRITE_NORMAL_HEIGHT
                    } else {
                        SPRITE_NORMAL_HEIGHT - 1 - (sprite_offset_y % SPRITE_NORMAL_HEIGHT)
                    };
                    // Calculate tile addr
                    let sprite_pattern_table_base_addr = u16::from(sprite_pattern_table_addr)
                        + (u16::from(sprite_tile_id) * PATTERN_TABLE_ENTRY_BYTE);
                    let sprite_pattern_table_addr_lower =
                        sprite_pattern_table_base_addr + (tile_offset_y as u16);
                    let sprite_pattern_table_addr_upper = sprite_pattern_table_addr_lower + 8;
                    let sprite_data_lower = self
                        .vram
                        .read_u8(cartridge, sprite_pattern_table_addr_lower);
                    let sprite_data_upper = self
                        .vram
                        .read_u8(cartridge, sprite_pattern_table_addr_upper);
                    // Create a pixel pattern at the corresponding x position
                    let sprite_palette_offset =
                        (((sprite_data_upper >> (7 - tile_offset_x)) & 0x01) << 1)
                            | ((sprite_data_lower >> (7 - tile_offset_x)) & 0x01);
                    // Calculate the address of the palette
                    let sprite_palette_addr = (PALETTE_TABLE_BASE_ADDR + PALETTE_SPRITE_OFFSET) +        // 0x3f10
                        (u16::from(sprite.attr.palette_id) * PALETTE_ENTRY_SIZE) + // Select Sprite Palette 0 ~ 3 in attribute
                        u16::from(sprite_palette_offset); // Color selection in palette
                                                          // If the palette is transparent, this pixel will not be drawn
                    let is_tranparent = (sprite_palette_addr & 0x03) == 0x00; // Background color selected
                    if !is_tranparent {
                        let sprite_palette_data = self
                            .pallet_read_u8(sprite_palette_addr);
                        if sprite.attr.is_draw_front {
                            sprite_palette_data_front = Some(sprite_palette_data);
                        } else {
                            sprite_palette_data_back = Some(sprite_palette_data);
                        }
                    }
                }
            } else {
                // sprite temps are pre-packed so no processing is needed anymore
                break 'draw_sprite;
            }
        }
        (sprite_palette_data_back, sprite_palette_data_front)
    }

    /// Search the OAM and fetch the sprite used for the next drawing into the register
    /// If it exceeds 8, the Overflow flag will be set
    fn fetch_sprite(&mut self) {
        if !self.control2.contains(Control2Flags::SHOW_SPRITES) {
            return;
        }

        // Pre-calculate sprite size
        let sprite_begin_y = self.current_line;
        let sprite_height = u16::from(self.sprite_height());
        let is_large = sprite_height == 16;

        // Clear all for the time being
        self.sprite_temps = [None; SPRITE_TEMP_SIZE];
        // Collect the ones whose current_line + 1 matches y in order (the condition is made bigger)
        let mut tmp_index = 0;
        'search_sprite: for sprite_index in 0..NUM_OF_SPRITE {
            let target_oam_addr = sprite_index * 4;
            // Equal to the value of y
            let sprite_y = u16::from(self.oam[target_oam_addr]);
            //println!("sprite y = {sprite_y}");
            let sprite_end_y = sprite_y + sprite_height;
            // Within the drawing range (y + 1) ~ (y + 1 + 8 or 16)
            if (sprite_y < sprite_begin_y) && (sprite_begin_y <= sprite_end_y) {
                // sprite 0 hit flag (Since it is processed for each line, it will be set first)
                let is_zero_hit_delay = sprite_begin_y > (sprite_end_y - 3); // If it is processed one line at a time, Mario etc. will be detected too quickly, so FIXME
                if sprite_index == 0 && is_zero_hit_delay {
                    self.status.set(StatusFlags::SPRITE0_HIT, true);
                }
                // sprite overflow
                if tmp_index >= SPRITE_TEMP_SIZE {
                    self.status.set(StatusFlags::SPRITE_OVERFLOW, true);
                    break 'search_sprite;
                } else {
                    debug_assert!(tmp_index < SPRITE_TEMP_SIZE);
                    self.sprite_temps[tmp_index] = Some(Sprite::from(
                        is_large,
                        self.oam[target_oam_addr],
                        self.oam[target_oam_addr + 1],
                        self.oam[target_oam_addr + 2],
                        self.oam[target_oam_addr + 3],
                    ));
                    tmp_index = tmp_index + 1;
                }
            }
        }
    }


    fn update_line(&mut self, cartridge: &mut Cartridge, fb: *mut u8) -> Option<Interrupt> {

        self.status.set(StatusFlags::SPRITE0_HIT, false);
        self.status.set(StatusFlags::SPRITE_OVERFLOW, false);

        /*
        match self.current_line {
            0..=239 => { // Visible

            },
            240 => { // PostRender

            },
            241..=260 => { // VBlank

            }
            261 => { // PreRender

            }
        }*/
        //println!("line = {}", self.current_line);
        match LineStatus::from(self.current_line) {
            LineStatus::Visible => {
                self.oam_offset = 0;
                self.fetch_sprite();
                // Draw one line
                self.draw_line(cartridge, fb);
                // Update row counter and finish
                self.current_line = (self.current_line + 1) % RENDER_SCREEN_HEIGHT;

                None
            }
            LineStatus::PostRender => {
                self.current_line = (self.current_line + 1) % RENDER_SCREEN_HEIGHT;
                None
            }
            LineStatus::VerticalBlanking(is_first) => {
                self.current_line = (self.current_line + 1) % RENDER_SCREEN_HEIGHT;
                if is_first {
                    self.status.set(StatusFlags::IN_VBLANK, true);
                }
                if self.control1.contains(Control1Flags::NMI_ENABLE) && self.status.contains(StatusFlags::IN_VBLANK) {
                    Some(Interrupt::NMI)
                } else {
                    None
                }
            }
            LineStatus::PreRender => {
                self.current_line = (self.current_line + 1) % RENDER_SCREEN_HEIGHT;
                self.status.set(StatusFlags::IN_VBLANK, false);

                // During dots 280 to 304 of the pre-render scanline (end of vblank)
                //
                // If rendering is enabled, at the end of vblank, shortly after
                // the horizontal bits are copied from t to v at dot 257, the
                // PPU will repeatedly copy the vertical bits from t to v from
                // dots 280 to 304, completing the full initialization of v
                // from t:
                //
                // FIXME: don'y copy _all_ bits: v: GHIA.BC DEF..... <- t: GHIA.BC DEF.....

                // FIXME
                //self.shared_vram_addr = self.shared_temp;
                //let (scroll_x, scroll_y) = self.decode_scroll_xy();

                self.oam_offset = 0;

                None
            }
        }
    }

    pub fn decode_scroll_xy(&self) -> (u8, u8) {
        let coarse_x = ((self.shared_temp & 0b11111) << 3) as u8;
        let fine_x = self.scroll_x_fine3 & 0b111;
        let scroll_x = coarse_x | fine_x;
        let coarse_y = ((self.shared_temp & 0b11_1110_0000) >> 2) as u8;
        let fine_y = ((self.shared_temp & 0b0111_0000_0000_0000) >> 12) as u8;
        let scroll_y  = coarse_y | fine_y;

        println!("scroll_x = {}, scroll_y = {}", self.current_scroll_x, self.current_scroll_y);
        (scroll_x, scroll_y)
    }

    pub fn step(&mut self, cpu_cyc: usize, cartridge: &mut Cartridge, fb: *mut u8) -> Option<Interrupt> {

        // TODO: rework this to update based on a PPU clock step that will be driven by the nes/system
        // according to the cpu clocks elapsed (instead of batching up scanline processing)

        let total_cyc = self.cumulative_cpu_cyc + cpu_cyc;
        if total_cyc >= CPU_CYCLE_PER_LINE {
            self.cumulative_cpu_cyc = total_cyc - CPU_CYCLE_PER_LINE;
            self.update_line(cartridge, fb)
        } else {
            self.cumulative_cpu_cyc = total_cyc;
            None
        }
    }
}
