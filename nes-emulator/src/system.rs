use crate::apu::apu::Apu;
use crate::ppu::Ppu;
use crate::ppusim::PpuSim;

use super::cartridge::*;
use super::pad::*;
use bitflags::bitflags;

pub const WRAM_SIZE: usize = 0x0800;
pub const APU_IO_REG_BASE_ADDR: u16 = 0x4000;

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

    //pub cpu_clock: u64,

   // pub ppu_clock: u64,
    pub ppu: Ppu,

    #[cfg(feature="sim")]
    pub ppu_sim: PpuSim,

    //pub apu_clock: u64,
    pub apu: Apu,

    /// Any time we step the APU we might get a DMA request from the DMC channel
    /// which the CPU needs to handle. The CPU will check this after any I/O
    /// or request to step the system.
    pub dmc_dma_request: Option<DmcDmaRequest>,

    pub open_bus_value: u8,

    /// 0x0000 - 0x07ff: WRAM
    /// 0x0800 - 0x1f7ff: WRAM  Mirror x3
    pub wram: [u8; WRAM_SIZE],

    // If the CPU starts an OAM DMA then the CPU will be suspended for 513 or 514 clock cycles
    // If the CPU starts a DCM sample buffer DMA the CPU will be suspended for 4 clock cycles
    //pub oam_dma_start_cycle: u64,
    //pub oam_dma_cpu_suspend_cycles: u16,
    //pub oam_dma_src_addr: u16,
    //pub oam_dma_last_offset: u16,

    pub cartridge: Cartridge,
    #[cfg(feature="sim")]
    pub ppu_sim_cartridge: Cartridge,

    pub pad1: Pad,
    pub pad2: Pad,

    pub watch_points: Vec<WatchPoint>,
    pub watch_hit: bool,
}

impl System {

    pub fn new(ppu: Ppu, ppu_sim: PpuSim, apu: Apu, cartridge: Cartridge) -> Self{
        Self {
            //cpu_clock: 0,

            //ppu_clock: 0,
            ppu,

            #[cfg(feature="sim")]
            ppu_sim,

            //apu_clock: 0,
            apu,
            dmc_dma_request: None,

            #[cfg(feature="sim")]
            ppu_sim_cartridge: cartridge.clone(),
            cartridge,

            wram: [0; WRAM_SIZE],

            //oam_dma_start_cycle: 0,
            //oam_dma_cpu_suspend_cycles: 0,
            //oam_dma_src_addr: 0,
            //oam_dma_last_offset: 0,

            pad1: Default::default(),
            pad2: Default::default(),

            open_bus_value: 0,

            watch_points: vec![],
            watch_hit: false,
        }
    }

    pub fn nmi_line(&self) -> bool {
        self.ppu.nmi_interrupt_raised
    }

    pub fn irq_line(&self) -> bool {
        self.apu.irq() || self.cartridge.mapper.irq()
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

    /// Perform a system bus read from the CPU
    ///
    /// Considering that all CPU IO takes one CPU clock cycle this will
    /// also step the APU and PPU attached to the system bus to help keep
    /// their clocks synchronized (we know this won't push their clock
    /// into the future)
    pub fn cpu_read(&mut self, addr: u16, cpu_clock: u64) -> u8 {
        if self.watch_points.len() > 0 {
            for w in &self.watch_points {
                if w.address == addr && w.ops.contains(WatchOps::READ) {
                    self.watch_hit = true;
                    break;
                }
            }
        }
        let (mut value, undefined_bits) = match addr {
            0x0000..=0x1fff => { // RAM
                //println!("system read {addr:x}");
                // mirror support
                let index = usize::from(addr) % self.wram.len();
                (arr_read!(self.wram, index), 0)
            }
            0x2000..=0x3fff => { // PPU I/O
                // Send any reads to the simulator for their side effects
                #[cfg(feature="sim")]
                {
                    self.ppu_sim.system_bus_read_start(addr);
                    // TODO: we also need to step the ppu_sim forward so we can
                    // read back a value
                    // XXX: to make that practical we need to give the sim ownership
                    // over its framebuffer otherwise we'd have to thread the
                    // framebuffer through read()s
                    //self.ppu_sim.sim_progress_for_read();
                    //(self.ppu_sim.data_bus, 0)
                }

                // PPU read handles open bus behaviour, so we assume there
                // are no undefined bits at this point
                (self.ppu.system_bus_read(&mut self.cartridge, addr), 0)
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
                    }
                }
            }
            _ => { // Cartridge
                //println!("calling cartridge read_u8 for {addr:x}");
                self.cartridge.system_bus_read(addr)
            }
        };

        //println!("Stepping system during CPU read");
        self.step_for_cpu_cycle();

        // If this is a PPU simulator read then we have to wait until after
        // stepping the PPU before we can access the value
        #[cfg(feature="sim")]
        {
            if let 0x2000..=0x3fff = addr {
                value = self.ppu_sim.data_bus;
            }
        }

        let value = self.apply_open_bus_bits_mut(value, undefined_bits);
        //if addr == 0x4016 {
        //    println!("Read $4016 as {value:02x} / {value:08b}");
        //}
        value
    }

    /// Read the system bus without side-effects
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
                (self.ppu.system_bus_peek(&mut self.cartridge, addr), 0)
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

    /// Perform a system bus write from the CPU
    pub fn cpu_write(&mut self, addr: u16, data: u8, cpu_clock: u64) {
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
                // mirror
                let index = usize::from(addr) % self.wram.len();
                arr_write!(self.wram, index, data);
            }
            0x2000..=0x3fff => { // PPU
                self.ppu.system_bus_write(&mut self.cartridge, addr, data);
                #[cfg(feature="sim")]
                self.ppu_sim.system_bus_write_start(addr, data);
            }
            0x4000..=0x401f => {  // APU + I/O
                let index = usize::from(addr - 0x4000);
                match index {
                    0x14 => {
                        // OAMDMA is handled directly within the CPU so nothing left to do here
                    },
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
            }
            _ => { // Cartridge
                self.cartridge.system_bus_write(addr, data);
                #[cfg(feature="sim")]
                self.ppu_sim_cartridge.system_bus_write(addr, data);
            }
        }

        //println!("Stepping system during CPU write");
        self.step_for_cpu_cycle();
    }

    #[cfg(feature="sim")]
    pub fn ppu_sim_step(&mut self, fb: *mut u8) -> PpuStatus {
        let sim_clks = self.ppu_sim.clk_per_pclk() * 2;
        let mut status = PpuStatus::None;
        for _ in 0..sim_clks {
            let new_status = self.ppu_sim.step_half(&mut self.cartridge, fb);
            if status == PpuStatus::None {
                status = new_status;
            }
        }
        status
    }

    pub fn ppu_step(&mut self) {
        self.ppu.step(&mut self.cartridge);

        //self.step_ppu_sim(fb)
    }
    pub fn ppu_clock(&self) -> u64 {
        self.ppu.clock
    }

    /// Returns: number of cycles to pause the CPU (form DMC sample buffer DMA)
    pub fn apu_step(&mut self) -> Option<DmcDmaRequest> {
        //self.apu_clock += 1;
        self.apu.step()
    }
    pub fn apu_clock(&self) -> u64 {
        self.apu.clock
    }

    /// Step everything connected to the system bus for a single CPU clock cycle
    ///
    /// It's guaranteed that this will be called once for each CPU cycle (usually as part of
    /// a `cpu_read` or cpu_write`) including cycles where it's "suspended" for OAMDMAs (where it's
    /// effectively just reading/writing on behalf of the PPU) or while halted for DMC DMA cycle stealing.
    ///
    /// Since the APU is clocked 1:1 by CPU clocks then it's enough to rely on this to clock
    /// the APU without any other mechanism to account for drift/divergence
    pub fn step_for_cpu_cycle(&mut self) {
        // The CPU should be checking for DMA requests after anything that might step
        // the system and the request should be taken / handled before stepping the
        // system again
        debug_assert!(self.dmc_dma_request.is_none());

        self.dmc_dma_request = self.apu_step();

        // There are always at least 3 pixel clocks per CPU cycle
        //
        // For PAL (3.2 pixel clocks) we will fall behind slightly within a single instruction
        // but that will be caught up in `Nes::progress()`
        //
        for _ in 0..3 {
            self.ppu_step();

            #[cfg(feature="sim")]
            self.ppu_sim_step();
        }
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