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
    ///  y座標
    /// 実際は+1した場所に表示する
    y: u8,
    /// tile ID指定
    tile_id: TileId,
    /// 属性とか
    attr: SpriteAttr,
    /// x座標
    x: u8,
}

impl Sprite {
    /// SpriteをOAMの情報から生成します。
    /// `is_large` -スプライトサイズが8*16ならtrue、8*8ならfalse
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
    pub ppu_reg: [u8; PPU_REG_SIZE],

    pub io_latch_value: u8,

    pub status: StatusFlags,
    pub control1: Control1Flags,
    pub control2: Control2Flags,

    // PPUDATA reads go via a buffer (except for special behaviour for pallet reads)
    pub ppu_data_buffer: u8,

    // Request trigger for PPU address space
    pub written_oam_data: bool,   // OAM_DATAがかかれた
    pub written_ppu_scroll: bool, // PPU_SCROLLが2回書かれた
    pub written_ppu_addr: bool,   // PPU_ADDRが2回書かれた
    pub written_ppu_data: bool,   // PPU_DATAがかかれた
    pub read_oam_data: bool,      // OAM_DATAが読まれた
    pub read_ppu_data: bool,      // PPU_DATAが読まれた

    /* 2回海ができるPPU register対応 */
    /// $2005, $2006は状態を共有する、$2002を読み出すと、どっちを書くかはリセットされる
    pub ppu_is_second_write: bool, // 初期値falseで, 2回目の書き込みが分岐するようにtrueにする
    pub ppu_scroll_y_reg: u8,   // $2005
    pub ppu_addr_lower_reg: u8, // $2006

    /// PPUが描画に使うメモリ空間
    pub video: VRam,

    /// Object Attribute Memoryの実態
    pub oam: [u8; OAM_SIZE],
    /// 次の描画で使うスプライトを格納する
    pub sprite_temps: [Option<Sprite>; SPRITE_TEMP_SIZE],

    /// 積もり積もったcpu cycle, 341を超えたらクリアして1行処理しよう
    pub cumulative_cpu_cyc: usize,
    /// 次処理するy_index
    pub current_line: u16,

    // scrollレジスタは1lineごとに更新
    pub fetch_scroll_x: u8,
    pub fetch_scroll_y: u8,
    pub current_scroll_x: u8,
    pub current_scroll_y: u8,

    /// PPUの描画設定(step時に渡したかったが、毎回渡すのも無駄なので)
    pub draw_option: DrawOption,
}

impl Default for Ppu {
    fn default() -> Self {
        Self {
            io_latch_value: 0,

            ppu_reg: [0; PPU_REG_SIZE],
            ppu_data_buffer: 0,

            status: StatusFlags::empty(),
            control1: Control1Flags::empty(),
            control2: Control2Flags::empty(),

            video: Default::default(),

            written_oam_data: false,
            written_ppu_scroll: false,
            written_ppu_addr: false,
            written_ppu_data: false,
            read_oam_data: false,
            read_ppu_data: false,

            ppu_is_second_write: false,
            ppu_scroll_y_reg: 0,
            ppu_addr_lower_reg: 0,

            oam: [0; OAM_SIZE],
            sprite_temps: [None; SPRITE_TEMP_SIZE],

            cumulative_cpu_cyc: 0,
            current_line: 241,

            fetch_scroll_x: 0,
            fetch_scroll_y: 0,
            current_scroll_x: 0,
            current_scroll_y: 0,

            draw_option: DrawOption::default(),
        }
    }
}

impl EmulateControl for Ppu {
    fn poweron(&mut self) {

        self.video.poweron();

        self.ppu_reg = [0; PPU_REG_SIZE];
        self.status = StatusFlags::empty();
        self.control1 = Control1Flags::empty();
        self.control2 = Control2Flags::empty();

        self.written_oam_data = false;
        self.written_ppu_scroll = false;
        self.written_ppu_addr = false;
        self.written_ppu_data = false;
        self.read_oam_data = false;
        self.read_ppu_data = false;

        self.ppu_is_second_write = false;
        self.ppu_scroll_y_reg = 0;
        self.ppu_addr_lower_reg = 0;

        self.oam = [0; OAM_SIZE];
        self.sprite_temps = [None; SPRITE_TEMP_SIZE];

        self.current_line = 241;
        self.cumulative_cpu_cyc = 0;

        self.fetch_scroll_x = 0;
        self.fetch_scroll_y = 0;
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

    pub fn read_u8(&mut self, addr: u16) -> u8 {
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
                self.ppu_is_second_write = false;
                self.status.set(StatusFlags::IN_VBLANK, false);
                (data, StatusFlags::UNDEFINED_BITS.bits())
            }
            0x2003 => { // SPR-RAM Address Register (Write-only)
                (0, 0xff)
            }
            // OAM_DATAの読み出しフラグ
            // OAM_DATA read flag
            0x2004 => { // SPR-RAM I/O Register (Read/Write)
                self.read_oam_data = true;
                (arr_read!(self.ppu_reg, 4), 0)
            }
            0x2005 => { // VRAM Address Register 1 (Write-only)
                (0, 0xff)
            }
            0x2006 => { // VRAM Address Register 2 (Write-only)
                (0, 0xff)
            }
            // Since there is a buffer that sets a flag for PPU_DATA update / address increment,
            // the result will be entered with a delay of 1 step
            0x2007 => { // PPUDATA (Read/Write)
                self.read_ppu_data = true;
                (arr_read!(self.ppu_reg, 7), 0)
            }
            _ => unreachable!()
        };

        self.read_with_latch(value, undefined_bits)
    }

    pub fn write_u8(&mut self, addr: u16, data: u8) {
        // mirror support
        let addr = ((addr - 0x2000) % 8) + 0x2000;
        self.io_latch_value = data;
        match addr {
            0x2000 => { // Control 1
                self.control1 = Control1Flags::from_bits_truncate(data)
            }
            0x2001 => {  // Control 2
                self.control2 = Control2Flags::from_bits_truncate(data)
            }
            0x2002 => { // Status
                // Read Only
            }
            0x2003 => { // SPR-RAM Address Register
                arr_write!(self.ppu_reg, 3, data);
            }
            // $2004 If you write it in OAM_DATA, set a write flag (though you will not use it)
            0x2004 => {
                self.written_oam_data = true;
                arr_write!(self.ppu_reg, 4, data);
            }
            // $2005 PPU_SCROLL Written twice
            0x2005 => {
                if self.ppu_is_second_write {
                    self.ppu_scroll_y_reg = data;
                    self.ppu_is_second_write = false;
                    // PPUに通知
                    self.written_ppu_scroll = true;
                } else {
                    arr_write!(self.ppu_reg, 5, data);
                    self.ppu_is_second_write = true;
                }
            }
            // $2006 PPU_ADDR Written twice
            0x2006 => {
                if self.ppu_is_second_write {
                    self.ppu_addr_lower_reg = data;
                    self.ppu_is_second_write = false;
                    // PPUに通知
                    self.written_ppu_addr = true;
                } else {
                    arr_write!(self.ppu_reg, 6, data);
                    self.ppu_is_second_write = true;
                }
            }
            // $2007 PPU_DATA addr autoincrement
            0x2007 => {
                arr_write!(self.ppu_reg, 7, data);
                // PPUに書いてもらおう
                self.written_ppu_data = true;
            }
            _ => unreachable!()
        };
    }

    /// 1行書きます
    ///
    /// `tile_base`   - スクロールオフセット加算なしの現在のタイル位置
    /// `tile_global` - スクロールオフセット換算した、4面含めた上でのタイル位置
    /// `tile_local`  - `tile_global`を1Namespace上のタイルでの位置に変換したもの
    /// scrollなしなら上記はすべて一致するはず
    fn draw_line(&mut self, cartridge: &mut Cartridge, fb: *mut u8) {
        // ループ内で何度も呼び出すとパフォーマンスが下がる
        let nametable_base_addr = self.name_table_base_addr();
        let pattern_table_addr = self.bg_pattern_table_addr();
        let is_clip_bg_leftend = self.control2.contains(Control2Flags::BG_LEFT_COL_SHOW) == false;
        let is_write_bg = self.control2.contains(Control2Flags::SHOW_BG);
        let is_monochrome = self.control2.contains(Control2Flags::MONOCHROME);
        let master_bg_color = Color::from(self.video.read_u8(
            cartridge,
            PALETTE_TABLE_BASE_ADDR + PALETTE_BG_OFFSET,
        ));

        let raw_y = self.current_line + u16::from(self.current_scroll_y);
        let offset_y = raw_y & 0x07; // tile換算でのy位置から、実pixelのズレ(0~7)
        let tile_base_y = raw_y >> 3; // オフセットなしのtile換算での現在位置
                                      // scroll regはtile換算でずらす
        let tile_global_y = tile_base_y % (SCREEN_TILE_HEIGHT * 2); // tile換算でのy絶対座標
        let tile_local_y = tile_global_y % SCREEN_TILE_HEIGHT; // 1 tile内での絶対座標
                                                               // 4面ある内、下側に差し掛かっていたらfalse
        let is_nametable_position_top = tile_global_y < SCREEN_TILE_HEIGHT;

        // pixel formatの決定
        let pixel_indexes = match self.draw_option.pixel_format {
            PixelFormat::RGBA8888 => (0, 1, 2, 3),
            PixelFormat::BGRA8888 => (2, 1, 0, 3),
            PixelFormat::ARGB8888 => (1, 2, 3, 0),
        };

        // 描画座標系でループさせる
        let pixel_y = usize::from(self.current_line);
        for pixel_x in 0..VISIBLE_SCREEN_WIDTH {
            // Sprite: 探索したテンポラリレジスタから描画するデータを取得する
            let (sprite_palette_data_back, sprite_palette_data_front) =
                self.get_sprite_draw_data(cartridge, pixel_x, pixel_y);

            // BG(Nametable): 座標に該当するNametableと属性テーブルからデータを取得する
            let offset_x = ((pixel_x as u16) + u16::from(self.current_scroll_x)) & 0x07;
            let tile_base_x = ((pixel_x as u16) + u16::from(self.current_scroll_x)) >> 3;
            // scroll regはtile換算でずらす
            let tile_global_x = tile_base_x % (SCREEN_TILE_WIDTH * 2); // 4tile換算でのx絶対座標
            let tile_local_x = tile_global_x % SCREEN_TILE_WIDTH; // 1 tile内での絶対座標
            let is_nametable_position_left = tile_global_x < SCREEN_TILE_WIDTH; // 4面ある内、右側にある場合false

            // 4面あるうちのどれかがわかるので、該当する面のベースアドレスを返します
            let target_nametable_base_addr = nametable_base_addr +
                (if is_nametable_position_left { 0x0000 } else { 0x0400 }) + // 左右面の広域offset
                (if is_nametable_position_top  { 0x0000 } else { 0x0800 }); // 上下面の広域offset

            // attribute tableはNametableの後32byteにいるのでアドレス計算して読み出す。縦横4*4tileで1attrになっている
            // scroll対応のためにoffset計算はglobal位置を使っている（もしかしたら1Nametableでクリッピングがいるかも)
            let attribute_base_addr = target_nametable_base_addr + ATTRIBUTE_TABLE_OFFSET; // 23c0, 27c0, 2bc0, 2fc0のどれか
            let attribute_x_offset = (tile_global_x >> 2) & 0x7;
            let attribute_y_offset = tile_global_y >> 2;
            let attribute_addr =
                attribute_base_addr + (attribute_y_offset << 3) + attribute_x_offset;

            // attribute読み出し, BGパレット選択に使う。4*4の位置で使うパレット情報を変える
            let raw_attribute = self.video.read_u8(cartridge, attribute_addr);
            let bg_palette_id = match (tile_local_x & 0x03 < 0x2, tile_local_y & 0x03 < 0x2) {
                (true, true) => (raw_attribute >> 0) & 0x03,  // top left
                (false, true) => (raw_attribute >> 2) & 0x03, // top right
                (true, false) => (raw_attribute >> 4) & 0x03, // bottom left
                (false, false) => (raw_attribute >> 6) & 0x03, // bottom right
            };

            // Nametableからtile_id読み出し->pattern tableからデータ構築
            let nametable_addr = target_nametable_base_addr + (tile_local_y << 5) + tile_local_x;
            let bg_tile_id = u16::from(self.video.read_u8(cartridge, nametable_addr));

            // pattern_table 1entryは16byte, 0行目だったら0,8番目のデータを使えば良い
            let bg_pattern_table_base_addr = pattern_table_addr + (bg_tile_id << 4);
            let bg_pattern_table_addr_lower = bg_pattern_table_base_addr + offset_y;
            let bg_pattern_table_addr_upper = bg_pattern_table_addr_lower + 8;
            let bg_data_lower = self
                .video
                .read_u8(cartridge, bg_pattern_table_addr_lower);
            let bg_data_upper = self
                .video
                .read_u8(cartridge, bg_pattern_table_addr_upper);

            // bgの描画色を作る
            let bg_palette_offset = (((bg_data_upper >> (7 - offset_x)) & 0x01) << 1)
                | ((bg_data_lower >> (7 - offset_x)) & 0x01);
            let bg_palette_addr = (PALETTE_TABLE_BASE_ADDR + PALETTE_BG_OFFSET) +   // 0x3f00
                (u16::from(bg_palette_id) << 2) + // attributeでBG Palette0~3選択
                u16::from(bg_palette_offset); // palette内の色選択

            // BG左端8pixel clipも考慮してBGデータ作る
            let is_bg_clipping = is_clip_bg_leftend && (pixel_x < 8);
            let is_bg_tranparent = (bg_palette_addr & 0x03) == 0x00; // 背景色が選択された場合はここで処理してしまう
            let bg_palette_data: Option<u8> = if is_bg_clipping || !is_write_bg || is_bg_tranparent
            {
                None
            } else {
                Some(self.video.read_u8(cartridge, bg_palette_addr))
            };

            // 透明色
            let mut draw_color = master_bg_color;

            // 前後関係考慮して書き込む
            'select_color: for palette_data in &[
                sprite_palette_data_front,
                bg_palette_data,
                sprite_palette_data_back,
            ] {
                // 透明色判定をしていたら事前にNoneされている
                if let Some(color_index) = palette_data {
                    let c = Color::from(*color_index);
                    draw_color = c;
                    break 'select_color;
                }
            }

            // 毎回計算する必要のないものを事前計算
            let draw_base_y =
                self.draw_option.offset_y + (pixel_y as i32) * (self.draw_option.scale as i32);
            let draw_base_x =
                self.draw_option.offset_x + (pixel_x as i32) * (self.draw_option.scale as i32);
            // 座標計算, 1dotをscale**2 pixelに反映する必要がある
            for scale_y in 0..self.draw_option.scale {
                // Y座標を計算
                let draw_y = draw_base_y + (scale_y as i32);

                // Y座標がFrameBuffer範囲外
                if (draw_y < 0) || ((self.draw_option.fb_height as i32) <= draw_y) {
                    continue;
                }

                for scale_x in 0..self.draw_option.scale {
                    // X位置を求める
                    let draw_x = draw_base_x + (scale_x as i32);

                    // X座標がFrameBuffer範囲外
                    if (draw_x < 0) || ((self.draw_option.fb_width as i32) <= draw_x) {
                        continue;
                    }

                    // FrameBufferのサイズから、相当する座標を計算
                    // Y位置に相当するindex計算時の幅は256ではなくFrameBufferの幅を使う
                    let base_index = ((draw_y as isize) * (self.draw_option.fb_width as isize)
                        + (draw_x as isize))
                        * (NUM_OF_COLOR as isize);

                    unsafe {
                        let base_ptr = fb.offset(base_index);

                        // データをFBに反映
                        *base_ptr.offset(pixel_indexes.0) = draw_color.0; // R
                        *base_ptr.offset(pixel_indexes.1) = draw_color.1; // G
                        *base_ptr.offset(pixel_indexes.2) = draw_color.2; // B
                        *base_ptr.offset(pixel_indexes.3) = 0xff; // alpha blending

                        // モノクロ出力対応(とりあえず総加平均...)
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

    /// 指定されたpixelにあるスプライトを描画します
    /// `pixel_x` - 描画対象の表示するリーンにおけるx座標
    /// `pixel_y` - 描画対象の表示するリーンにおけるy座標
    /// retval - (bgよりも後ろに描画するデータ, bgより前に描画するデータ)
    fn get_sprite_draw_data(
        &mut self,
        cartridge: &mut Cartridge,
        pixel_x: usize,
        pixel_y: usize,
    ) -> (Option<u8>, Option<u8>) {
        // Sprite描画無効化されていたら即終了
        if !self.control2.contains(Control2Flags::SHOW_SPRITES) {
            return (None, None);
        }
        // Spriteを探索する (y位置的に描画しなければならないSpriteは事前に読み込み済)
        let mut sprite_palette_data_back: Option<u8> = None; // 背面
        let mut sprite_palette_data_front: Option<u8> = None; // 全面
        'draw_sprite: for &s in self.sprite_temps.iter() {
            if let Some(sprite) = s {
                // めんどいのでusizeにしておく
                let sprite_x = usize::from(sprite.x);
                let sprite_y = usize::from(sprite.y);
                // 左端sprite clippingが有効な場合表示しない
                let is_sprite_clipping = self.control2.contains(Control2Flags::SPRITES_LEFT_COL_SHOW) == false && (pixel_x < 8);
                // X位置が描画範囲の場合
                if !is_sprite_clipping
                    && (sprite_x <= pixel_x)
                    && (pixel_x < usize::from(sprite_x + SPRITE_WIDTH))
                {
                    // sprite上での相対座標
                    let sprite_offset_x: usize = pixel_x - sprite_x; // 0-7
                    let sprite_offset_y: usize = pixel_y - sprite_y - 1; // 0-7 or 0-15 (largeの場合, tile参照前に0-7に詰める)
                    debug_assert!(sprite_offset_x < SPRITE_WIDTH);
                    debug_assert!(sprite_offset_y < usize::from(self.sprite_height()));
                    // pattern table addrと、tile idはサイズで決まる
                    let (sprite_pattern_table_addr, sprite_tile_id): (u16, u8) = match sprite
                        .tile_id
                    {
                        TileId::Normal { id } => (self.sprites_pattern_table_addr(), id),
                        // 8*16 spriteなので上下でidが別れている
                        TileId::Large {
                            pattern_table_addr,
                            upper_tile_id,
                            lower_tile_id,
                        } => {
                            let is_upper = sprite_offset_y < SPRITE_NORMAL_HEIGHT; // 上8pixelの座標?
                            let is_vflip = sprite.attr.is_vert_flip; // 上下反転してる?
                            let id = match (is_upper, is_vflip) {
                                (true, false) => upper_tile_id,  // 描画座標は上8pixel、Flipなし
                                (false, false) => lower_tile_id, // 描画座標は下8pixel、Flipなし
                                (true, true) => lower_tile_id,   // 描画座標は上8pixel、Flipあり
                                (false, true) => upper_tile_id,  // 描画座標は下8pixel、Flipあり
                            };
                            (pattern_table_addr, id)
                        }
                    };
                    // x,y flipを考慮してtile上のデータ位置を決定する
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
                    // tile addrを計算する
                    let sprite_pattern_table_base_addr = u16::from(sprite_pattern_table_addr)
                        + (u16::from(sprite_tile_id) * PATTERN_TABLE_ENTRY_BYTE);
                    let sprite_pattern_table_addr_lower =
                        sprite_pattern_table_base_addr + (tile_offset_y as u16);
                    let sprite_pattern_table_addr_upper = sprite_pattern_table_addr_lower + 8;
                    let sprite_data_lower = self
                        .video
                        .read_u8(cartridge, sprite_pattern_table_addr_lower);
                    let sprite_data_upper = self
                        .video
                        .read_u8(cartridge, sprite_pattern_table_addr_upper);
                    // 該当するx位置のpixel patternを作る
                    let sprite_palette_offset =
                        (((sprite_data_upper >> (7 - tile_offset_x)) & 0x01) << 1)
                            | ((sprite_data_lower >> (7 - tile_offset_x)) & 0x01);
                    // paletteのアドレスを計算する
                    let sprite_palette_addr = (PALETTE_TABLE_BASE_ADDR + PALETTE_SPRITE_OFFSET) +        // 0x3f10
                        (u16::from(sprite.attr.palette_id) * PALETTE_ENTRY_SIZE) + // attributeでSprite Palette0~3選択
                        u16::from(sprite_palette_offset); // palette内の色選択
                                                          // パレットが透明色の場合はこのpixelは描画しない
                    let is_tranparent = (sprite_palette_addr & 0x03) == 0x00; // 背景色が選択された
                    if !is_tranparent {
                        // パレットを読み出し
                        let sprite_palette_data = self
                            .video
                            .read_u8(cartridge, sprite_palette_addr);
                        // 表裏の優先度がattrにあるので、該当する方に書き込み
                        if sprite.attr.is_draw_front {
                            sprite_palette_data_front = Some(sprite_palette_data);
                        } else {
                            sprite_palette_data_back = Some(sprite_palette_data);
                        }
                    }
                }
            } else {
                // sprite tempsは前詰めなのでもう処理はいらない
                break 'draw_sprite;
            }
        }
        // 描画するデータを返す
        (sprite_palette_data_back, sprite_palette_data_front)
    }

    /// OAMを探索して次の描画で使うスプライトをレジスタにフェッチします
    /// 8個を超えるとOverflowフラグを立てる
    fn fetch_sprite(&mut self) {
        // sprite描画無効化
        if !self.control2.contains(Control2Flags::SHOW_SPRITES) {
            return;
        }
        // スプライトのサイズを事前計算
        let sprite_begin_y = self.current_line;
        let sprite_height = u16::from(self.sprite_height());
        let is_large = sprite_height == 16;
        // とりあえず全部クリアしておく
        self.sprite_temps = [None; SPRITE_TEMP_SIZE];
        // current_line + 1がyと一致するやつを順番に集める(条件分がよりでかいにしてある)
        let mut tmp_index = 0;
        'search_sprite: for sprite_index in 0..NUM_OF_SPRITE {
            let target_oam_addr = sprite_index << 2;
            // yの値と等しい
            let sprite_y = u16::from(self.oam[target_oam_addr]);
            let sprite_end_y = sprite_y + sprite_height;
            // 描画範囲内(y+1)~(y+1+ 8or16)
            if (sprite_y < sprite_begin_y) && (sprite_begin_y <= sprite_end_y) {
                // sprite 0 hitフラグ(1lineごとに処理しているので先に立ててしまう)
                let is_zero_hit_delay = sprite_begin_y > (sprite_end_y - 3); //1lineずつ処理だとマリオ等早く検知しすぎるので TODO: #40
                if sprite_index == 0 && is_zero_hit_delay {
                    self.status.set(StatusFlags::SPRITE0_HIT, true);
                }
                // sprite overflow
                if tmp_index >= SPRITE_TEMP_SIZE {
                    self.status.set(StatusFlags::SPRITE_OVERFLOW, true);
                    break 'search_sprite;
                } else {
                    debug_assert!(tmp_index < SPRITE_TEMP_SIZE);
                    // tmp regに格納する
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


    /// 1行ごとに色々更新する処理です
    /// 341cyc溜まったときに呼び出されることを期待
    fn update_line(&mut self, cartridge: &mut Cartridge, fb: *mut u8) -> Option<Interrupt> {
        self.current_scroll_x = self.fetch_scroll_x;
        self.current_scroll_y = self.fetch_scroll_y;

        self.status.set(StatusFlags::SPRITE0_HIT, false);
        self.status.set(StatusFlags::SPRITE_OVERFLOW, false);

        match LineStatus::from(self.current_line) {
            LineStatus::Visible => {
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

                None
            }
        }
    }

    /// Proceed with PPU processing (it takes 341 cpu cycle to advance 1 line)
    /// `cpu_cyc` - Number of cpu clock cycles elapsed for last step of cpu.
    /// `cpu` - Interruptの要求が必要
    /// `system` - レジスタ読み書きする
    /// `video_system` - レジスタ読み書きする
    /// `videoout_func` - pixelごとのデータが決まるごとに呼ぶ(NESは出力ダブルバッファとかない)
    pub fn step(&mut self, cpu_cyc: usize, cartridge: &mut Cartridge, fb: *mut u8) -> Option<Interrupt> {
        // PPU_SCROLL書き込み
        let (_, scroll_x, scroll_y) = self.read_ppu_scroll();
        self.fetch_scroll_x = scroll_x;
        self.fetch_scroll_y = scroll_y;

        // PPU_ADDR, PPU_DATA読み書きに答えてあげる
        let (_, ppu_addr) = self.read_ppu_addr();
        let (is_read_ppu_req, is_write_ppu_req, ppu_data) = self.read_ppu_data();

        if is_write_ppu_req {
            self
                .video
                .write_u8(cartridge, ppu_addr, ppu_data);
            self.increment_ppu_addr();
        }
        if is_read_ppu_req {
            let data = self.video.read_u8(cartridge, ppu_addr);
            self.write_ppu_data(data);
            self.increment_ppu_addr();
        }

        // OAM R/W (It seems that it will not be used because it can be done by DMA)
        let oam_addr = self.read_ppu_oam_addr();
        let (is_read_oam_req, is_write_oam_req, oam_data) = self.read_oam_data();
        if is_write_oam_req {
            self.oam[usize::from(oam_addr)] = oam_data;
        }
        if is_read_oam_req {
            let data = self.oam[usize::from(oam_addr)];
            self.write_oam_data(data);
        }

        // clock cycle Judgment and row update
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
