use std::ops::Index;
use std::ops::IndexMut;

use anyhow::Result;
use anyhow::anyhow;
//use bitvec::BitArr;

use crate::color::Color32;
use crate::constants::FRAME_HEIGHT;
use crate::constants::FRAME_WIDTH;
use crate::framebuffer::PixelFormat;
use crate::ppu_palette::rgb_lut;
use crate::ppu_registers::Control1Flags;
use crate::ppu_registers::Control2Flags;
use crate::ppu_registers::StatusFlags;
use crate::cartridge::Cartridge;
use crate::framebuffer::Framebuffer;
use crate::framebuffer::FramebufferDataRental;
use crate::hook::{HooksList, HookHandle};
use crate::system::Model;

use crate::trace::TraceBuffer;
#[cfg(feature="trace-events")]
use crate::trace::TraceEvent;

//pub const CPU_CYCLE_PER_LINE: usize = 341 / 3; // ppu cyc -> cpu cyc
//pub const NUM_OF_COLOR: usize = 4;
//pub const FRAME_WIDTH: usize = 256;
//pub const FRAME_HEIGHT: usize = 240;
//pub const RENDER_SCREEN_WIDTH: u16 = FRAME_WIDTH as u16;
pub const N_LINES: u16 = 262;
pub const DOTS_PER_LINE: u16 = 341;
pub const NAMETABLE_PIXELS_PER_TILE: u16 = 8; // 1tile=8*8
pub const NAMETABLE_X_TILES_COUNT: u16 = (FRAME_WIDTH as u16) / NAMETABLE_PIXELS_PER_TILE; // 256/8=32
//pub const NAMETABLE_Y_TILES_COUNT: u16 = (FRAME_HEIGHT as u16) / PIXEL_PER_TILE; // 240/8=30
//pub const BG_NUM_OF_TILE_PER_ATTRIBUTE_TABLE_ENTRY: u16 = 4;
//pub const ATTRIBUTE_TABLE_WIDTH: u16 = NAMETABLE_X_TILES_COUNT / BG_NUM_OF_TILE_PER_ATTRIBUTE_TABLE_ENTRY;

const VT_HORIZONTAL_SCROLL_BITS_MASK: u16 = 0b0000_0100_0001_1111;
const VT_VERTICAL_SCROLL_BITS_MASK: u16 = 0b0111_1011_1110_0000;

pub const OAM_SIZE: usize = 64 * 4;
pub const PATTERN_TABLE_ENTRY_BYTE: u16 = 16;

//pub const SPRITE_TEMP_SIZE: usize = 8;
//pub const NUM_OF_SPRITE: usize = 64;
//pub const SPRITE_SIZE: usize = 4;
//pub const SPRITE_WIDTH: usize = 8;
//pub const SPRITE_NORMAL_HEIGHT: usize = 8;
//pub const SPRITE_LARGE_HEIGHT: usize = 16;
//pub const CYCLE_PER_DRAW_FRAME: usize = CPU_CYCLE_PER_LINE * ((RENDER_N_LINES + 1) as usize);

pub const PATTERN_TABLE_BASE_ADDR: u16 = 0x0000;
//pub const NAME_TABLE_BASE_ADDR: u16 = 0x2000;
//pub const NAME_TABLE_MIRROR_BASE_ADDR: u16 = 0x3000;
pub const PALETTE_TABLE_BASE_ADDR: u16 = 0x3f00;
//pub const VIDEO_ADDRESS_SIZE: u16 = 0x4000;

pub const NAME_TABLE_SIZE: usize = 0x0400;
//pub const NUM_OF_NAME_TABLE: usize = 2;
pub const ATTRIBUTE_TABLE_SIZE: u16 = 64;
pub const ATTRIBUTE_TABLE_OFFSET: u16 = 960; // 30x32 tiles

pub const PALETTE_SIZE: usize = 32;
pub const PALETTE_ENTRY_SIZE: u16 = 0x04;
pub const PALETTE_SPRITE_OFFSET: u16 = 0x10;

const FRAMEBUFFER_BPP: isize = 4;
const FRAMEBUFFER_STRIDE: isize = FRAME_WIDTH as isize * FRAMEBUFFER_BPP;


/// Closure type for the callback when a breakpoint is hit
pub type FnDotBreakpointCallback = dyn FnMut(&mut Ppu, u32, u16, u16) -> DotBreakpointCallbackAction;

/// Determines whether a breakpoint should be kept or removed after being hit
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DotBreakpointCallbackAction {
    Keep,
    Remove
}

/// A unique handle for a registered breakpoint that can be used to remove the breakpoint
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct DotBreakpointHandle(u32);

pub(super) struct DotBreakpoint {
    pub(super) handle: DotBreakpointHandle,
    pub(super) frame: Option<u32>,
    pub(super) line: Option<u16>,
    pub(super) dot: u16,
    pub(super) callback: Box<FnDotBreakpointCallback>
}

/// Debugger state attached to a PPU instance that won't be
/// cloned if the PPU is cloned but will be preserved through
/// a power cycle
#[derive(Default)]
pub struct NoCloneDebugState {
    #[cfg(feature="ppu-sim")]
    pub last_cartridge_read: Option<(u16, u8, u16, u16)>,

    #[cfg(feature="debugger")]
    pub(super) next_breakpoint_handle: u32,
    #[cfg(feature="debugger")]
    pub(super) breakpoints: Vec<DotBreakpoint>,
    #[cfg(feature="debugger")]
    pub breakpoint_hit: bool,

    #[cfg(feature="trace-events")]
    pub trace_events_current: TraceBuffer,
    #[cfg(feature="trace-events")]
    pub trace_events_prev: TraceBuffer,

    #[cfg(feature="ppu-hooks")]
    mux_hooks: HooksList<FnMuxHook>,

    #[cfg(feature="ppu-hooks")]
    dot_hooks: Vec<[HooksList<FnDotHook>; 341]>,
}
impl Clone for NoCloneDebugState {
    fn clone(&self) -> Self {
        Self::default()
    }
}

#[derive(Debug, Clone, Copy)]
pub enum MuxDecision {
    Sprite,
    Background,
    PaletteHackBackground,
    UniversalBackground
}
impl Default for MuxDecision {
    fn default() -> Self {
        MuxDecision::UniversalBackground
    }
}

/// All the state that a debugger can collect at the final stage of rendering each pixel
///
/// The background priority MUX takes a global background color, background fragment and
/// a sprite fragment and decides which one should be output for the final pixel.
///
/// Debuggers can register a hook into the MUX operation via [`Ppu::add_mux_hook`]
///
/// This state enables a debugger to build a decomposed view of the background
/// and sprite layers, as well as see sprite-zero hits and a break down of the
/// background/sprite pattern + palette state.
///
/// Since this hooks into the heart of the PPU emulator this state is a true
/// reflection of the data that was used to render which means it can also
/// potentially account for tricky mid-frame state changes (e.g. during hblank)
/// that may get lost by only inspecting nametable or sprite state that only renders
/// the current state, at a single point in time.
#[derive(Debug, Clone, Default)]
pub struct MuxHookState {
    pub rendering_enabled: bool,
    pub decision: MuxDecision,

    pub screen_x: u8,
    pub screen_y: u8,

    pub sprite_pattern: u8,
    pub sprite_palette: u8,
    pub sprite_zero: bool,
    pub sprite_zero_hit: bool,
    pub background_priority: bool,

    pub sprite_palette_value: u8,

    pub bg_pattern: u8,
    pub bg_palette: u8,
    pub bg_palette_value: u8,

    pub shared_v_register: u16,
    pub fine_x_scroll: u8,

    /// The palette value for the pixel, based on hue and brightness:
    /// ```text
    /// 76543210
    /// ||||||||
    /// ||||++++- Hue (phase, determines NTSC/PAL chroma)
    /// ||++----- Value (voltage, determines NTSC/PAL luma)
    /// ++------- Unimplemented, reads back as 0
    /// Each 8-bit sprite pixel contains:
    /// ```
    pub palette_value: u8,

    /// The currently BGR emphasis bits from the Control2 register
    /// ```text
    /// .....BGR
    ///      |||
    ///      ||+-- Red
    ///      |+--- Green
    ///      ++--- Blue
    /// ```
    pub emphasis: u8,

    /// The current monochrome state from the Control2 register
    pub monochrome: bool,
}

//type LineHookMask = BitArr!(for 262);
//type DotHookMask = BitArr!(for 341);


pub type FnDotHook = dyn FnMut(&mut Ppu, &mut Cartridge);
pub type FnMuxHook = dyn FnMut(&mut Ppu, &mut Cartridge, &MuxHookState);


#[derive(Copy, Clone)]
pub struct Position(pub u8, pub u8);

#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum LineStatus {
    /// Lines 0..=239
    #[default]
    Visible,
    /// Line 240
    PostRender,
    /// Lines 241..=260
    VerticalBlanking,
    /// Line 261
    PreRender,
}
impl LineStatus {
    fn from(line: u16) -> LineStatus {
        if line < 240 {
            LineStatus::Visible
        } else if line == 240 {
            LineStatus::PostRender
        } else if line < 261 {
            LineStatus::VerticalBlanking
        } else if line == 261 {
            LineStatus::PreRender
        } else {
            panic!("invalid line status");
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SpriteEvalState {
    #[default]
    Copying,
    LookingForOverflow,
    Done
}

/// Newtype for 256 byte array so we can impl Default
#[derive(Clone)]
pub struct Arr256([u8; 256]);
impl Default for Arr256 {
    fn default() -> Self { Self([0u8; 256]) }
}
impl Index<usize> for Arr256 {
    type Output = u8;
    fn index(&self, index: usize) -> &Self::Output { self.0.index(index) }
}
impl IndexMut<usize> for Arr256 {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output { self.0.index_mut(index) }
}
impl Arr256 {
    pub unsafe fn get_unchecked(&self, index: usize) -> &u8 {
        self.0.get_unchecked(index)
    }
    pub unsafe fn get_unchecked_mut(&mut self, index: usize) -> &mut u8 {
        self.0.get_unchecked_mut(index)
    }
}

#[derive(Clone, Default)]
pub struct Ppu {
    pub nes_model: Model,

    pub clock: u64,
    pub frame: u32,

    //pub is_rendering: bool,
    pub framebuffer: FramebufferDataRental,

    pub control1: Control1Flags,
    pub nmi_enable: bool,
    pub nmi_interrupt_raised: bool,

    pub frame_ready: bool,

    pub control2: Control2Flags,
    pub show_background: bool,
    pub show_sprites: bool,

    /// nesdev: "(i.e., when either background or sprite rendering is enabled in $2001:3-4)"
    pub rendering_enabled: bool,
    pub show_sprites_in_left_margin: bool,
    pub show_background_in_left_margin: bool,
    pub monochrome: bool,
    pub emphasize_red: bool,
    pub emphasize_green: bool,
    pub emphasize_blue: bool,

    pub status: StatusFlags,

    pub dot: u16, // wraps every 341 clock cycles
    pub line: u16,
    pub line_status: LineStatus,

    pub palette: [u8; PALETTE_SIZE],

    pub io_latch_value: u8,

    /// To (crudely) model the decay of the IO latch value then:
    /// - Each time a bit is updated it is added to the first keep-alive mask.
    /// - We periodically move the first mask to a second keep-alive mask before
    ///   clearing the first one.
    /// - Whenever a bit is not found in either of these masks before the first
    ///   mask is cleared then we know that bit hasn't been written recently and
    ///   it decays to zero.
    io_latch_keep_alive_masks: [u8; 2],
    io_latch_last_update_clock: u64,
    io_latch_decay_clock_period: u32,

    pub read_buffer: u8,

    pub shared_w_toggle: bool, // Latch for PPU_SCROLL and PPU_ADDR

    /// Shared temp for PPU_SCROLL and PPU_ADDR
    ///
    /// The layout of the (15bit) shared temp register when used
    /// for rendering / scrolling:
    /// ```text
    /// yyy NN YYYYY XXXXX
    /// ||| || ||||| +++++-- coarse X scroll
    /// ||| || +++++-------- coarse Y scroll
    /// ||| ++-------------- nametable select
    /// +++----------------- fine Y scroll
    /// ```
    pub shared_t_register: u16,

    /// Shared 'V' register, either configured directly via PPU_ADDR, or indirectly via PPU_SCROLL
    ///
    /// The layout of the (15bit) shared temp register when used
    /// for rendering / scrolling:
    /// ```text
    /// yyy NN YYYYY XXXXX
    /// ||| || ||||| +++++-- coarse X scroll
    /// ||| || +++++-------- coarse Y scroll
    /// ||| ++-------------- nametable select
    /// +++----------------- fine Y scroll
    /// ```
    pub shared_v_register: u16,

    pub scroll_x_fine3: u8,

    pub oam: Arr256,
    pub oam_offset: u8,

    /// The current state of OAM sprite evaluation
    oam_evaluate_state: SpriteEvalState,

    /// Last value read on each odd cycle
    oam_evaluate_read: u8,

    /// Write offset into secondary OAM while evaluating sprites
    secondary_oam_offset: usize,

    /// Number of in-range sprites found for the current line, added to secondary OAM
    oam_evaluate_n_sprites: u8,
    /// The current sprite index being evaluated
    oam_evaluate_n: usize,
    /// sprite byte offset for tracking completion of secondary oam copy and handling (buggy) overflow emulation
    oam_evaluate_m: usize,
    /// Is the sprite currently being read / evaluated from OAM in range
    oam_evaluate_sprite_in_range: bool,
    oam_evaluate_sprite_zero_in_range: bool,

    /// The index of the sprite currently being fetched
    sprite_fetch_index: usize,

    /// The pattern table row address (lower plane) for the current sprite, cached
    /// between reads of the low and high byte
    sprite_pattern_addr_lo: u16,

    pub secondary_oam_being_cleared: bool,
    pub secondary_oam: [u8; 4 * 8],

    /// A composed scanline of sprite pixels to be combined with the background, with priority rules
    ///
    /// Each 8-bit sprite pixel contains:
    /// ```text
    /// .. Z P LL TT
    ///    | | || ++--- pattern (0..=1)
    ///    | | ++------ palette (2..=3)
    ///    | +--------- background priority (4)
    ///    +----------- is sprite zero (5)
    /// ```
    sprite_line_back: Arr256,
    sprite_line_front: Arr256,

    // State latches that feed into the shift registers
    pub nametable_latch: u8,
    pub palette_latch: u8,
    pub pattern_table0_latch: u8,
    pub pattern_table1_latch: u8,

    // Every 8 pixels we load the pattern table bits for the next tile
    // into the lsb of these shift registers and the registers are
    // left shifted after each pixel
    // "The shifters are reloaded during ticks 9, 17, 25, ..., 257."
    pub pattern_table0_shift: u16,
    pub pattern_table1_shift: u16,

    // For consistency / simplicity we extend the 1bit latch values
    // across 8 bits so we get a 16bit shift register, the same
    // as for the pattern table bits.
    pub palette0_shift: u16,
    pub palette1_shift: u16,

    /// Which sub-tile pixel will be rendered next (wraps every 8 pixels)
    pub shift_pixel_x: u8,

    /// The offset for outputting the next pixel
    pub framebuffer_offset: isize,

    pub debug: NoCloneDebugState,
}

impl Ppu {

    pub fn new(nes_model: Model) -> Self {
        let clock_hz = match nes_model {
            Model::Ntsc => 5369318, // +- 10Hz
            Model::Pal => 4295454, // more like 4295454.4 +- 10Hz
        };

        // For now we have a conservative decay rate for the IO bus latch which will
        // decay bits to zero if they haven't been written for at _least_ half
        // a second
        let io_latch_decay_clock_period = clock_hz / 2;

        let framebuffer = Framebuffer::new(FRAME_WIDTH, FRAME_HEIGHT, PixelFormat::RGBA8888);
        let framebuffer = framebuffer.rent_data().unwrap();
        //let start_line = 241;
        let start_line = 0;
        Self {
            nes_model,
            io_latch_decay_clock_period,
            framebuffer,
            line: start_line,
            line_status: LineStatus::from(start_line),
            debug: NoCloneDebugState {
                dot_hooks: vec![[(); 341].map(|_| HooksList::default()); 262], // awkward because HooksList isn't Copy
                ..Default::default()
            },
            ..Default::default()
        }
    }

    pub fn power_cycle(&mut self) {
        // Note we preserve any debugger state / hooks
        let debug = std::mem::take(&mut self.debug);

        *self = Self {
            debug,
            ..Ppu::new(self.nes_model)
        }
    }

    pub fn reset(&mut self) {
        /* Actually - no reason why we can't trace across a reset
        #[cfg(feature="trace-events")]
        {
            self.trace_events[0].clear();
            self.trace_events[1].clear();
            self.trace_events_back = 0;
        }
        */
    }

    /// Allocate a framebuffer that can be used as a PPU render target.
    ///
    /// Returns a new framebuffer that can later be associated with the PPU via [`Self::swap_framebuffer`]
    pub fn alloc_framebuffer(&self) -> Framebuffer {
        Framebuffer::new(256, 240, PixelFormat::RGBA8888)
    }

    /// Associate a new framebuffer with the PPU for rendering
    ///
    /// While the framebuffer is associated with the PPU the PPU will rent access to the underlying data
    /// and so you must swap with a new framebuffer before being able to rent access to the data
    /// for presenting
    pub fn swap_framebuffer(&mut self, framebuffer: Framebuffer) -> Result<Framebuffer> {
        if let Some(rental) = framebuffer.rent_data() {
            let old = self.framebuffer.owner();
            self.framebuffer = rental;
            Ok(old)
        } else {
            Err(anyhow!("Failed to rent access to framebuffer data for rendering"))
        }
    }

    /// Records an internal event into the back trace buffer
    ///
    /// This is a debug mechanism for being able to track mid-frame events which a
    /// debug tool can plot onto an expanded (341 x 262) framebuffer view covering
    /// the full dot clock range for a frame
    #[cfg(feature="trace-events")]
    #[inline(always)]
    pub fn trace(&mut self, event: TraceEvent) {
        self.debug.trace_events_current.push(event);
    }

    #[cfg(feature="trace-events")]
    #[inline(always)]
    pub fn trace_start_of_line(&mut self, cpu_clock: u64, new_frame: bool) {
        if new_frame {
            std::mem::swap(&mut self.debug.trace_events_current, &mut self.debug.trace_events_prev);
            self.debug.trace_events_current.clear();
        }
        self.trace(TraceEvent::PpuCpuLineSync { cpu_clk: cpu_clock, ppu_clk: self.clock, line: self.line });
    }

    #[inline(always)]
    fn is_rendering(&self) -> bool {
        matches!(self.line_status, LineStatus::Visible | LineStatus::PreRender) && self.rendering_enabled
    }

    /// .....BGR
    ///      |||
    ///      ||+-- Red
    ///      |+--- Green
    ///      ++--- Blue
    #[inline(always)]
    fn emphasis(&self) -> u8 {
        (self.control2 & Control2Flags::EMPHASIS).bits() >> 5
    }

    fn update_nmi(&mut self) {
        if self.nmi_enable && self.status.contains(StatusFlags::IN_VBLANK) {
            self.nmi_interrupt_raised = true;
        } else {
            self.nmi_interrupt_raised = false;
        }
    }

    pub fn palette_peek(&self, addr: u16) -> u8 {
        let index = usize::from(addr - PALETTE_TABLE_BASE_ADDR) % PALETTE_SIZE;

        // "Addresses $3F10/$3F14/$3F18/$3F1C are mirrors of
        // $3F00/$3F04/$3F08/$3F0C. Note that this goes for writing as well as
        // reading. A symptom of not having implemented this correctly in an
        // emulator is the sky being black in Super Mario Bros., which writes
        // the backdrop color through $3F10."
        let mut value = match index {
            0x10 => self.palette[0x00],
            0x14 => self.palette[0x04],
            0x18 => self.palette[0x08],
            0x1c => self.palette[0x0c],
            _ => arr_read!(self.palette, index),
        };

        // "Bit 0 controls a greyscale mode, which causes the palette to use
        // only the colors from the grey column: $00, $10, $20, $30. This is
        // implemented as a bitwise AND with $30 on any value read from PPU
        // $3F00-$3FFF, both on the display and through PPUDATA. Writes to the
        // palette through PPUDATA are not affected. Also note that black
        // colours like $0F will be replaced by a non-black grey $00."
        if self.monochrome {
            value &= 0x30;
        }

        value
    }

    pub fn palette_read(&self, addr: u16) -> u8 {
        self.palette_peek(addr)
    }

    pub fn palette_write(&mut self, addr: u16, data: u8) {
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

    /// Perform a buffered ppu data read for register $2007
    ///
    /// This will also update the the internal buffer, ready for the next read
    ///
    /// Returns (value, undefined_bit_mask)
    fn buffered_ppu_data_read(&mut self, cartridge: &mut Cartridge, addr: u16) -> (u8, u8) {
        if let 0x3f00..=0x3fff = addr { // Pallet reads bypass buffering
            self.read_buffer = cartridge.vram_read(addr);
            (self.palette_read(addr), 0xc0)
        } else {
            let buffered = self.read_buffer;
            self.read_buffer = cartridge.vram_read(addr);
            (buffered, 0)
        }
    }

    /// Peek what a buffered ppu data read (via $2007) would fetch without any side effects
    pub fn buffered_ppu_data_peek(&mut self, _cartridge: &mut Cartridge, addr: u16) -> (u8, u8) {
        if let 0x3f00..=0x3fff = addr { // Pallet reads bypass buffering
            (self.palette_read(addr), 0xc0)
        } else {
            (self.read_buffer, 0)
        }
    }

    /// Perform a ppu data write (via $2007)
    pub fn ppu_data_write(&mut self, cartridge: &mut Cartridge, addr: u16, data: u8) {
        if let 0x3f00..=0x3fff = addr {
            //println!("palette write: addr={addr:x}, data={data:x}");
            self.palette_write(addr, data);
        } else {
            //println!("data write: addr={addr:x}, data={data:x}");
            cartridge.vram_write(addr, data);
        }
        //println!("ppu bus wrote 0x{addr:04x} = 0x{data:02x}, h={}, v={}, RB=0x{:02x}", self.dot, self.line, self.read_buffer);
    }

    /// Perform an unbuffered ppu bus read
    fn unbuffered_ppu_bus_read(&mut self, cartridge: &mut Cartridge, addr: u16) -> u8 {
        let value = if let 0x3f00..=0x3fff = addr {
            self.palette_read(addr)
        } else {
            let value= cartridge.vram_read(addr);

            // So we can do a running comparison of what the emulated PPU reads vs
            // what the simulated PPU reads we trace each read operation
            #[cfg(feature="ppu-sim")]
            {
                self.debug.last_cartridge_read = Some((addr, value, self.dot, self.line));
            }

            value
        };

        //println!("ppu bus read 0x{addr:04x} = 0x{value:02x}, h={}, v={}", self.dot, self.line);
        //if self.dot == 259 {
        //    panic!("unexpected PPU read");
        //}
        value
    }

    /// Peek what an unbuffered ppu bus read would fetch without any side effects
    pub fn unbuffered_ppu_bus_peek(&mut self, cartridge: &mut Cartridge, addr: u16) -> u8 {
        self.unbuffered_ppu_bus_read(cartridge, addr) // reading currently has no side effects
    }

    pub fn increment_data_addr(&mut self, cartridge: &mut Cartridge) {

        // nesdev:
        //
        // "Outside of rendering, reads from or writes to $2007 will add either
        // 1 or 32 to v depending on the VRAM increment bit set via $2000.
        // During rendering (on the pre-render line and the visible lines
        // 0-239, provided either background or sprite rendering is enabled),
        // it will update v in an odd way, triggering a coarse X increment and
        // a Y increment simultaneously (with normal wrapping behavior).
        //
        // Internally, this is caused by the carry inputs to various sections
        // of v being set up for rendering, and the $2007 access triggering a
        // "load next value" signal for all of v (when not rendering, the carry
        // inputs are set up to linearly increment v by either 1 or 32)"

        if !self.is_rendering() {
            self.shared_v_register = self.shared_v_register.wrapping_add(self.address_increment());

            // "During VBlank and when rendering is disabled, the value on
            // the PPU address bus is the current value of the v register."
            cartridge.ppu_bus_nop_io(self.shared_v_register);
        } else {
            self.increment_coarse_x_scroll();
            self.increment_fine_y_scroll();
        }
        //println!("inc data address: scroll_tile_x = {}, scroll_tile_y = {}", self.scroll_tile_x(), self.scroll_tile_y());
    }

    // TODO: decay the latch value over time
    //pub fn finish_read_with_latch(&mut self, value: u8, undefined_bits: u8) -> u8 {
    //    let read = (value & !undefined_bits) | (self.io_latch_value & undefined_bits);
    //    self.io_latch_value = (self.io_latch_value & undefined_bits) | (value & !undefined_bits);
    //    read
    //}

    pub fn finish_peek_with_latch(&mut self, value: u8, undefined_bits: u8) -> u8 {
        let read = (value & !undefined_bits) | (self.io_latch_value & undefined_bits);
        read
    }

    /// Directly read OAM data from the current OAMADDR offset
    #[inline]
    fn read_oam_data(&self, offset: u8) -> u8 {
        //"The three unimplemented bits of each sprite's byte 2 do not exist in the PPU and always read back as
        // 0 on PPU revisions that allow reading PPU OAM through OAMDATA ($2004)"
        arr_read!(self.oam, offset as usize)
    }

    /// Directly write OAM data to the current OAMADDR offset
    #[inline]
    fn write_oam_data(&mut self, offset: u8, val: u8) {
        //"The three unimplemented bits of each sprite's byte 2 do not exist in the PPU and always read back as
        // 0 on PPU revisions that allow reading PPU OAM through OAMDATA ($2004)"
        //
        // Since we read more-often than we write then we emulate this on writes instead of reads
        if offset & 0b11 == 2 {
            let masked_val = val & 0b1110_0011;
            //println!("OAM[{offset}] = {masked_val:02x} (masked from {val:02x})");
            arr_write!(self.oam, offset as usize, masked_val);
        } else {
            //println!("OAM[{offset}] = {val:02x}");
            arr_write!(self.oam, offset as usize, val);
        }
    }

    /// Read OAM data via $2004 register
    fn read_oam_data_register(&self) -> u8 {
        // "Cycles 1-64: Secondary OAM (32-byte buffer for current
        // sprites on scanline) is initialized to $FF - attempting to
        // read $2004 will return $FF. Internally, the clear operation
        // is implemented by reading from the OAM and writing into the
        // secondary OAM as usual, only a signal is active that makes
        // the read always return $FF."
        if self.secondary_oam_being_cleared {
            0xff
        } else {
            self.read_oam_data(self.oam_offset)
        }
    }

    pub fn peek_oam_data(&self, offset: u8) -> u8 {
        self.read_oam_data(offset)
    }

    /// Read without applying open bus latch value
    ///
    /// Returns (value, undefined bits mask)
    fn read_without_openbus(&mut self, cartridge: &mut Cartridge, addr: u16) -> (u8, u8) {
        // mirror
        let addr = ((addr - 0x2000) % 8) + 0x2000;
        match addr {
            0x2000 => { // Control (Write-only)
                (0, 0xff)
            }
            0x2001 => {  // Mask (Write-only)
                (0, 0xff)
            }
            // PPU_STATUS (read-only) Resets double-write register status, clears VBLANK flag
            0x2002 => { // Status (Read-only)
                let data = self.status.bits();
                self.shared_w_toggle = false;
                //self.ppu_is_second_write = false;
                self.status.set(StatusFlags::IN_VBLANK, false);
                //println!("Clear IN_VBLANK flag (status read)");
                (data, StatusFlags::UNDEFINED_BITS.bits())
            }
            0x2003 => { // OAMADDR (Write-only)
                (0, 0xff)
            }
            0x2004 => { // OAMDATA (Read/Write)
                (self.read_oam_data_register(), 0x0)
            }
            0x2005 => { // PPU_SCROLL (Write-only)
                (0, 0xff)
            }
            0x2006 => { // PPU_ADDR (Write-only)
                (0, 0xff)
            }
            0x2007 => { // PPU_DATA (Read/Write)
                let (data, undefined_mask) = self.buffered_ppu_data_read(cartridge, self.shared_v_register);
                self.increment_data_addr(cartridge);
                (data, undefined_mask)
            }
            _ => unreachable!()
        }
    }

    pub fn system_bus_read(&mut self, cartridge: &mut Cartridge, addr: u16) -> (u8, u8) {
        self.read_without_openbus(cartridge, addr)
        //let (value, undefined_bits) = self.read_without_openbus(cartridge, addr);
        //let value = self.finish_read_with_latch(value, undefined_bits);
        //println!("ppu read 0x{addr:04x} = 0x{value:02x}");
        //value
    }

    pub fn system_bus_peek(&mut self, cartridge: &mut Cartridge, addr: u16) -> (u8, u8) {
        // mirror
        let addr = ((addr - 0x2000) % 8) + 0x2000;
        let (value, undefined_bits) = match addr {
            0x2002 => { // Status (Read-only)
                let data = self.status.bits();
                (data, StatusFlags::UNDEFINED_BITS.bits())
            }
            0x2004 => { // OamData
                (self.read_oam_data_register(), 0x0)
            }
            0x2007 => { // PPU_DATA (Read/Write)
                self.buffered_ppu_data_peek(cartridge, self.shared_v_register)
            }
            _ => (0, 0xff)
        };

        //self.finish_peek_with_latch(value, undefined_bits)

        (value, undefined_bits)
    }

    pub fn system_bus_write(&mut self, cartridge: &mut Cartridge, addr: u16, data: u8) {
        //println!("CPU->PPU write 0x{:04x} = 0x{:02x}", addr, data);
        // mirror
        let addr = ((addr - 0x2000) % 8) + 0x2000;
        //self.io_latch_value = data;
        match addr {
            0x2000 => { // Control 1
                self.control1 = Control1Flags::from_bits_truncate(data);
                self.nmi_enable = self.control1.contains(Control1Flags::NMI_ENABLE);
                self.update_nmi();

                // The lower nametable bits become 10-11 of the shared (15 bit) temp register that's
                // used by PPU_SCROLL and PPU_ADDR
                self.shared_t_register = (self.shared_t_register & 0b0111_0011_1111_1111) | ((data as u16 & 0b11) << 10);
            }
            0x2001 => {  // Control 2

                self.control2 = Control2Flags::from_bits_truncate(data);
                self.show_background = self.control2.contains(Control2Flags::SHOW_BG);
                self.show_sprites = self.control2.contains(Control2Flags::SHOW_SPRITES);
                self.rendering_enabled = self.show_background || self.show_sprites;
                self.show_sprites_in_left_margin = self.control2.contains(Control2Flags::SPRITES_LEFT_COL_SHOW);
                self.show_background_in_left_margin = self.control2.contains(Control2Flags::BG_LEFT_COL_SHOW);
                self.monochrome = self.control2.contains(Control2Flags::MONOCHROME);
                //self.monochrome = true;
                self.emphasize_red = self.control2.contains(Control2Flags::EMPHASIZE_RED);
                self.emphasize_green = self.control2.contains(Control2Flags::EMPHASIZE_GREEN);
                self.emphasize_blue = self.control2.contains(Control2Flags::EMPHASIZE_BLUE);
                //println!("PPU Control2 write = {:08b}: rendering_enabled = {:?}", data, self.rendering_enabled);
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
                //println!("oam write {:x} = {data:x}", self.oam_offset);

                // nesdev:
                //
                // "Writes to OAMDATA during rendering (on the pre-render line
                // and the visible lines 0-239, provided either sprite or
                // background rendering is enabled) do not modify values in OAM,
                // but do perform a glitchy increment of OAMADDR, bumping only
                // the high 6 bits (i.e., it bumps the [n] value in PPU sprite
                // evaluation - it's plausible that it could bump the low bits
                // instead depending on the current status of sprite
                // evaluation). This extends to DMA transfers via OAMDMA, since
                // that uses writes to $2004"
                //
                // "For emulation purposes, it is probably best to completely ignore
                //  writes during rendering."
                if !self.is_rendering() {
                    self.write_oam_data(self.oam_offset, data);
                    self.oam_offset = self.oam_offset.wrapping_add(1);
                } else {
                    self.oam_offset = self.oam_offset.wrapping_add(4);
                }
            }
            0x2005 => { // PPU_SCROLL

                if self.shared_w_toggle {
                    let fine3_y = (data & 0b111) as u16;
                    let coarse5_y = ((data & 0b1111_1000) >> 3) as u16;
                    self.shared_t_register = (self.shared_t_register & 0b0000_1100_0001_1111) | (fine3_y << 12) | (coarse5_y << 5);

                    // TODO: supporting mid-frame updates from t -> v
                    //self.update_scroll_xy();
                } else {
                    self.scroll_x_fine3 = data & 0b111;
                    self.shared_t_register = (self.shared_t_register & 0b0111_1111_1110_0000) | (((data >> 3) as u16) & 0b1_1111);
                }
                self.shared_w_toggle = !self.shared_w_toggle;
            }
            0x2006 => { // PPU_ADDR
                if self.shared_w_toggle {
                    let lsb = data;
                    self.shared_t_register = (self.shared_t_register & 0xff00) | (lsb as u16);
                    self.shared_v_register = self.shared_t_register;

                    // "During VBlank and when rendering is disabled, the value on
                    // the PPU address bus is the current value of the v register."
                    if !self.is_rendering() {
                        //println!("Notifying cartridge of PPUADDR update {:04X}", self.shared_v_register);
                        cartridge.ppu_bus_nop_io(self.shared_v_register);
                    } else {
                        //println!("PPU_ADDR write with rendering enabled")
                    }
                    //println!("PPU ADDR write 2 = {data:02x}, new PPU DATA address = 0x{:04x}, scroll_tile_x = {}, scroll_tile_y = {}", self.shared_v_register, self.scroll_tile_x(), self.scroll_tile_y());
                } else {
                    //println!("PPU ADDR write 1 = {data:02x}");
                    // NB: shared_temp (t) is a 15 bit register that's shared between
                    // PPU_ADDR and PPU_SCROLL. Also note the PPU only has a 14bit address
                    // space for vram and the first write to $2006 will set the upper
                    // bits of the shared_temp address except with the top bit of the
                    // address cleared (so we clear the top two bits since we're storing
                    // as a 16 bit value)
                    //
                    let msb = data & 0b0011_1111;
                    self.shared_t_register = ((msb as u16) << 8) | (self.shared_t_register & 0xff);
                }
                self.shared_w_toggle = !self.shared_w_toggle;
            }
            0x2007 => { // PPU_DATA
                //println!("data_write_u8: {:x}, {data:x}", self.shared_vram_addr);
                //debug_assert!(self.line >239);
                //println!("ppu data: writing = {:02x} to {:04x}", data, self.shared_v_register);
                self.ppu_data_write(cartridge, self.shared_v_register, data);
                self.increment_data_addr(cartridge);
            }
            _ => unreachable!()
        };
    }

    fn clear_secondary_oam(&mut self) {
        self.secondary_oam = [0xff; 32];
    }

    fn start_sprite_evaluation(&mut self) {
        //println!("Starting sprite evaluation, secondary OAM = {:x?}", self.secondary_oam);
        //println!("Starting sprite evaluation, OAM = {:x?}", self.oam);

        // The value of OAMADDR when sprite evaluation starts at tick 65 of the
        // visible scanlines will determine where in OAM sprite evaluation
        // starts, and hence which sprite gets treated as sprite 0. The first
        // OAM entry to be checked during sprite evaluation is the one starting
        // at OAM[OAMADDR]. If OAMADDR is unaligned and does not point to the y
        // position (first byte) of an OAM entry, then whatever it points to
        // (tile index, attribute, or x coordinate) will be reinterpreted as a y
        // position, and the following bytes will be similarly reinterpreted. No
        // more sprites will be found once the end of OAM is reached,
        // effectively hiding any sprites before OAM[OAMADDR].

        self.secondary_oam_offset = 0;
        self.oam_evaluate_state = SpriteEvalState::Copying;
        self.oam_evaluate_n = (self.oam_offset >> 2) as usize;
        self.oam_evaluate_m = (self.oam_offset & 0b11) as usize;
        self.oam_evaluate_sprite_in_range = false;
        self.oam_evaluate_sprite_zero_in_range = false;
        self.oam_evaluate_n_sprites = 0;
    }

    #[inline]
    fn is_sprite_in_range(&self, sprite_y: u8) -> bool {
        let line = self.line as u8;
        line >= sprite_y && line < (sprite_y + self.sprite_height())
    }

    fn step_sprite_evaluation(&mut self) {

        // Note: sprite evaluation may be done even if sprite rendering is disabled:
        //  "With background rendering on, sprite evaluation will resume but remain hidden"

        // nesdev:
        //
        // During all visible scanlines, the PPU scans through OAM to determine which sprites to render on the next scanline. Sprites found to be within
        // range are copied into the secondary OAM, which is then used to initialize eight internal sprite output units.
        //
        // OAM[n][m] below refers to the byte at offset 4*n + m within OAM, i.e. OAM byte m (0-3) of sprite n (0-63).
        //
        // # Cycles 65-256: Sprite evaluation
        // On odd cycles, data is read from (primary) OAM
        // On even cycles, data is written to secondary OAM (unless secondary OAM is full, in which case it will read the value in secondary OAM instead)
        //  1. Starting at n = 0, read a sprite's Y-coordinate (OAM[n][0], copying it to the next open slot in secondary OAM (unless 8 sprites
        //     have been found, in which case the write is ignored).
        //    1a. If Y-coordinate is in range, copy remaining bytes of sprite data (OAM[n][1] thru OAM[n][3]) into secondary OAM.
        //  2. Increment n
        //    2a. If n has overflowed back to zero (all 64 sprites evaluated), go to 4
        //    2b. If less than 8 sprites have been found, go to 1
        //    2c. If exactly 8 sprites have been found, disable writes to secondary OAM because it is full. This causes sprites in back to drop out.
        //  3. Starting at m = 0, evaluate OAM[n][m] as a Y-coordinate.
        //    3a. If the value is in range, set the sprite overflow flag in $2002 and read the next 3 entries of OAM
        //        (incrementing 'm' after each byte and incrementing 'n' when 'm' overflows); if m = 3, increment n
        //    3b. If the value is not in range, increment n and m (without carry). If n overflows to 0, go to 4; otherwise go to 3
        //        _The m increment is a hardware bug - if only n was incremented, the overflow flag would be set whenever more than
        //        8 sprites were present on the same scanline, as expected._
        //  4. Attempt (and fail) to copy OAM[n][0] into the next free slot in secondary OAM, and increment n (repeat until HBLANK is reached)
        //
        // # Notes
        //
        // The value of OAMADDR when sprite evaluation starts at tick 65 of the
        // visible scanlines will determine where in OAM sprite evaluation
        // starts, and hence which sprite gets treated as sprite 0. The first
        // OAM entry to be checked during sprite evaluation is the one starting
        // at OAM[OAMADDR]. If OAMADDR is unaligned and does not point to the y
        // position (first byte) of an OAM entry, then whatever it points to
        // (tile index, attribute, or x coordinate) will be reinterpreted as a y
        // position, and the following bytes will be similarly reinterpreted. No
        // more sprites will be found once the end of OAM is reached,
        // effectively hiding any sprites before OAM[OAMADDR].
        //
        // Sprite evaluation does not happen on the pre-render scanline. Because evaluation applies to the next line's sprite rendering, no sprites will
        // be rendered on the first scanline, and this is why there is a 1 line offset on a sprite's Y coordinate.
        //
        // Sprite evaluation occurs if either the sprite layer or background layer is enabled via $2001. Unless both layers are disabled, it merely hides sprite rendering.
        //
        // Sprite evaluation does not cause sprite 0 hit. This is handled by sprite rendering instead.

        if self.dot & 1 != 0 {
            // "On odd cycles, data is read from (primary) OAM"
            self.oam_evaluate_read = self.read_oam_data(self.oam_offset);
            //println!("Read OAM[{}] = {}", self.oam_offset, self.oam_evaluate_read);
        } else {
            //println!("OAM evaluate state = {:?}, line = {}", self.oam_evaluate_state, self.line);
            match self.oam_evaluate_state {
                SpriteEvalState::Copying => {
                    self.secondary_oam[self.secondary_oam_offset] = self.oam_evaluate_read;

                    if self.secondary_oam_offset % 4 == 0 {
                        self.oam_evaluate_sprite_in_range = self.is_sprite_in_range(self.oam_evaluate_read);
                        if self.secondary_oam_offset == 0 {
                            self.oam_evaluate_sprite_zero_in_range = self.oam_evaluate_sprite_in_range;
                        }
                        if self.oam_evaluate_sprite_in_range {
                            //println!("Found in-range sprite[{}] @ dot = {}, line = {}, Y = {}", self.secondary_oam_offset / 4, self.dot, self.line, self.oam_evaluate_read);
                            self.oam_evaluate_n_sprites += 1;
                        }
                    }
                    if self.oam_evaluate_sprite_in_range {
                        self.secondary_oam_offset += 1;

                        // XXX: not sure this add with carry behaviour is quite how the HW works.
                        //
                        // Even though the above notes from nesdev seem to say the hardware will blindly read unaligned OAM data
                        // that seems highly suspicious and I think that should be confirmed.

                        self.oam_evaluate_m = (self.oam_evaluate_m + 1) % 4;
                        if self.oam_evaluate_m == 0 {
                            self.oam_evaluate_n += 1;
                        }
                    } else {
                        self.oam_evaluate_n += 1;
                    }
                    if self.oam_evaluate_n >= 64 {
                        self.oam_evaluate_state = SpriteEvalState::Done;
                    } else if self.secondary_oam_offset >= 32 {
                        self.oam_evaluate_state = SpriteEvalState::LookingForOverflow;
                    }
                }
                SpriteEvalState::LookingForOverflow => {
                    if self.is_sprite_in_range(self.oam_evaluate_read) {
                        // 9th in-range sprite found
                        //println!("Found 9th sprite @ dot = {}, line = {}", self.dot, self.line);
                        debug_assert_eq!(self.oam_evaluate_n_sprites, 8);
                        self.status.set(StatusFlags::SPRITE_OVERFLOW, true);
                        self.oam_evaluate_state = SpriteEvalState::Done;
                    } else {
                        // Emulate overflow bug where the hardware effectively traverses diagonally in memory
                        // incrementing the sprite index _and_ the byte offset
                        // XXX: I wonder how this interacts with OAMDATA being unaligned at the start of evaluation
                        self.oam_evaluate_n += 1;
                        self.oam_evaluate_m = (self.oam_evaluate_m + 1) % 4;
                        if self.oam_evaluate_n >= 64 {
                            self.oam_evaluate_state = SpriteEvalState::Done;
                        }
                    }
                }
                SpriteEvalState::Done => {
                    //  "4. Attempt (and fail) to copy OAM[n][0] into the next free slot in secondary OAM, and increment n (repeat until HBLANK is reached)"

                    // " a side effect of the OAM write disable signal is to turn
                    // writes to the secondary OAM into reads from it. Once eight
                    // in-range sprites have been found, the value being read during
                    // write cycles from that point on is the y coordinate of the
                    // first sprite copied into the secondary OAM."

                    // The main significance for this is that it results in reading in a Y
                    // value that's guaranteed to be in-range which is then what causes the (real)
                    // hardware to step the 'm' read offset when it shouldn't.

                    self.oam_evaluate_read = self.secondary_oam[0];
                    self.oam_evaluate_n = (self.oam_evaluate_n + 1) % 64;
                }
            }
        }

        // We have decomposed the oam offset for sprite evaluation (including emulating the overflow bug) but
        // need to recompose it since self.read_oam_data() will refer to .oam_offset.
        self.oam_offset = ((self.oam_evaluate_n % 64) as u8) << 2 | (self.oam_evaluate_m as u8) & 0b11;
        //println!("Updating OAM offset = {}, N = {}, M = {}", self.oam_offset, self.oam_evaluate_n, self.oam_evaluate_m);
    }


    /// Compose the sprite into an intermediate `sprite_line_back`, in priority order (i.e. lowest secondary oam index to highest)
    /// This avoids needing more intricate shift register emulation for all the sprites while rendering the background
    /// Each pixel contains:
    /// bits 0-1 = pattern
    /// bits 2-3 = palette
    /// bit 4 = background priority
    /// bit 5 = is zero sprite
    fn compose_sprite(&mut self) {

        // Don't cull sprites at this stage in case this flag gets changed by the time we render the final pixels
        //if !self.control2.contains(Control2Flags::SHOW_SPRITES) {
        //    return;
        //}

        if self.sprite_fetch_index >= self.oam_evaluate_n_sprites as usize {
            return
        }

        //println!("line = {}, composing sprite {} of {}", self.line, self.sprite_fetch_index, self.oam_evaluate_n_sprites);

        // We store the sprite-zero state in bit 5
        let sprite_zero_bit = if self.sprite_fetch_index == 0 && self.oam_evaluate_sprite_zero_in_range { 0b0010_0000u8 } else { 0 };

        let sprite_attr = self.secondary_oam[self.sprite_fetch_index * 4 + 2];
        let sprite_palette_bits = (sprite_attr & 0b11) << 2;
        let sprite_priority_bit = if sprite_attr & 0b0010_0000 != 0 { 0b0001_0000 } else { 0 }; // we store the priority state in bit 4
        let x_flip = sprite_attr & 0b0100_0000 != 0;
        //let y_flip = sprite_attr & 0b1000_0000 != 0;
        let sprite_x = self.secondary_oam[self.sprite_fetch_index * 4 + 3] as usize;
        let sprite_row_pattern_low = self.pattern_table0_latch;
        let sprite_row_pattern_hi = self.pattern_table1_latch;

        // Don't handle clipping here in case it gets changed by the time we render the final pixels
        //let clip_left_margin = !self.control2.contains(Control2Flags::SPRITES_LEFT_COL_SHOW);
        for x in 0..8 {
            //if x < 8 && clip_left_margin {
            //    continue;
            //}

            let screen_x = sprite_x + x;
            if screen_x <= 255 {
                let existing = self.sprite_line_back[screen_x];
                // "the first non-transparent pixel moves on to a multiplexer, where it joins the BG pixel."
                if existing & 0b11 == 0 {
                    let sprite_pattern = if x_flip {
                        Ppu::right_select_bits_from_lo_hi_u8(sprite_row_pattern_low, sprite_row_pattern_hi, x)
                    } else {
                        Ppu::left_select_bits_from_lo_hi_u8(sprite_row_pattern_low, sprite_row_pattern_hi, x)
                    };
                    if sprite_pattern & 0b11 != 0 {
                        self.sprite_line_back[screen_x as usize] = sprite_zero_bit | sprite_priority_bit | sprite_palette_bits | sprite_pattern;
                        //if sprite_zero_bit != 0 {
                        //    println!("Sprite Zero: Writing sprite_line[{}] = {:02x}, sz = {:08b}, sp = {:08b}, plt = {:08b}, pat = {:08b}",
                        //            screen_x, self.sprite_line_back[screen_x as usize], sprite_zero_bit, sprite_priority_bit, sprite_palette_bits, sprite_pattern);
                        //}
                    }
                }
            }
        }
    }

    #[allow(dead_code)]
    fn print_shift_register_state(&mut self) {
        println!("Shift registers @ dot = {}, line = {}", self.dot, self.line);
        for i in 0..16 {
            let pattern = self.select_bg_pattern_from_shift_registers(i);
            let palette = self.select_bg_palette_from_shift_registers(i);
            println!("{i}) palette = {}, pattern = {}", palette, pattern);
        }

        println!("Latched state:");
        for i in 0..8 {
            let pattern = Ppu::left_select_bits_from_lo_hi_u8(self.pattern_table0_latch, self.pattern_table1_latch, i);
            println!("{i}) palette = {}, pattern = {}", self.palette_latch, pattern);
        }
    }

    /// Query the current pattern state from the shift registers, for debugging
    pub fn peek_shift_register_patterns(&self) -> [u8; 16] {
        let mut patterns = [0u8; 16];
        for i in 0..16 {
            patterns[i] = self.select_bg_pattern_from_shift_registers(i);
        }
        patterns
    }

    /// Query the current palette state from the shift registers, for debugging
    pub fn peek_shift_register_palettes(&self) -> [u8; 8] {
        let mut palettes = [0u8; 8];
        for i in 0..8 {
            palettes[i] = self.select_bg_palette_from_shift_registers(i);
        }
        palettes
    }

    /// Load data for the next tile (8 pixel span) into the low 8 bits of our shift registers
    fn reload_shift_registers(&mut self) {
        self.pattern_table0_shift = self.pattern_table0_shift & 0xff00 | self.pattern_table0_latch as u16;
        self.pattern_table1_shift = self.pattern_table1_shift & 0xff00 | self.pattern_table1_latch as u16;

        // The palette bits will be constant for the next tile so we extend into 8 bits for
        // consistency with the pattern table shift registers (the actual hardware instead has
        // 1bit latches to feed the constant into the shift register for 8 pixels)
        let palette0_bits = if self.palette_latch & 1 != 0 { 0xffu16 } else { 0u16 };
        let palette1_bits = if self.palette_latch & 2 != 0 { 0xffu16 } else { 0u16 };
        self.palette0_shift = self.palette0_shift & 0xff00 | palette0_bits as u16;
        self.palette1_shift = self.palette1_shift & 0xff00 | palette1_bits as u16;

        /*
        if self.line == 261 || self.line == 0 {
            //println!("Reloaded shift registers: dot = {}, line = {}", self.dot, self.line);
            self.print_shift_register_state();
        }*/
    }

    #[inline]
    fn shift_registers(&mut self) {
        self.pattern_table0_shift <<= 1;
        self.pattern_table1_shift <<= 1;
        self.palette0_shift <<= 1;
        self.palette1_shift <<= 1;
        self.shift_pixel_x = (self.shift_pixel_x + 1) % 8;
    }

    fn read_nametable_byte(&mut self, cartridge: &mut Cartridge) {
        let tile_address = self.scroll_tile_address();
        self.nametable_latch = self.unbuffered_ppu_bus_read(cartridge, tile_address);

        /*
        if self.dot == 1 {
            if self.rendering_enabled && self.line != 261 {
                debug_assert_eq!(self.scroll_tile_x(), 2); // First two tiles are read at end of previous line
                if self.line == 0 {
                    debug_assert_eq!(self.scroll_tile_y(), 0);
                    debug_assert_eq!(self.scroll_fine_y(), 0);
                }
            }
        }
        */
        //if self.scroll_tile_x() == 0 && self.scroll_tile_y() == 29 && self.scroll_fine_y() == 0 {
        //    println!("read NT byte 0,29 @ 0x{tile_address:04x} = {:02x}, dot = {}, line = {}", self.nametable_latch, self.dot, self.line);
        //}

    }

    fn read_attribute_table_byte(&mut self, cartridge: &mut Cartridge) {
        let attr_address = self.scroll_attribute_address();
        let attr_value = self.unbuffered_ppu_bus_read(cartridge, attr_address);

        let tile_x = self.scroll_tile_x();
        let tile_y = self.scroll_tile_y();
        self.palette_latch = match (tile_y & 2)  | ((tile_x >> 1) & 1) {
            0 => attr_value & 0b11, // Top-left
            1 => (attr_value & 0b1100) >> 2, // Top-right
            2 => (attr_value & 0b11_0000) >> 4, // Bottom-left
            3 => (attr_value & 0b1100_0000) >> 6, // Bottom-right
            _ => unreachable!()
        };

        /*
        if self.line == 261 && self.dot > 320 {
            println!("prerender read attribute @ 0x{attr_address:04x} = {:02x}, palette_latch = {:08b}/{:02x}, dot = {}, line = {}, tile_x = {}, tile_y = {}", attr_value, self.palette_latch, self.palette_latch, self.dot, self.line, tile_x, tile_y);
        }
        if self.scroll_tile_x() == 0 && self.scroll_tile_y() == 29 && self.scroll_fine_y() == 0 {
            println!("read attribute byte 0,29 @ 0x{attr_address:04x} = {:02x}, palette_latch = {:08b}/{:02x}, dot = {}, line = {}", attr_value, self.palette_latch, self.palette_latch, self.dot, self.line);
        }
        */

    }

    fn read_pattern_table_low_byte(&mut self, cartridge: &mut Cartridge) {
        let pattern_table_index = self.nametable_latch;
		let bg_pattern_table_addr_lower = self.bg_pattern_table_addr() |
			((pattern_table_index as u16) << 4) | (self.scroll_fine_y() as u16);
        self.pattern_table0_latch = self.unbuffered_ppu_bus_read(cartridge, bg_pattern_table_addr_lower);

        /*
        if self.scroll_tile_x() == 0 && self.scroll_tile_y() == 29 && self.scroll_fine_y() == 0 {
            println!("read pattern table low byte @ 0x{bg_pattern_table_addr_lower:04x} = {:08b}/{:02x}", self.pattern_table0_latch, self.pattern_table0_latch);
        }
        */
    }

    fn read_pattern_table_high_byte(&mut self, cartridge: &mut Cartridge) {
        let pattern_table_index = self.nametable_latch;
		let bg_pattern_table_addr_lower = self.bg_pattern_table_addr() |
			((pattern_table_index as u16) << 4) | (self.scroll_fine_y() as u16);
        self.pattern_table1_latch = self.unbuffered_ppu_bus_read(cartridge, bg_pattern_table_addr_lower + 8);

        /*
        if self.scroll_tile_x() == 0 && self.scroll_tile_y() == 29 && self.scroll_fine_y() == 0 {
            let bg_pattern_table_addr_high = bg_pattern_table_addr_lower + 8;
            println!("read pattern table high byte @ 0x{bg_pattern_table_addr_high:04x} = {:08b}/{:02x}", self.pattern_table1_latch, self.pattern_table1_latch);
        }*/
    }

    /// Calculate the pattern table row address (lower plane) for the 8x16 sprite currently being fetched
    fn current_sprite8x16_pattern_table_row_address(&self) -> u16 {
        // XXX: if the 'current' sprite state was stored in the Ppu struct we could avoid
        // a bunch of bounds checks here
        let sprite_index_byte = self.secondary_oam[self.sprite_fetch_index * 4 + 1];
        let sprite_tile_index = sprite_index_byte & 0xfe;
        let sprite_y = self.secondary_oam[self.sprite_fetch_index * 4];
        let y_flip = self.secondary_oam[self.sprite_fetch_index * 4 + 2] & 0x80 != 0;

        let line = self.line as u8;

        // Make sure we get an in-bounds line between 0..8 even if the current
        // sprite is actually out-of-range since we may be effectively fetching
        // garbage just for consistent timing (some mappers drive clocks based
        // on ppu fetches)
        let delta = line.wrapping_sub(sprite_y) % 16;
        let sprite_row = if y_flip {
            15 - delta
        } else {
            delta
        };
        let row_offset = if sprite_row > 8 { 8 + sprite_row } else { sprite_row };

        let pattern_table_base = if sprite_index_byte & 1 != 0 { 0x1000 } else { 0x0000 };
        pattern_table_base | ((sprite_tile_index as u16) << 4) | (row_offset as u16)
    }

    /// Calculate the pattern table row address (lower plane) for the 8x8 sprite currently being fetched
    fn current_sprite8x8_pattern_table_row_address(&self) -> u16 {
        // XXX: if the 'current' sprite state was store in the Ppu struct we could avoid
        // a bunch of bounds checks here
        let sprite_tile_index = self.secondary_oam[self.sprite_fetch_index * 4 + 1];
        let sprite_y = self.secondary_oam[self.sprite_fetch_index * 4];
        let y_flip = self.secondary_oam[self.sprite_fetch_index * 4 + 2] & 0x80 != 0;

        let line = self.line as u8;

        // Make sure we get an in-bounds line between 0..8 even if the current
        // sprite is actually out-of-range since we may be effectively fetching
        // garbage just for consistent timing (some mappers drive clocks based
        // on ppu fetches)
        let delta = line.wrapping_sub(sprite_y) % 8;
        let row_offset = if y_flip {
            7 - delta
        } else {
            delta
        };
        let pattern_table_base = self.sprites8x8_pattern_table_addr();
        pattern_table_base | ((sprite_tile_index as u16) << 4) | (row_offset as u16)
    }

    fn calculate_sprite_pattern_table_row_address(&self) -> u16 {
        //println!("sprite fetch index = {}, sprite count = {}", self.sprite_fetch_index, self.oam_evaluate_n_sprites);
        if self.sprite_height() == 8 {
            self.current_sprite8x8_pattern_table_row_address()
        } else {
            self.current_sprite8x16_pattern_table_row_address()
        }
    }

    fn fetch_sprite_pattern_low_byte(&mut self, addr_lower: u16, cartridge: &mut Cartridge) {
        self.pattern_table0_latch = self.unbuffered_ppu_bus_read(cartridge, addr_lower);
    }

    fn fetch_sprite_pattern_high_byte(&mut self, addr_high: u16, cartridge: &mut Cartridge) {

        self.pattern_table1_latch = self.unbuffered_ppu_bus_read(cartridge, addr_high);
    }

    #[inline]
    fn select_bg_pattern_from_shift_registers(&self, fine_x: usize) -> u8 {
        let pattern0 = ((self.pattern_table0_shift << fine_x) & 0x8000) >> 15;
        let pattern1 = ((self.pattern_table1_shift << fine_x) & 0x8000) >> 14;
        let bits = pattern1 as u8 | pattern0 as u8;

        /*
        if self.scroll_tile_x() == 0 && self.scroll_tile_y() == 29 && self.scroll_fine_y() == 0 {
            println!("selected pattern bits = {:08b}/{:02x}", bits, bits);
        }*/

        bits
    }

    /// Counting from the left, with the most-significant, left bit being
    /// selected as index `0`, this selects corresponding bits from given `lo`
    /// and `hi` values and forms a 2-bit number with the `hi` bit as the
    /// most-significant
    #[inline]
    fn left_select_bits_from_lo_hi_u16(lo: u16, hi: u16, select: usize) -> u8 {
        let bit0 = ((lo << select) & 0x8000) >> 15;
        let bit1 = ((hi << select) & 0x8000) >> 14;
        (bit1 | bit0) as u8
    }

    /// Counting from the left, with the most-significant, left bit being
    /// selected as index `0`, this selects corresponding bits from given `lo`
    /// and `hi` values and forms a 2-bit number with the `hi` bit as the
    /// most-significant
    #[inline]
    fn left_select_bits_from_lo_hi_u8(lo: u8, hi: u8, select: usize) -> u8 {
        //println!("left selecting bit {select}");
        let bit0 = ((lo << select) & 0x80) >> 7;
        let bit1 = ((hi << select) & 0x80) >> 6;
        bit1 | bit0
    }

    /// Counting from the right, with the least-significant, right bit being
    /// selected as index `0`, this selects corresponding bits from given `lo`
    /// and `hi` values and forms a 2-bit number with the `hi` bit as the
    /// most-significant
    #[inline]
    fn right_select_bits_from_lo_hi_u8(lo: u8, hi: u8, select: usize) -> u8 {
        let bit0 = (lo >> select) & 1;
        let bit1 = ((hi >> select) & 1) << 1;
        bit1 | bit0
    }

    #[inline]
    fn select_bg_palette_from_shift_registers(&self, fine_x: usize) -> u8 {
        let bits = Ppu::left_select_bits_from_lo_hi_u16(self.palette0_shift, self.palette1_shift, fine_x);

        /*
        if self.scroll_tile_x() == 0 && self.scroll_tile_y() == 29 && self.scroll_fine_y() == 0 {
            println!("selected palette bits = {:08b}/{:02x}", bits, bits);
        }*/

        bits
    }

    /*
    pub fn decode_scroll_xy(&self) -> (u8, u8) {
        let coarse_x = ((self.shared_t_register & 0b11111) << 3) as u8;
        let fine_x = self.scroll_x_fine3 & 0b111;
        let scroll_x = coarse_x | fine_x;
        let coarse_y = ((self.shared_t_register & 0b11_1110_0000) >> 2) as u8;
        let fine_y = ((self.shared_t_register & 0b0111_0000_0000_0000) >> 12) as u8;
        let scroll_y  = coarse_y | fine_y;

        //println!("scroll_x = {}, scroll_y = {}", self.current_scroll_x, self.current_scroll_y);
        (scroll_x, scroll_y)
    }*/

    /// Combines a nametable offset with the coarse and fine, horizontal scroll factors
    ///
    /// For debugging this combines all the horizontal scroll factors into a 9-bit
    /// number that can extend across two logical nametables.
    pub fn scroll_x(&self) -> u16 {
        let nametable_x = (self.shared_v_register & 0b0000_0100_0000_0000) >> 2;
        let coarse_x = (self.scroll_tile_x() as u16) << 3; // 5 bits
        let fine_x = self.scroll_x_fine3 as u16; // 3 bits
        nametable_x | coarse_x | fine_x
    }

    /// Combines a nametable offset with the coarse and fine, vertical scroll factors
    ///
    /// For debugging this combines all the vertical scroll factors into a 9-bit
    /// number that can extend across two logical nametables.
    ///
    /// NB: It's possible for this to be out-of-range since there are only 240 lines
    /// to the screen, not 256. This is the range that the hardware supports and some
    /// games even exploit this to make the hardware overrun and read attribute data
    /// as if it were nametable data for moving the top row out of overscan.
    pub fn scroll_y(&self) -> u16 {
        let nametable_y = (self.shared_v_register & 0b0000_1000_0000_0000) >> 3;
        let coarse_y = (self.scroll_tile_y() as u16) << 3; // 5 bits
        let fine_y   = self.scroll_fine_y() as u16; // 3 bits
        nametable_y | coarse_y | fine_y
    }

    /*
    #[inline]
    pub fn scroll_fine_y(&self) -> u16 {
        (self.shared_v_register & 0b0111_0000_0000_0000) >> 12
    }

    #[inline]
    pub fn scroll_coarse_y(&self) -> u16 {
        (self.shared_v_register & 0b0000_0011_1110_0000) >> 2
    }

    #[inline]
    pub fn scroll_nametable_offset_y(&self) {
        (self.shared_v_register & 0b0000_1000_0000_0000) >> 3
    }

    pub fn name_table_base_addr(&self) -> u16 {
        //(self.shared_v_register & 0b0000_1100_0000_0000) + 0x2000

        match self.control1 & Control1Flags::NAME_TABLE_MASK {
            Control1Flags::NAME_TABLE_0 => 0x2000,
            Control1Flags::NAME_TABLE_1 => 0x2400,
            Control1Flags::NAME_TABLE_2 => 0x2800,
            Control1Flags::NAME_TABLE_3 => 0x2c00,
            _ => panic!("invalid name table addr index"),
        }

    }
    */

    /// Returns the horizontal nametable tile offset (within a single nametable)
    /// NB: There are 32 horizontal tiles per nametable
    pub fn scroll_tile_x(&self) -> u8 {
        (self.shared_v_register & 0b1_1111) as u8
    }

    /// Returns the horizontal nametable tile offset (within a single nametable)
    ///
    /// NB: There are 30 vertical tiles per nametable but it's technically
    /// possible (and of course some games do this) to index up to 32 tiles
    /// and cause the hardware to overrun and read attribute data as if it
    /// were nametable data!
    pub fn scroll_tile_y(&self) -> u8 {
        ((self.shared_v_register & 0b0000_0011_1110_0000) >> 5) as u8
    }

    /// Returns the pixel-level vertical scroll within the current nametable tile
    ///
    /// NB: Nametable tiles are 8x8 pixels
    pub fn scroll_fine_y(&self) -> u8 {
        ((self.shared_v_register & 0b0111_0000_0000_0000) >> 12) as u8
    }

    /// Derives the nametable address from the current scroll offsets
    ///
    /// This is the address that is read every 8 clocks to read the nametable entry
    /// for the next tile.
    pub fn scroll_tile_address(&self) -> u16 {
        0x2000 | (self.shared_v_register & 0x0FFF)
    }

    /// Derives the attribute table address from the current scroll offsets
    ///
    /// NB: The attribute table comes 960 bytes after the start of its corresponding
    /// nametable and is 64 bytes long, with a quarter the resolution of the
    /// nametable
    pub fn scroll_attribute_address(&self) -> u16 {
        0x2000 | // Nametable 0 base address
            self.shared_v_register & 0b0000_1100_0000_0000 | // Nametable offset
            960 | // 960 byte offset to attribute table
            (self.shared_v_register >> 4) & 0x38 | // Coarse X / 4
            (self.shared_v_register >> 2) & 0x07 // Coarse Y / 4
    }

    /// Increments the horizontal nametable tile offset with wrapping after two nametables
    ///
    /// NB: nametables are logically arranged in a 2x2 grid and the coarse scroll specifies
    /// the tile-aligned origin for the screen within those four nametables.
    fn increment_coarse_x_scroll(&mut self) {
        let nametable_select_x = (self.shared_v_register & 0b0000_0100_0000_0000) >> 5;
        let coarse_x = self.shared_v_register & 0b0000_0000_0001_1111;
        let mut coarse_x = nametable_select_x | coarse_x;
        coarse_x += 1;
        let nametable_select_x = (coarse_x & 0b10_0000) << 5;
        let coarse_x = coarse_x & 0b01_1111;

        self.shared_v_register = (self.shared_v_register & VT_VERTICAL_SCROLL_BITS_MASK) | nametable_select_x | coarse_x;
        //println!("Increment coarse x scroll: (line = {}, dot = {}), scroll_tile_x = {}, scroll_tile_y = {}", self.line, self.dot, self.scroll_tile_x(), self.scroll_tile_y());
    }

    /// Increments the vertical nametable tile offset with wrapping after two nametables
    ///
    /// NB: nametables are logically arranged in a 2x2 grid and the coarse scroll specifies
    /// the tile-aligned origin for the screen within those four nametables.
    ///
    /// Note: considering that vertical scrolling needs to wrap at 240 pixels which isn't a neat
    /// power of two, then the hardware actually has some slightly funky behaviour here whereby
    /// you can actually overrun 240 pixels if you program the scroll registers directly via
    /// $2005 which can skip over the correct wrap handling for the nametable select. This
    /// results in the PPU reading attribute data as if it were nametable data!
    ///
    /// From nesdev:
    /// > "Coarse Y can be set out of bounds (> 29), which will cause the PPU
    /// > to read the attribute data stored there as tile data. If coarse Y is
    /// > incremented from 31, it will wrap to 0, but the nametable will not
    /// > switch. For this reason, a write >= 240 to $2005 may appear as a
    /// > "negative" scroll value, where 1 or 2 rows of attribute data will
    /// > appear before the nametable's tile data is reached. (Some games use
    /// > this to move the top of the nametable out of the Overscan area.)"
    fn increment_fine_y_scroll(&mut self) {
        let mut nametable_select_y  = self.shared_v_register & 0b0000_1000_0000_0000;
        let mut coarse_y = self.scroll_tile_y() as u16;
        let fine_y   = self.scroll_fine_y() as u16 + 1;
        if fine_y == 8 {
            if coarse_y == 29 {
                nametable_select_y = nametable_select_y ^ 0b0000_1000_0000_0000;
                coarse_y = 0;
            } else if coarse_y == 31 {
                coarse_y = 0;
            } else {
                coarse_y += 1;
            }
        }

        let coarse_y = (coarse_y & 0b1_1111) << 5;
        let fine_y   = (fine_y & 0b111) << 12;

        self.shared_v_register = (self.shared_v_register & VT_HORIZONTAL_SCROLL_BITS_MASK) | nametable_select_y | coarse_y | fine_y;
        //println!("Increment fine Y scroll: scroll_tile_x = {}, scroll_tile_y = {}, scroll_fine_y = {}", self.scroll_tile_x(), self.scroll_tile_y(), self.scroll_fine_y());
    }

    #[cfg(feature="ppu-hooks")]
    #[inline]
    fn call_mux_hooks(&mut self, cartridge: &mut Cartridge, state: &MuxHookState) {
        let mut hooks = std::mem::take(&mut self.debug.mux_hooks);
        for hook in hooks.hooks.iter_mut() {
            (hook.func)(self, cartridge, &state);
        }
        std::mem::swap(&mut self.debug.mux_hooks, &mut hooks);
    }

    /// Combines background and sprite pixels according to priority rules
    ///
    /// Also handles sprite zero hit detection and left margin clipping
    fn background_priority_mux(&mut self, bg_palette: u8, bg_pattern: u8, sprite_pix: u8, screen_x: usize, cartridge: &mut Cartridge) -> Color32 {

        //let is_sprite_zero_debug = sprite_pix & 0b10_0000 != 0;
        //if is_sprite_zero_debug {
        //    println!("PX: line = {}, screen_x = {screen_x}, sprite pix = {sprite_pix:02x}m bg_pattern = {bg_pattern:02x}", self.line);
        //}

        let sprite_bg_priority = sprite_pix & 0b01_0000 != 0;

        let sprite_pattern = if self.show_sprites && (self.show_sprites_in_left_margin || screen_x >= 8) {
            sprite_pix & 0b11
        } else {
            0
        };

        // Determine background clipping up-front, even if we might not select the background color
        // to simplify sprite0 hit detection
        let bg_pattern = if self.show_background && (self.show_background_in_left_margin || screen_x >= 8) {
            bg_pattern
        } else {
            0
        };

        //if is_sprite_zero_debug {
        //    println!("PX: line = {}, screen_x = {screen_x}, sprite pattern = {sprite_pattern:02x}, bg pattern = {bg_pattern:02x}", self.line);
        //}

        // "when an opaque pixel of sprite 0 overlaps an opaque pixel of the background, this is a sprite zero hit"
        //
        // Sprite 0 hit does not happen:
        // - If background or sprite rendering is disabled in PPUMASK ($2001)
        // - At x=0 to x=7 if the left-side clipping window is enabled (if bit 2 or bit 1 of PPUMASK is 0).
        // - At x=255, for an obscure reason related to the pixel pipeline.
        // - At any pixel where the background or sprite pixel is transparent (2-bit color index from the CHR pattern is %00).
        // - If sprite 0 hit has already occurred this frame. Bit 6 of PPUSTATUS ($2002) is cleared to 0 at dot 1 of the pre-render line. This means only the first sprite 0 hit in a frame can be detected.
        //
        // Sprite 0 hit happens regardless of the following:
        // - Sprite priority. Sprite 0 can still hit the background from behind.
        // - The pixel colors. Only the CHR pattern bits are relevant, not the actual rendered colors, and any CHR color index except %00 is considered opaque.
        // - The palette. The contents of the palette are irrelevant to sprite 0 hits. For example: a black ($0F) sprite pixel can hit a black ($0F) background as long as neither is the transparent color index %00.
        // - The PAL PPU blanking on the left and right edges at x=0, x=1, and x=254 (see Overscan).
        let is_sprite_zero = sprite_pix & 0b10_0000 != 0;
        let sprite_zero_hit = is_sprite_zero && sprite_pattern != 0 && bg_pattern != 0 && screen_x != 255;
        if sprite_zero_hit {
            //println!("PX: line = {}, screen_x = {screen_x}: SPRITE ZERO HIT", self.line);
            self.status.set(StatusFlags::SPRITE0_HIT, true);
        }

        let sprite_priority = sprite_pattern != 0 && (sprite_bg_priority == false || bg_pattern == 0);
        let (palette_addr, pattern) = if sprite_priority {
            let sprite_palette = (sprite_pix & 0b1100) >> 2;
            let addr = (0x3f10 | (sprite_palette as u16) << 2, sprite_pattern);
            //if is_sprite_zero {
            //    println!("PX: line = {}, screen_x = {screen_x}, sprite pix = {sprite_pix:02x}", self.line);
            //    println!("Reading from sprite palette @ {:04x}", addr.0 + addr.1 as u16);
            //}
            addr
        } else if bg_pattern != 0 {
            (0x3f00 | (bg_palette as u16) << 2, bg_pattern)

            //if self.scroll_tile_x() == 0 && self.scroll_tile_y() == 29  {
            //    println!("bg palette address 0,29 = {:04x}: palette bits = {:08b}, pattern bits = {:08b}, dot = {}, line = {}", bg_palette_addr, bg_palette, bg_pattern, self.dot, self.line);
            //}
        } else {
            (0x3f00, 0) // universal background color
        };

        let palette_value = self.palette_read(palette_addr + pattern as u16);

        let color = rgb_lut(palette_value);

        #[cfg(feature="ppu-hooks")]
        if self.debug.mux_hooks.hooks.len() > 0 {
            // Copied code from above
            let sprite_palette = (sprite_pix & 0b1100) >> 2;
            let sprite_palette_addr = 0x3f10 | (sprite_palette as u16) << 2 | sprite_pattern as u16;
            let bg_state = bg_palette << 2 | bg_pattern;
            let bg_palette_addr = 0x3f00 | bg_state as u16;

            let state = MuxHookState {
                rendering_enabled: true,
                decision: if sprite_priority { MuxDecision::Sprite } else if bg_pattern != 0 { MuxDecision::Background } else { MuxDecision::UniversalBackground },
                screen_x: screen_x as u8,
                screen_y: self.line as u8,
                sprite_palette,
                sprite_pattern,
                sprite_zero: is_sprite_zero,
                sprite_zero_hit,
                background_priority: sprite_bg_priority,
                //sprite_state: sprite_pix | if sprite_zero_hit { 1u8<<6 } else { 0 },
                sprite_palette_value: self.palette_peek(sprite_palette_addr),
                bg_palette,
                bg_pattern,
                //bg_state,
                bg_palette_value: self.palette_peek(bg_palette_addr),

                shared_v_register: self.shared_v_register,
                fine_x_scroll: self.scroll_x_fine3,

                palette_value,
                emphasis: self.emphasis(),
                monochrome: self.monochrome,
            };
            self.call_mux_hooks(cartridge, &state);
        }

        color
    }

    fn compose_enabled_pixel(&mut self, screen_x: usize, _screen_y: usize, cartridge: &mut Cartridge) -> Color32 {
        let bg_palette = self.select_bg_palette_from_shift_registers(self.scroll_x_fine3 as usize);
        let bg_pattern = self.select_bg_pattern_from_shift_registers(self.scroll_x_fine3 as usize);

        // Uncomment to just show the attribute/palette colors across each tile, ignoring the pattern
        //let mut tile_quadrant =  if screen_y % 8 > 3 { 2 } else { 0 };
        //tile_quadrant += if screen_x % 8 > 3 { 1 } else { 0 };
        //let bg_pattern_bits = tile_quadrant; // HACK

        //let bg_pattern_bits = 2u8; // HACK

        let sprite_pixel = self.sprite_line_front[screen_x];
        let color = self.background_priority_mux(bg_palette, bg_pattern, sprite_pixel, screen_x, cartridge);

        color
    }

    fn compose_disabled_pixel(&mut self, screen_x: usize, _screen_y: usize, cartridge: &mut Cartridge) -> Color32 {

        // "During forced blanking, when neither background nor sprites are
        // enabled in PPUMASK ($2001), the picture will show the backdrop color"
        //
        // # The background palette hack
        //
        // "If the current VRAM address points in the range $3F00-$3FFF during
        // forced blanking, the color indicated by this palette location will be
        // shown on screen instead of the backdrop color. (Looking at the
        // relevant circuitry in Visual 2C02, this is an intentional feature of
        // the PPU and not merely a side effect of how rendering works.) This
        // can be used to display colors from the normally unused
        // $3F04/$3F08/$3F0C palette locations. A loop that fills the palette
        // will cause each color in turn to be shown on the screen, so to avoid
        // horizontal rainbow bar glitches while loading the palette, wait for a
        // real vertical blank first using an NMI technique."
        //
        let palette_addr = if self.shared_v_register >= 0x3f00 && self.shared_v_register <= 0x3fff {
            self.shared_v_register
        } else {
            0x3f00
        };
        let palette_value = self.palette_read(palette_addr);

        #[cfg(feature="ppu-hooks")]
        if self.debug.mux_hooks.hooks.len() > 0 {
            let state = MuxHookState {
                rendering_enabled: false,
                decision: if palette_addr != 0x3f00 { MuxDecision::PaletteHackBackground } else { MuxDecision::UniversalBackground },
                screen_x: screen_x as u8,
                screen_y: self.line as u8,

                shared_v_register: self.shared_v_register,
                fine_x_scroll: self.scroll_x_fine3,

                palette_value,
                emphasis: self.emphasis(),
                monochrome: self.monochrome,

                ..Default::default()
            };
            self.call_mux_hooks(cartridge, &state);
        }

        rgb_lut(palette_value)
    }

    fn render_pixel(&mut self, screen_x: usize, screen_y: usize, cartridge: &mut Cartridge) {

        let color = if self.rendering_enabled {
            self.compose_enabled_pixel(screen_x, screen_y, cartridge)
        } else {
            self.compose_disabled_pixel(screen_x, screen_y, cartridge)
        };

        /*
        if screen_x < 50 || screen_x > 200 {
            color = Color::from(0x13);
        }
        if screen_y < 50 || screen_y > 200 {
            color = Color::from(0x11);
        }
        if self.line == 232  {
            //color = Color::from(0x11);
            if screen_x == 0 {
                println!("line = {}, screen_tile_y = {}, screen_fine_y = {}", self.line, self.scroll_tile_y(), self.scroll_fine_y());
            }
        }
        */

        let fb = self.framebuffer.data.as_mut_ptr();
        let fb_off = self.framebuffer_offset;
        debug_assert!(fb_off >= 0 && fb_off < FRAMEBUFFER_STRIDE * FRAME_HEIGHT as isize);
        unsafe {
            *fb.offset(fb_off + 0) = color.r();
            *fb.offset(fb_off + 1) = color.g();
            *fb.offset(fb_off + 2) = color.b();
            *fb.offset(fb_off + 3) = 0xff;
        }

        self.framebuffer_offset += 4;

    }

    /// Treating VRAM as a 2x2 grid of nametables / screens this samples a single (background) pixel
    pub fn peek_vram_four_screens(&self, x: usize, y: usize, cartridge: &mut Cartridge) -> [u8; 3] {

        //let nametable_base_addr = self.name_table_base_addr();
        let pattern_table_addr = self.bg_pattern_table_addr();
        //println!("nt = {:x}, pt = {:x}", nametable_base_addr, pattern_table_addr);

        // Which nametable are we in
        let nametable_base_addr = if y < FRAME_HEIGHT {
            if x < FRAME_WIDTH { 0x2000 } else { 0x2400 }
        } else {
            if x < FRAME_WIDTH { 0x2800 } else { 0x2c00 }
        };

        let nametable_y = y as u16 % FRAME_HEIGHT as u16; //self.line + u16::from(self.current_scroll_y);
        let tile_y = nametable_y >> 3;
        let tile_pixel_y = nametable_y & 0x07;

        let nametable_x = x as u16 % FRAME_WIDTH as u16; // ((pixel_x as u16) + u16::from(self.current_scroll_x))
        let tile_x = nametable_x >> 3;
        let tile_pixel_x = nametable_x & 0x07;

        let attribute_base_addr = nametable_base_addr + ATTRIBUTE_TABLE_OFFSET;
        let attribute_x_offset = (tile_x >> 2) & 0x7;
        let attribute_y_offset = tile_y >> 2;
        let attribute_addr =
            attribute_base_addr + (attribute_y_offset << 3) + attribute_x_offset;

        let raw_attribute = cartridge.vram_peek(attribute_addr);
        let bg_palette_id = match (tile_x & 0x03 < 0x2, tile_y & 0x03 < 0x2) {
            (true, true) => (raw_attribute >> 0) & 0x03, // top left
            (false, true) => (raw_attribute >> 2) & 0x03, // top right
            (true, false) => (raw_attribute >> 4) & 0x03, // bottom left
            (false, false) => (raw_attribute >> 6) & 0x03, // bottom right
        };

        let nametable_addr = nametable_base_addr + tile_y * NAMETABLE_X_TILES_COUNT + tile_x;
        let bg_tile_id = u16::from(cartridge.ppu_bus_peek(nametable_addr));

        // pattern_table 1entry is 16 bytes, if it is the 0th line, use the 0th and 8th data
        let bg_pattern_table_base_addr = pattern_table_addr + (bg_tile_id << 4);
        let bg_pattern_table_addr_lower = bg_pattern_table_base_addr + tile_pixel_y;
        let bg_pattern_table_addr_upper = bg_pattern_table_addr_lower + 8;
        let bg_tile_pattern_lower = cartridge.ppu_bus_peek(bg_pattern_table_addr_lower);
        let bg_tile_pattern_upper = cartridge.ppu_bus_peek(bg_pattern_table_addr_upper);

        // Make the drawing color of bg
        let bg_pattern = (((bg_tile_pattern_upper >> (7 - tile_pixel_x)) & 0x01) << 1)
            | ((bg_tile_pattern_lower >> (7 - tile_pixel_x)) & 0x01);

        let palette_addr = if bg_pattern != 0 {
            0x3f00 +
                (u16::from(bg_palette_id) << 2) +
                u16::from(bg_pattern)
        } else {
            0x3f00 // universal background color for transparent pixels
        };

        let pattern_value = self.palette_read(palette_addr);
        let color = rgb_lut(pattern_value);

        [color.r(), color.g(), color.b()]
    }

    fn step_line(&mut self, cartridge: &mut Cartridge) {

        //println!("tick, dot = {}, line = {}", self.dot, self.line);

        /*
        if self.line == 0 && self.dot == 0 && self.line_status == LineStatus::Visible {
            println!("Start of new screen, line = 0, dot = 0");
            println!("scroll_tile_x = {}", self.scroll_tile_x());
            println!("scroll_tile_y = {}", self.scroll_tile_y());
            println!("scroll_fine_y = {}", self.scroll_fine_y());
            self.print_shift_register_state();
        } else if self.dot == 0 {
            println!("Start of new line = {}, dot = 0", self.line);
            self.print_shift_register_state();
        }
        */
        if let LineStatus::Visible | LineStatus::PreRender = self.line_status {
            if let 2..=257 | 322..=337 = self.dot {
                match self.dot % 8 {
                    1 => {
                        // "The shifters are reloaded during ticks 9, 17, 25, ..., 257."
                        self.reload_shift_registers();
                    }
                    _ => {}
                }
            }

            // PPU Reads one byte every two clocks
            if let 1..=256 | 321..=336 = self.dot {
                if self.rendering_enabled {
                    match self.dot % 8 {
                        0 => {
                            //self.draw_tile_span(cartridge, fb);
                            self.increment_coarse_x_scroll();
                            if self.dot == 256 {
                                self.increment_fine_y_scroll();
                            }
                        }
                        1 => {
                            self.read_nametable_byte(cartridge);
                        }
                        2 => {}
                        3 => {
                            self.read_attribute_table_byte(cartridge);
                        }
                        4 => {}
                        5 => {
                            self.read_pattern_table_low_byte(cartridge);
                        }
                        6 => {}
                        7 => {
                            self.read_pattern_table_high_byte(cartridge);
                        }
                        _ => unreachable!()
                    }
                }
            } else if let 257..=320 = self.dot {
                if self.rendering_enabled {
                    // "OAMADDR is set to 0 during each of ticks 257-320 (the sprite tile loading interval) of the pre-render and visible scanlines."
                    self.oam_offset = 0;

                    if self.dot == 257 {
                        //if self.line == 200 && self.oam_evaluate_n_sprites > 0 {
                        //    println!("line {}: evaluation found {} sprites", self.line, self.oam_evaluate_n_sprites);
                        //}

                        // "If rendering is enabled, the PPU copies all bits related to horizontal position from t to v"
                        // rendering enabled = "(i.e., when either background or sprite rendering is enabled in $2001:3-4)"
                        // ref: https://www.nesdev.org/wiki/File:Ntsc_timing.png
                        self.shared_v_register = (self.shared_v_register & (!VT_HORIZONTAL_SCROLL_BITS_MASK)) | (self.shared_t_register & VT_HORIZONTAL_SCROLL_BITS_MASK);
                        //println!("sync horizontal V bits: scroll_tile_x = {}, scroll_tile_y = {}", self.scroll_tile_x(), self.scroll_tile_y());
                    }

                    self.sprite_fetch_index = (self.dot as usize - 257) / 8;
                    match self.dot % 8 {
                        0 => {
                        }
                        1 => {
                            // These are a continuation of the nametable reads that happen between 1..=256 | 321..=336 that
                            // are redundant except that mappers may depend on observing them for synchronization
                            self.read_nametable_byte(cartridge);
                        }
                        2 => {}
                        3 => {
                            // These are a continuation of the attribute reads that happen between 1..=256 | 321..=336 that
                            // are redundant except that mappers may depend on observing them for synchronization
                            self.read_attribute_table_byte(cartridge);
                        }
                        4 => {}
                        5 => {
                            self.sprite_pattern_addr_lo = self.calculate_sprite_pattern_table_row_address();
                            self.fetch_sprite_pattern_low_byte(self.sprite_pattern_addr_lo, cartridge);
                        }
                        6 => {}
                        7 => {
                            self.fetch_sprite_pattern_high_byte(self.sprite_pattern_addr_lo + 8, cartridge);

                            // Although the PPU fetches sprite data during the pre-render line, we currently infer
                            // that it doesn't set up the the sprite outputs. As a minor optimization we can can also
                            // avoid composing any sprite output during line 239.
                            //
                            // Note: we currently rely on this optimization to leave the sprite_line_back buffer
                            // clear on lin 239, so that the next time the buffers are swapped at the start of the
                            // next frame then line zero will have a clear sprite_line_front buffer.
                            if self.line != 239 && self.line != 261 {
                                self.compose_sprite();
                            }
                        }
                        _ => unreachable!()
                    }
                }

            } else if let 337 | 339 = self.dot {
                if self.rendering_enabled {
                    // Dummy nametable reads that might be expected by mappers for synchronization (e.g. MMC5)
                    self.read_nametable_byte(cartridge);

                    // "With rendering enabled, each odd PPU frame is one PPU
                    // clock shorter than normal. This is done by skipping the
                    // first idle tick on the first visible scanline (by jumping
                    // directly from (339,261) on the pre-render scanline to
                    // (0,0) on the first visible scanline and doing the last
                    // cycle of the last dummy nametable fetch there instead"
                    //
                    // We skip from 339 to 340 so we don't need special case logic
                    // to progress the line counter (this is still after the
                    // nametable read)
                    if self.frame & 1 != 0 && self.line == 261 && self.dot == 339 {
                        self.dot = 340;
                    }
                }
            }
        }

        match self.line_status {

            LineStatus::Visible => {

                if self.rendering_enabled {
                    // Attempts to read OAM data via $2004 will return 0xff while secondary OAM is being cleared
                    if self.dot == 1 {
                        self.secondary_oam_being_cleared = true;
                        self.clear_secondary_oam();

                        // also clear the intermediate buffer we use for composing sprite pixels while fetching
                        // sprite data later
                        //println!("Swapping sprite line buffers");
                        std::mem::swap(&mut self.sprite_line_front, &mut self.sprite_line_back);
                        //println!("Clearing sprite line back buffer");
                        self.sprite_line_back = Default::default();
                    } else if self.dot == 64 {
                        self.secondary_oam_being_cleared = false;
                        self.start_sprite_evaluation();
                    } else if let 65..=256 = self.dot {
                        // "Cycles 65-256: Sprite evaluation"

                        // "Sprite evaluation occurs if either the sprite layer or
                        // background layer is enabled via $2001. Unless both layers
                        // are disabled, it merely hides sprite rendering"
                        if self.rendering_enabled {
                            self.step_sprite_evaluation();
                        }
                    }
                }

                if let 1..=256 = self.dot {
                    let screen_x = self.dot as usize - 1;
                    let screen_y = self.line as usize;
                    self.render_pixel(screen_x, screen_y, cartridge);
                }
                /*
                if self.dot == 340 {
                    //println!("draw line = {}", self.line);
                    self.draw_tile_span(cartridge, fb);
                }*/
            }
            LineStatus::PostRender => { // TODO: remove redundant enum value
                if self.dot == 340 {
                    //println!("PPU: Finished Frame");
                    self.frame_ready = true;
                }
            }
            LineStatus::VerticalBlanking => {
                if self.line == 241 && self.dot == 1 {
                    //println!("IN VBLANK {}", self.clock);
                    self.status.set(StatusFlags::IN_VBLANK, true);
                    //println!("Set IN_VBLANK flag");
                    self.update_nmi();
                }
            }
            LineStatus::PreRender => {
                if self.dot == 0 {
                    // "It is also the case that if OAMADDR is not less than eight when rendering starts,
                    // the eight bytes starting at OAMADDR & 0xF8 are copied to the first eight bytes of OAM"
                    if self.rendering_enabled {
                        if self.oam_offset >= 8 {
                            let off = (self.oam_offset & 0xf8) as usize;
                            for i in 0..8 {
                                self.write_oam_data(i, self.read_oam_data((off as u8) + i));
                            }
                        }
                    }
                } else if self.dot == 1 {
                    //println!("OUT OF VBLANK {}", self.clock);
                    self.status.set(StatusFlags::IN_VBLANK, false);
                    //println!("Clear IN_VBLANK flag");
                    self.update_nmi();

                    self.status.set(StatusFlags::SPRITE0_HIT, false);
                    //println!("Clearing SPRITE_OVERFLOW in pre-render line, dot = 1");
                    self.status.set(StatusFlags::SPRITE_OVERFLOW, false);
                    self.framebuffer_offset = 0;
                }

                //if self.dot % 8 == 0 {
                //    println!("Pre-render dot = {}", self.dot);
                //}
                // nesdev:
                //
                // "During dots 280 to 304 of the pre-render scanline (end of vblank)"
                //
                // "If rendering is enabled, at the end of vblank, shortly after
                // the horizontal bits are copied from t to v at dot 257, the
                // PPU will repeatedly copy the vertical bits from t to v from
                // dots 280 to 304, completing the full initialization of v
                // from t":
                if let 280..=304 = self.dot {
                    if self.rendering_enabled {
                        self.shared_v_register = (self.shared_v_register & (!VT_VERTICAL_SCROLL_BITS_MASK)) | (self.shared_t_register & VT_VERTICAL_SCROLL_BITS_MASK);
                        //println!("sync vertical V bits: scroll_tile_x = {}, scroll_tile_y = {}, scroll_fine_y = {}", self.scroll_tile_x(), self.scroll_tile_y(), self.scroll_fine_y());
                        //println!("sync vertical V bits: new PPU ADDR = 0x{:04x}, control2 = {:?}", self.shared_v_register, self.control2);
                        //self.update_scroll_xy();
                    } else {
                        //println!("Skipping sync of vertical V bits (rendering disabled)");
                    }
                }
            }
        }

        if let LineStatus::Visible | LineStatus::PreRender = self.line_status {
            if let 2..=257 | 322..=337 = self.dot {
                self.shift_registers();
            }
        }

        #[cfg(feature="ppu-hooks")]
        {
            if self.debug.dot_hooks[self.line as usize][self.dot as usize].hooks.len() != 0 {
                let mut hooks = std::mem::take(&mut self.debug.dot_hooks[self.line as usize][self.dot as usize]);
                for hook in hooks.hooks.iter_mut() {
                    (hook.func)(self, cartridge);
                }
                std::mem::swap(&mut self.debug.dot_hooks[self.line as usize][self.dot as usize], &mut hooks);
            }
        }
    }

    pub fn step(&mut self, cartridge: &mut Cartridge) -> bool {
        //println!("PPU Step dot");

        //println!("ppu clock = {ppu_clock}");

        #[cfg(feature="debugger")]
        {
            if self.debug.breakpoints.len() > 0 {
                let mut tmp = std::mem::take(&mut self.debug.breakpoints);
                let mut remove = vec![];
                for bp in tmp.iter_mut() {

                    if let Some(frame) = bp.frame {
                        if self.frame != frame {
                            continue;
                        }
                    }
                    //println!("PPU breakpoint frame {:?} matches", self.frame);
                    if let Some(line) = bp.line {
                        if self.line != line {
                            continue
                        }
                    }
                    //println!("PPU breakpoint line {:?} matches", self.line);
                    if self.dot == bp.dot {
                        //println!("PPU breakpoint dot matches frame = {}, line = {}, dot = {}", self.frame, self.line, self.dot);
                        self.debug.breakpoint_hit = true;
                        if (bp.callback)(self, self.frame, self.line, self.dot) == DotBreakpointCallbackAction::Remove {
                            remove.push(bp.handle);
                        }
                    }
                }
                std::mem::swap(&mut tmp, &mut self.debug.breakpoints);
                for h in remove {
                    self.remove_dot_breakpoint(h);
                }
            }
            // Note we also check for breakpoint_hit even if there are no breakpoints
            // since the breakpoint may get immediately removed by its callback but it
            // may take longer for .breakpoint_hit flag to be cleared while the current
            // CPU instruction continues to run
            if self.debug.breakpoint_hit {
                return false;
            }
        }

        self.step_line(cartridge);

        self.clock += 1;

        // Note: this used to do (self.clock % 341) but that doesn't allow for skipping
        // the a dot on odd frames.
        self.dot = (self.dot + 1) % 341;

        if self.dot == 0 {
            self.line = (self.line + 1) % N_LINES;
            self.line_status = LineStatus::from(self.line);
            //println!("Next line = {}: {:?}", self.line, self.line_status);

            if self.line == 0 {
                self.frame += 1;
            }
        }

        self.decay_io_latch();

        return true;
    }

    #[inline(always)]
    fn decay_io_latch(&mut self) {
        if self.clock - self.io_latch_last_update_clock >= (self.io_latch_decay_clock_period as u64) {
            let keep = self.io_latch_keep_alive_masks[0] | self.io_latch_keep_alive_masks[1];
            self.io_latch_value &= keep;
            self.io_latch_keep_alive_masks[1] = self.io_latch_keep_alive_masks[0];
            self.io_latch_keep_alive_masks[0] = 0;
            self.io_latch_last_update_clock = self.clock;
        }
    }

    #[cfg(feature="ppu-hooks")]
    pub fn add_dot_hook(&mut self, line: usize, dot: usize, func: Box<FnDotHook>) -> HookHandle {
        self.debug.dot_hooks[line][dot].add_hook(func)
    }

    #[cfg(feature="ppu-hooks")]
    pub fn remove_dot_hook(&mut self, line: usize, dot: usize, handle: HookHandle) {
        self.debug.dot_hooks[line][dot].remove_hook(handle);
    }

    /// Add a hook function into the background priority MUX operation
    ///
    /// Debuggers can use this to trace key rendering state at the heart of the PPU rendering
    /// emulation
    #[cfg(feature="ppu-hooks")]
    pub fn add_mux_hook(&mut self, func: Box<FnMuxHook>) -> HookHandle {
        self.debug.mux_hooks.add_hook(func)
    }

    /// Remove a hook function, with a given `key` from the background priority MUX operation
    #[cfg(feature="ppu-hooks")]
    pub fn remove_mux_hook(&mut self, handle: HookHandle) {
        self.debug.mux_hooks.remove_hook(handle);
    }

    /// Request that the emulator should stop once it reaches the given frame, line and dot
    #[cfg(feature="debugger")]
    pub fn add_dot_breakpoint(&mut self, frame: Option<u32>, line: Option<u16>, dot: u16, callback: Box<FnDotBreakpointCallback>) -> DotBreakpointHandle {
        let handle = DotBreakpointHandle(self.debug.next_breakpoint_handle);
        self.debug.next_breakpoint_handle += 1;

        self.debug.breakpoints.push(DotBreakpoint {
            handle,
            frame,
            line,
            dot,
            callback
        });

        handle
    }

    #[cfg(feature="debugger")]
    pub fn remove_dot_breakpoint(&mut self, handle: DotBreakpointHandle) {
        if let Some(i) = self.debug.breakpoints.iter().position(|b| b.handle == handle) {
            self.debug.breakpoints.swap_remove(i);
        }
    }


}

/*
#[test]
fn ppu_scroll_x_increment() {
    let mut ppu = Ppu::default();

    // increment_coarse_x_scroll should effectively increment to the next nametable tile
    // There are 32 horizontal tiles per screen.
    // There are logically two adjacent screens/nametables horizontally = 64 tiles
    // The effective scroll should wrap around after two screens
    // Scroll increments should update the shared V register, not the T register
    for i in 0..128 {
        let expected = (i * 8) % (VISIBLE_SCREEN_WIDTH as u16 * 2);
        println!("scroll_x {i} = {}, expect = {}", ppu.scroll_x(), expected);
        //assert_eq!(ppu.scroll_x(), expected);
        ppu.update_scroll_xy();
        assert_eq!(ppu.current_scroll_x, expected);
        let saved_t = ppu.shared_t_register;
        let saved_v = ppu.shared_v_register;
        ppu.increment_coarse_x_scroll();
        assert_ne!(saved_v, ppu.shared_v_register);
        assert_eq!(saved_t, ppu.shared_t_register);
    }
}

#[test]
fn ppu_scroll_y_increment() {
    let mut ppu = Ppu::default();

    // increment_fine_y_scroll should effectively increment by a single pixel
    // There are 240 vertical pixels per screen.
    // There are logically two adjacent screens/nametables vertically = 480 pixels
    // The effective scroll should wrap around after two screens
    // Scroll increments should update the shared V register, not the T register
    for i in 0..(VISIBLE_SCREEN_HEIGHT as u16 * 3) {
        let expected = i % (VISIBLE_SCREEN_HEIGHT as u16 * 2);
        println!("scroll_y {i} = {}, expect = {}", ppu.scroll_y(), expected);
        assert_eq!(ppu.scroll_y(), expected);
        //ppu.update_scroll_xy();
        assert_eq!(ppu.current_scroll_y, expected);
        let saved_t = ppu.shared_t_register;
        let saved_v = ppu.shared_v_register;
        ppu.increment_fine_y_scroll();
        assert_ne!(saved_v, ppu.shared_v_register);
        assert_eq!(saved_t, ppu.shared_t_register);
    }
}
*/