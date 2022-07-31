#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(unused)]

use std::ops::Index;

use anyhow::anyhow;
use anyhow::Result;

use crate::binary::NesBinaryConfig;
use crate::cartridge::{self, Cartridge};
use crate::constants::{PAGE_SIZE_16K, PAGE_SIZE_8K};
use crate::framebuffer::{Framebuffer, FramebufferDataRental, PixelFormat};
use crate::system::Model;

mod ffi {
    include!(concat!(env!("OUT_DIR"), "/bindings.rs"));
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Revision {
    RP2C02G = ffi::PPUSim_Revision_RP2C02G as isize,
    RP2C02H = ffi::PPUSim_Revision_RP2C02H as isize,
    RP2C03B = ffi::PPUSim_Revision_RP2C03B as isize,
    RP2C03C = ffi::PPUSim_Revision_RP2C03C as isize,
    RC2C03B = ffi::PPUSim_Revision_RC2C03B as isize,
    RC2C03C = ffi::PPUSim_Revision_RC2C03C as isize,
    RP2C04_0001 = ffi::PPUSim_Revision_RP2C04_0001 as isize,
    RP2C04_0002 = ffi::PPUSim_Revision_RP2C04_0002 as isize,
    RP2C04_0003 = ffi::PPUSim_Revision_RP2C04_0003 as isize,
    RP2C04_0004 = ffi::PPUSim_Revision_RP2C04_0004 as isize,
    RC2C05_01 = ffi::PPUSim_Revision_RC2C05_01 as isize,
    RC2C05_02 = ffi::PPUSim_Revision_RC2C05_02 as isize,
    RC2C05_03 = ffi::PPUSim_Revision_RC2C05_03 as isize,
    RC2C05_04 = ffi::PPUSim_Revision_RC2C05_04 as isize,
    RC2C05_99 = ffi::PPUSim_Revision_RC2C05_99 as isize,
    RP2C07_0 = ffi::PPUSim_Revision_RP2C07_0 as isize,
    UMC_UA6538 = ffi::PPUSim_Revision_UMC_UA6538 as isize,
}

struct AddressLatch {
    value: u8,
}

impl AddressLatch {
    pub fn step(&mut self, enable: TriState, data: u8) -> u8 {
        if enable == TriState::One {
            self.value = data;
        }
        self.value
    }
}

/// Zero = 0,
/// One = 1,
/// Z = (uint8_t)-1,
/// X = (uint8_t)-2,
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum TriState {
    Zero,
    One,
    Z,
    X
}
const TRISTATE_Z_VALUE: u8 = (-1 as i8) as u8;
const TRISTATE_X_VALUE: u8 = (-2 as i8) as u8;

impl From<u8> for TriState {
    fn from(num: u8) -> Self {
        match num {
            0 => TriState::Zero,
            1 => TriState::One,
            TRISTATE_Z_VALUE => TriState::Z,
            TRISTATE_X_VALUE => TriState::X,
            _ => panic!("Spurious TriState value of {num}")
        }
    }
}
impl Into<u8> for TriState {
    fn into(self) -> u8 {
        match self {
            TriState::Zero => 0,
            TriState::One => 1,
            TriState::Z => TRISTATE_Z_VALUE,
            TriState::X => TRISTATE_X_VALUE,
        }
    }
}

enum InputPad {
    RnW = ffi::PPUSim_InputPad_RnW as isize,
    RS0 = ffi::PPUSim_InputPad_RS0 as isize,
    RS1 = ffi::PPUSim_InputPad_RS1 as isize,
    RS2 = ffi::PPUSim_InputPad_RS2 as isize,
    n_DBE = ffi::PPUSim_InputPad_n_DBE as isize,
    CLK = ffi::PPUSim_InputPad_CLK as isize,
    n_RES = ffi::PPUSim_InputPad_n_RES as isize,
}

enum OutputPad
{
    n_INT = ffi::PPUSim_OutputPad_n_INT as isize,

    /// Address latch enable. When high then low address bits should be
    /// store in a latch register which is output on the address bus
    ALE = ffi::PPUSim_OutputPad_ALE as isize,

    n_RD = ffi::PPUSim_OutputPad_n_RD as isize,
    n_WR = ffi::PPUSim_OutputPad_n_WR as isize,
}


pub struct PpuSim {
    revision: Revision,

    pub framebuffer: FramebufferDataRental,
    frame_ready: bool,

    clk: TriState,

    /// Bits 0..8 of bus address, which go via address_latch
    address_bus_lo: u8,

    /// Bits 0..14 of bus address
    address_bus_hi: u8,

    /// Latch register state for bits 0..8 of bus address
    address_latch: AddressLatch,
    /// Output bits 0..8 of latched bus address
    address_latch_output: u8,

    //data_bus_enable: bool,

    /// The number of half CLKs to enable the data bus for a read/write
    data_bus_enable_duration: usize,
    data_bus_address: u16,
    data_bus_write: bool,
    data_bus_write_value: u8,
    data_bus: u8,

    pending_reset: bool,
    reset_half_clock_count: usize,

    last_frame_pclk: u64,

    prev_h_cnt: usize,
    prev_v_cnt: usize,

    // PPU bus reads/writes via the emulator are latched so as to only
    // make a single `cartridge.` API call for each read/write by
    // the PPU. The latch updates if the address, n_RE or n_WE state
    // changes

    ppu_bus_latch_data: u8,
    ppu_bus_latch_pclk: u64,
    ppu_bus_latch_address: u16,
    ppu_bus_latch_read_neg: TriState,
    ppu_bus_latch_write_neg: TriState,

    //address: [TriState; 14],

    ppu: *mut ffi::PPUSim_PPU,

    inputs: [u8; ffi::PPUSim_InputPad_Max as usize],
    outputs: [u8; ffi::PPUSim_OutputPad_Max as usize],

    pub nmi_interrupt_raised: bool
}


impl PpuSim {

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

    pub fn new(nes_model: Model) -> Self {

        debug_assert_eq!(ffi::PPUSim_Revision_Max, 18);
        debug_assert_eq!(ffi::PPUSim_InputPad_Max, 7);
        debug_assert_eq!(ffi::PPUSim_OutputPad_Max, 4);

        let revision = match nes_model {
            Model::Ntsc => Revision::RP2C02G,
            Model::Pal => Revision::RP2C07_0,
        };

        let framebuffer = Framebuffer::new(256, 240, PixelFormat::RGBA8888);
        let framebuffer = framebuffer.rent_data().unwrap();

        let ppu = unsafe { ffi::ppu_sim_new(revision as i32) };
        let inputs =  [0u8; ffi::PPUSim_InputPad_Max as usize];
        let outputs =  [0u8; ffi::PPUSim_OutputPad_Max as usize];

        let mut sim = Self {

            framebuffer,
            frame_ready: false,

            clk: TriState::Zero,
            address_bus_lo: 0,
            address_bus_hi: 0,
            address_latch: AddressLatch { value: 0 },
            address_latch_output: 0,

            //data_bus_enable: false,
            data_bus_enable_duration: 0,
            data_bus_address: 0,
            data_bus_write: false,
            data_bus_write_value: 0,
            data_bus: 0,
            //address: [TriState::Zero; 14],

            pending_reset: false,
            reset_half_clock_count: 0,

            last_frame_pclk: 0,

            prev_h_cnt: 0,
            prev_v_cnt: 0,

            ppu_bus_latch_data: 0,
            ppu_bus_latch_pclk: 0,
            ppu_bus_latch_address: 0,
            ppu_bus_latch_read_neg: TriState::X,
            ppu_bus_latch_write_neg: TriState::X,

            revision,
            ppu,
            inputs,
            outputs,

            nmi_interrupt_raised: false,
        };

        sim.set_raw_output(true);

        sim
    }

    /// Returns the number of CLK cycles per PCLK (there doesn't seem to be a utility for
    /// this in PPUSim itself
    pub fn clk_per_pclk(&self) -> usize {
        match self.revision {
            Revision::RP2C07_0 | Revision::UMC_UA6538 => 5,
            _ => 4
        }
    }

    pub fn system_bus_write_start(&mut self, addr: u16, value: u8) {
        debug_assert_eq!(self.data_bus_enable_duration, 0);
        self.data_bus_enable_duration = self.clk_per_pclk() * 2;
        //self.data_bus_enable = true;
        self.data_bus_address = addr;
        self.data_bus_write = true;
        self.data_bus_write_value = value;
    }

    //pub fn cpu_write_stop(&mut self) {
    //    debug_assert_eq!(self.data_bus_enable, true);
    //    self.data_bus_enable = false;
    //}

    pub fn system_bus_read_start(&mut self, addr: u16) {
        debug_assert_eq!(self.data_bus_enable_duration, 0);
        self.data_bus_enable_duration = self.clk_per_pclk() * 2;
        //self.data_bus_enable = true;
        self.data_bus_address = addr;
        self.data_bus_write = false;
    }

    //pub fn cpu_read_stop(&mut self) {
    //    debug_assert_eq!(self.data_bus_enable, true);
    //    self.data_bus_enable = false;
    //}

    pub fn debug_set_force_render_enabled(&mut self, enabled: bool) {
        unsafe { ffi::PPUSim_PPU_Dbg_RenderAlwaysEnabled(self.ppu, enabled) };
    }

    pub fn set_raw_output(&mut self, raw_output: bool) {
        unsafe { ffi::PPUSim_PPU_SetRAWOutput(self.ppu, raw_output) };
    }

    pub fn debug_read_registers(&self) -> ffi::PPUSim_PPU_Registers {
        let mut regs = ffi::PPUSim_PPU_Registers::default();
        unsafe { ffi::PPUSim_PPU_GetDebugInfo_Regs(self.ppu, &mut regs) };
        regs
    }

    pub fn debug_read_wires(&self) -> ffi::PPUSim_PPU_Interconnects {
        let mut wires = ffi::PPUSim_PPU_Interconnects::default();
        unsafe { ffi::PPUSim_PPU_GetDebugInfo_Wires(self.ppu, &mut wires) };
        wires
    }

    pub fn debug_set_control_register(&mut self, value: u8) {
        unsafe { ffi::PPUSim_PPU_Dbg_SetCTRL0(self.ppu, value) };
    }

    pub fn debug_set_mask_register(&mut self, value: u8) {
        unsafe { ffi::PPUSim_PPU_Dbg_SetCTRL1(self.ppu, value) };
    }

    pub fn pclk(&self) -> u64 {
        unsafe { ffi::PPUSim_PPU_GetPCLKCounter(self.ppu) }
    }
    pub fn reset_pclk(&self) {
        unsafe { ffi::PPUSim_PPU_ResetPCLKCounter(self.ppu) }
    }

    pub fn h_counter(&self) -> u64 {
        unsafe { ffi::PPUSim_PPU_GetHCounter(self.ppu) }
    }
    pub fn v_counter(&self) -> u64 {
        unsafe { ffi::PPUSim_PPU_GetVCounter(self.ppu) }
    }

    /// Sets the reset pin for four half clock cycles (to ensure the PPU resets all internal circuits)
    pub fn reset(&mut self) {
        self.reset_half_clock_count = 4;
    }

    fn sim_ppu_bus_io(&mut self, address: u16, read_enable_neg: TriState, write_enable_neg: TriState, cartridge: &mut Cartridge) {

        if read_enable_neg == TriState::One && write_enable_neg == TriState::One {
            return;
        }

        // XXX: maybe also compare ppu_bus_latch_data to self.address_bus_lo
        // XXX: maybe also compare ppu_bus_latch_pclk to self.pclk()
        if self.ppu_bus_latch_address == address &&
            self.ppu_bus_latch_read_neg == read_enable_neg &&
            self.ppu_bus_latch_write_neg == write_enable_neg
        {
            return;
        }

        // For SRAM in particular reads / writes are driven according to this logic table:
        //       | n_RE | n_WE
        // ------|------|------
        //  Read | Zero | One
        // ------|------|------
        // Write |  X   | Zero
        //
        // Ref: https://github.com/emu-russia/breaks/blob/master/Docs/Famicom/HM6116_SRAM.pdf
        // Ref: https://console5.com/techwiki/images/b/b7/LC3517B.pdf
        //
        // As a generalization though, considering we're calling into an emulator where we've
        // abstracted away low-level details, we use the same logic for cartridge/mapper
        // I/O too.
        //
        if write_enable_neg == TriState::Zero {
           let data = self.address_bus_lo;
           //println!("PPU SIM: writing {data} to {address:04x}");
           match address {
               0x0000..=0x1fff => cartridge.ppu_bus_write(address, data),
               0x2000..=0x3fff => cartridge.vram_write(address, data),
               _ => panic!("out-of-bounds PPU address {address:04x}")
           }
        } else if read_enable_neg == TriState::Zero && write_enable_neg == TriState::One {
           let data = match address {
               0x0000..=0x1fff => cartridge.ppu_bus_read(address),
               0x2000..=0x3fff => cartridge.vram_read(address),
               _ => panic!("out-of-bounds PPU address {address:04x}")
           };
           //println!("PPU SIM: read {data} from {address:04x}");
           self.address_bus_lo = data;
        }

        self.ppu_bus_latch_address = address;
        self.ppu_bus_latch_read_neg = read_enable_neg;
        self.ppu_bus_latch_write_neg = write_enable_neg;
        //self.ppu_bus_latch_data = self.address_bus_lo;
        //self.ppu_bus_latch_pclk = self.pclk();
    }

    pub fn step_half(&mut self, cartridge: &mut Cartridge) {
        let mut _ext: u8 = 0;

        self.inputs[InputPad::CLK as usize] = self.clk.into();
        self.inputs[InputPad::n_RES as usize] = if self.reset_half_clock_count > 0 { TriState::Zero } else { TriState::One }.into();
        self.inputs[InputPad::RnW as usize] = if self.data_bus_write { TriState::Zero } else { TriState::One }.into();
        self.inputs[InputPad::RS0 as usize] = if self.data_bus_address & 1 != 0 { TriState::One } else { TriState::Zero }.into();
        self.inputs[InputPad::RS1 as usize] = if self.data_bus_address & 2 != 0 { TriState::One } else { TriState::Zero }.into();
        self.inputs[InputPad::RS2 as usize] = if self.data_bus_address & 4 != 0 { TriState::One } else { TriState::Zero }.into();
        self.inputs[InputPad::n_DBE as usize] = if self.data_bus_enable_duration > 0 { TriState::Zero } else { TriState::One }.into();

        if self.data_bus_enable_duration > 0 {
            if self.data_bus_write {
                self.data_bus = self.data_bus_write_value;
            }
            self.data_bus_enable_duration -= 1;
        }

        let h_cnt = self.h_counter() as usize;
        let v_cnt = self.v_counter() as usize;
        unsafe {
            let mut vout_raw: ffi::PPUSim_VideoOutSignal = std::mem::zeroed();;
            ffi::PPUSim_PPU_sim(self.ppu, self.inputs.as_mut_ptr(), self.outputs.as_mut_ptr(), &mut _ext, &mut self.data_bus, &mut self.address_bus_lo, &mut self.address_bus_hi, &mut vout_raw);
            if v_cnt < 240 && h_cnt < 256 {
                let mut vout_rgb: ffi::PPUSim_VideoOutSignal = std::mem::zeroed();
                ffi::PPUSim_PPU_ConvertRAWToRGB(self.ppu, &mut vout_raw, &mut vout_rgb);
                const FRAMEBUFFER_BPP: usize = 4;
                const FRAMEBUFFER_STRIDE: usize = 256 * FRAMEBUFFER_BPP;

                let fb = self.framebuffer.data.as_mut_ptr();
                let fb_off = v_cnt * FRAMEBUFFER_STRIDE + (h_cnt * FRAMEBUFFER_BPP);
                debug_assert!(fb_off < FRAMEBUFFER_STRIDE * 240 as usize);
                let fb_off = fb_off as isize;
                unsafe {
                    *fb.offset(fb_off + 0) = vout_rgb.RGB.RED;
                    *fb.offset(fb_off + 1) = vout_rgb.RGB.GREEN;
                    *fb.offset(fb_off + 2) = vout_rgb.RGB.BLUE;
                    *fb.offset(fb_off + 3) = 0xff;
                }
            }
        }

        if self.reset_half_clock_count > 0 {
            self.reset_half_clock_count -= 1;
        }

        let address_latch_enable: TriState = self.outputs[OutputPad::ALE as usize].into();
        let read_neg: TriState = self.outputs[OutputPad::n_RD as usize].into();
        let write_neg: TriState = self.outputs[OutputPad::n_WR as usize].into();
        let mut interrupt_neg: TriState = self.outputs[OutputPad::n_INT as usize].into();

        if interrupt_neg == TriState::Z {
            interrupt_neg = TriState::One;
        }
        self.nmi_interrupt_raised = interrupt_neg == TriState::Zero;

        self.address_latch_output = self.address_latch.step(address_latch_enable, self.address_bus_lo);

        let address = self.address_latch_output as u16 | (((self.address_bus_hi as u16) & 0b11_1111) << 8);

        self.sim_ppu_bus_io(address, read_neg, write_neg, cartridge);

        let wires = self.debug_read_wires();
        //println!("clk = {}, /clk = {}, pclk = {} /pclk = {}, ale = {:?}, /rd = {:?}, /wr = {:?}, /int = {:?}, pclk = {}",
        //         wires.CLK, wires.n_CLK, wires.PCLK, wires.n_PCLK, address_latch_enable, read_neg, write_neg, interrupt_neg, self.pclk());

        self.clk = if self.clk == TriState::Zero { TriState::One } else { TriState::Zero };

        let pclk = self.pclk();
        if h_cnt == 0 && v_cnt == 241 && self.last_frame_pclk != pclk && self.clk == TriState::Zero {
            self.last_frame_pclk = pclk;
            println!("PPU SIM: Finished frame: regs: {:?}", self.debug_read_registers());
            self.frame_ready = true;
        }

    }
}

impl Drop for PpuSim {
    fn drop(&mut self) {
        unsafe {
            ffi::ppu_sim_drop(self.ppu);
        }
    }
}

/*
/// Writes a value to the PPU's data bus and progresses the simulation for 1 PCLK
/// Use this to write registers from tests as if from the CPU
fn ppu_sim_test_cpu_write(ppu: &mut PpuSim, cartridge: &mut Cartridge, address: u16, value: u8) {
    ppu.cpu_write_start(address, value);

    // Even though read/write ops _should_ simulate in half a CLK cycle, the real HW would assert
    // cpu operations for longer so to be slightly more realistic we at least hold the operation
    // for a single PCLK

    let pclk_len = ppu.clk_per_pclk() * 2;
    for i in 0..pclk_len {
        ppu.step_half(cartridge);
    }

    ppu.cpu_write_stop();
}

/// Reads a value from the PPU's data bus and progresses the simulation for 1 PCLK
/// Use this to read registers from tests as if read from the CPU
fn ppu_sim_test_cpu_read(ppu: &mut PpuSim, cartridge: &mut Cartridge, address: u16) -> u8 {
    ppu.cpu_read_start(address);

    // Even though read/write ops _should_ simulate in half a CLK cycle, the real HW would assert
    // cpu operations for longer so to be slightly more realistic we at least hold the operation
    // for a single PCLK

    let pclk_len = ppu.clk_per_pclk() * 2;
    for i in 0..pclk_len {
        ppu.step_half(cartridge);
    }

    ppu.cpu_read_stop();

    ppu.data_bus
}*/

#[test]
fn ppu_sim_step() {

    let mut fb = Framebuffer::new(256, 240, PixelFormat::RGBA8888);
    let mut fb_data = fb.rent_data().unwrap();
    let fb_ptr = fb_data.data.as_mut_ptr();

    let mut ppu = PpuSim::new(Model::Ntsc);
    ppu.reset();
    ppu.debug_set_force_render_enabled(true);

    let prg_rom = vec![0u8; PAGE_SIZE_16K];
    let chr_ram = vec![0u8; PAGE_SIZE_8K];
    let mut cartridge = Cartridge {
        config: NesBinaryConfig::None,
        mapper: Box::new(crate::mappers::Mapper0::new_full(prg_rom, chr_ram, true, 1, cartridge::NameTableMirror::Vertical))
    };

    // nesdev:
    // "Writes to the following registers are ignored if earlier than ~29658 CPU clocks after reset: PPUCTRL, PPUMASK, PPUSCROLL, PPUADDR.
    // This also means that the PPUSCROLL/PPUADDR latch will not toggle."

    const DOTS_PER_FRAME: usize = 341 * 262;
    let clk_per_pclk: usize = ppu.clk_per_pclk();

    for i in 0..2 {
        println!("Warm-up Frame {i}");
        for line in 0..261 {
            //println!("line = {line}");
            for dot in 0..341 {
                for i in 0..(clk_per_pclk * 2) {
                    ppu.step_half(&mut cartridge);
                }
            }
        }
    }
    let regs = ppu.debug_read_registers();
    println!("PPU regs = {:?}", regs);

    //ppu.debug_set_control_register(0);
    ppu.debug_set_mask_register(0b0001_1110); // show bg + sprites + left col

    for i in 0..2 {
        for line in 0..261 {
            //println!("line = {line}");
            for dot in 0..341 {
                if dot == line {
                    println!("Frame {i}, line = {line}, dot = {dot} regs = {:?}", ppu.debug_read_registers());
                }
                for i in 0..(clk_per_pclk * 2) {
                    ppu.step_half(&mut cartridge);
                }
            }
        }
    }

}