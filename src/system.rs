use crate::apu::Apu;
use crate::cartridge;
use crate::cpu::Interrupt;
use crate::ppu::OAM_SIZE;
use crate::ppu::Ppu;
use crate::ppu::PpuStatus;
use crate::ppu_registers::APU_IO_OAM_DMA_OFFSET;

use super::cartridge::*;
use super::interface::*;
use super::pad::*;
use super::vram::*;

pub const WRAM_SIZE: usize = 0x0800;
pub const PPU_REG_SIZE: usize = 0x0008;
pub const APU_IO_REG_SIZE: usize = 0x0018;
pub const EROM_SIZE: usize = 0x1FE0;
pub const ERAM_SIZE: usize = 0x2000;
pub const PROM_SIZE: usize = 0x8000; // 32KB

pub const WRAM_BASE_ADDR: u16 = 0x0000;
pub const PPU_REG_BASE_ADDR: u16 = 0x2000;
pub const APU_IO_REG_BASE_ADDR: u16 = 0x4000;
pub const CARTRIDGE_BASE_ADDR: u16 = 0x4020;

/// Memory Access Dispatcher
//#[derive(Clone)]

pub struct System {

    pub cpu_clock: u64,

    pub ppu_clock: u64,
    pub ppu: Ppu,

    /// 0x0000 - 0x07ff: WRAM
    /// 0x0800 - 0x1f7ff: WRAM  Mirror x3
    pub wram: [u8; WRAM_SIZE],

    //  0x4000 - 0x401f: APU I/O, PAD
    pub io_reg: [u8; APU_IO_REG_SIZE],

    // If the CPU starts an OAM DMA then the CPU will be suspended for 513 or 514 clock cycles
    pub oam_dma_cpu_suspend_cycles: u16,

    /// The R / W request to the cassette is Emulation at the call destination,
    /// and the addr passed to the argument that switches the actual machine
    /// passes the address as it is from the CPU instruction
    ///  0x4020 - 0x5fff: Extended ROM
    ///  0x6000 - 0x7FFF: Extended RAM
    ///  0x8000 - 0xbfff: PRG-ROM switchable
    ///  0xc000 - 0xffff: PRG-ROM fixed to the last bank or switchable
    pub cartridge: Cartridge,

    /// コントローラへのアクセスは以下のモジュールにやらせる
    /// 0x4016, 0x4017
    pub pad1: Pad,
    pub pad2: Pad,
}

impl SystemBus for System {
    fn read_u8(&mut self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x1fff => { // RAM
                //println!("system read {addr:x}");
                // mirror support
                let index = usize::from(addr) % self.wram.len();
                arr_read!(self.wram, index)
            }
            0x2000..=0x3fff => { // PPU I/O
                self.ppu.read_u8(&mut self.cartridge, addr)
            }
            0x4000..=0x401f => {  // APU I/O
                let index = usize::from(addr - APU_IO_REG_BASE_ADDR);
                match index {
                    // TODO: APU
                    0x16 => self.pad1.read_out(), // pad1
                    0x17 => self.pad2.read_out(), // pad2
                    _ => arr_read!(self.io_reg, index),
                }
            }
            _ => { // Cartridge
                self.cartridge.read_u8(addr)
            }
        }

    }

    fn write_u8(&mut self, addr: u16, data: u8) {
        match addr {
            0x0000..=0x1fff => { // RAM
                // mirror support
                let index = usize::from(addr) % self.wram.len();
                arr_write!(self.wram, index, data);
            }
            0x2000..=0x3fff => { // PPU I/O
                self.ppu.write_u8(&mut self.cartridge, addr, data);
            }
            0x4000..=0x401f => {  // APU I/O
                let index = usize::from(addr - 0x4000);
                match index {
                    // TODO: APU
                    0x14 => {
                        //println!("start OAM DMA");

                        self.oam_dma_cpu_suspend_cycles = 513;
                        if self.cpu_clock % 2 == 1 {
                            self.oam_dma_cpu_suspend_cycles += 1;
                        }
                        self.run_dma((data as u16) << 8);
                    }
                    0x16 => self.pad1.write_strobe((data & 0x01) == 0x01), // pad1
                    0x17 => self.pad2.write_strobe((data & 0x01) == 0x01), // pad2
                    _ => {}
                }
                arr_write!(self.io_reg, index, data);
            }
            _ => { // Cartridge
                self.cartridge.write_u8(addr, data);
            }
        }
    }
}

impl System {

    pub fn new(ppu: Ppu, cartridge: Cartridge) -> Self{
        Self {
            cpu_clock: 0,

            ppu_clock: 0,
            ppu,
            cartridge,

            wram: [0; WRAM_SIZE],
            io_reg: [0; APU_IO_REG_SIZE],

            oam_dma_cpu_suspend_cycles: 0,

            pad1: Default::default(),
            pad2: Default::default(),
        }
    }

    // An OAM DMA is currently handled immediately and assumed to not be observed by anything
    // (considering the CPU is going to be suspended)
    fn run_dma(&mut self, cpu_start_addr: u16) {
        for offset in 0..256 {
            let cpu_addr = cpu_start_addr.wrapping_add(offset);
            let cpu_data = self.read_u8(cpu_addr);
            self.ppu.write_u8(&mut self.cartridge, 0x2004 /* OAMDATA */, cpu_data);
        }
    }

    pub fn step_ppu(&mut self, ppu_clock: u64, fb: *mut u8) -> PpuStatus {
        self.ppu_clock = ppu_clock;
        self.ppu.step(ppu_clock, &mut self.cartridge, fb)
    }
}