use crate::apu::apu::Apu;
//use crate::stash_apu::Apu;
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
use bitflags::bitflags;

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

bitflags! {
    pub struct WatchOps: u8 {
        const READ =  0b1;
        const WRITE = 0b10;
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchPoint {
    pub address: u16,
    pub ops: WatchOps,
}

// The DMC doesn't have direct access to the system bus within
// apu.step() so any DMA needs to requested, and the result
// will be passed back
pub struct DmcDmaRequest {
    pub address: u16
}

pub struct System {

    pub cpu_clock: u64,

    pub ppu_clock: u64,
    pub ppu: Ppu,

    pub apu_clock: u64,
    pub apu: Apu,

    pub open_bus_value: u8,

    /// 0x0000 - 0x07ff: WRAM
    /// 0x0800 - 0x1f7ff: WRAM  Mirror x3
    pub wram: [u8; WRAM_SIZE],

    //  0x4000 - 0x401f: APU I/O, PAD
    pub io_reg: [u8; APU_IO_REG_SIZE],

    // If the CPU starts an OAM DMA then the CPU will be suspended for 513 or 514 clock cycles
    // If the CPU starts a DCM sample buffer DMA the CPU will be suspended for 4 clock cycles
    pub dma_cpu_suspend_cycles: u16,

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

    pub watch_points: Vec<WatchPoint>,
    pub watch_hit: bool,
}

impl System {

    pub fn new(ppu: Ppu, apu: Apu, cartridge: Cartridge) -> Self{
        Self {
            cpu_clock: 0,

            ppu_clock: 0,
            ppu,

            apu_clock: 0,
            apu,

            cartridge,

            wram: [0; WRAM_SIZE],
            io_reg: [0; APU_IO_REG_SIZE],

            dma_cpu_suspend_cycles: 0,

            pad1: Default::default(),
            pad2: Default::default(),

            open_bus_value: 0,

            watch_points: vec![],
            watch_hit: false,
        }
    }

    /// Apply the open bus bits and update the open bus value for future reads
    fn apply_open_bus_bits_mut(&mut self, mut value: u8, undefined_bits: u8) -> u8 {
        value = value & !undefined_bits;
        value |= self.open_bus_value & undefined_bits;
        self.open_bus_value = value;
        value
    }

    /// Apply the open bus bits without additional side effects (for peeking)
    fn apply_open_bus_bits(&self, mut value: u8, undefined_bits: u8) -> u8 {
        value = value & !undefined_bits;
        value |= self.open_bus_value & undefined_bits;
        value
    }

    pub fn read(&mut self, addr: u16) -> u8 {
        if self.watch_points.len() > 0 {
            for w in &self.watch_points {
                if w.address == addr && w.ops.contains(WatchOps::READ) {
                    self.watch_hit = true;
                    break;
                }
            }
        }
        let (value, undefined_bits) = match addr {
            0x0000..=0x1fff => { // RAM
                //println!("system read {addr:x}");
                // mirror support
                let index = usize::from(addr) % self.wram.len();
                (arr_read!(self.wram, index), 0)
            }
            0x2000..=0x3fff => { // PPU I/O
                // PPU read handles open bus behaviour, so we assume there
                // are no undefined bits at this point
                (self.ppu.read(&mut self.cartridge, addr), 0)
            }
            0x4000..=0x401f => {  // APU I/O
                let index = usize::from(addr - APU_IO_REG_BASE_ADDR);
                match index {
                    0x14 => { // Write-only OAMDMA
                        (0, 0xff)
                    }
                    0x16 => (self.pad1.read(), 0b1110_0000), // pad1
                    0x17 => (self.pad2.read(), 0b1110_0000), // pad2
                    _ => {
                        self.apu.read(addr)
                        //arr_read!(self.io_reg, index),
                    }
                }
            }
            _ => { // Cartridge
                //println!("calling cartridge read_u8 for {addr:x}");
                self.cartridge.system_bus_read(addr)
            }
        };

        let value = self.apply_open_bus_bits_mut(value, undefined_bits);
        //if addr == 0x4016 {
        //    println!("Read $4016 as {value:02x} / {value:08b}");
        //}
        value
    }

    /// Read without side-effects
    ///
    /// Use this for debugging purposes to be able to inspect memory and registers
    /// without affecting any state.
    pub fn peek(&mut self, addr: u16) -> u8 {

        let (value, undefined_bits) = match addr {
            0x0000..=0x1fff => { // RAM
                let index = usize::from(addr) % self.wram.len();
                (arr_read!(self.wram, index), 0)
            }
            0x2000..=0x3fff => { // PPU I/O
                // PPU read handles open bus behaviour, so we assume there
                // are no undefined bits at this point
                (self.ppu.peek(&mut self.cartridge, addr), 0)
            }
            0x4000..=0x401f => {  // APU I/O
                let index = usize::from(addr - APU_IO_REG_BASE_ADDR);
                match index {
                    0x14 => { // Write-only OAMDMA
                        (0, 0xff)
                    }
                    0x16 => (self.pad1.peek(), 0b1110_0000), // pad1
                    0x17 => (self.pad2.peek(), 0b1110_0000), // pad2
                    _ => {
                        self.apu.peek(addr)
                    }
                }
            }
            _ => { // Cartridge
                self.cartridge.system_bus_peek(addr)
            }
        };

        let value = self.apply_open_bus_bits(value, undefined_bits);
        value
    }

    pub fn write(&mut self, addr: u16, data: u8) {
        if self.watch_points.len() > 0 {
            for w in &self.watch_points {
                if w.address == addr && w.ops.contains(WatchOps::WRITE) {
                    self.watch_hit = true;
                    break;
                }
            }
        }

        match addr {
            0x0000..=0x1fff => { // RAM
                // mirror support
                let index = usize::from(addr) % self.wram.len();
                arr_write!(self.wram, index, data);
            }
            0x2000..=0x3fff => { // PPU I/O
                self.ppu.write(&mut self.cartridge, addr, data);
            }
            0x4000..=0x401f => {  // APU I/O
                let index = usize::from(addr - 0x4000);
                match index {
                    0x14 => { // OAMDMA
                        //println!("start OAM DMA");

                        self.dma_cpu_suspend_cycles = 513;
                        if self.cpu_clock % 2 == 1 {
                            self.dma_cpu_suspend_cycles += 1;
                        }
                        self.run_dma((data as u16) << 8);
                    }
                    0x16 => {
                        // This register is split between being an APU register and a controller register
                        self.pad1.write_register(data);
                        self.pad2.write_register(data);
                        self.apu.write(addr, data);
                    },
                    0x17 => {
                        // This register is split between being an APU register and a controller register
                        self.apu.write(addr, data);
                    }
                    _ => {
                        self.apu.write(addr, data);
                    }
                }
                //arr_write!(self.io_reg, index, data);
            }
            _ => { // Cartridge
                self.cartridge.system_bus_write(addr, data);
            }
        }
    }

    // An OAM DMA is currently handled immediately and assumed to not be observed by anything
    // (considering the CPU is going to be suspended)
    fn run_dma(&mut self, cpu_start_addr: u16) {
        for offset in 0..256 {
            let cpu_addr = cpu_start_addr.wrapping_add(offset);
            let cpu_data = self.read(cpu_addr);
            self.ppu.write(&mut self.cartridge, 0x2004 /* OAMDATA */, cpu_data);
        }
    }

    pub fn step_ppu(&mut self, ppu_clock: u64, fb: *mut u8) -> PpuStatus {
        self.ppu_clock = ppu_clock;
        self.ppu.step(ppu_clock, &mut self.cartridge, fb)
    }

    // Returns: number of cycles to pause the CPU (form DMC sample buffer DMA)
    pub fn step_apu(&mut self) {
        self.apu_clock += 1;
        self.apu.step(self.apu_clock);
    }

    pub fn add_watch(&mut self, addr: u16, ops: WatchOps) {
        if let Some(i) = self.watch_points.iter().position(|w| w.address == addr) {
            self.watch_points.swap_remove(i);
        }
        self.watch_points.push(WatchPoint {
            address: addr,
            ops
        })
    }

}