use crate::apu::Apu;
use crate::cartridge;
use crate::cpu::Interrupt;
use crate::ppu::OAM_DMA_COPY_SIZE_PER_PPU_STEP;
use crate::ppu::OAM_SIZE;
use crate::ppu::Ppu;
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
pub const CASSETTE_BASE_ADDR: u16 = 0x4020;

/// Memory Access Dispatcher
//#[derive(Clone)]
pub struct System {

    pub ppu: Ppu,

    /// 0x0000 - 0x07ff: WRAM
    /// 0x0800 - 0x1f7ff: WRAM  Mirror x3
    pub wram: [u8; WRAM_SIZE],

    //  0x4000 - 0x401f: APU I/O, PAD
    pub io_reg: [u8; APU_IO_REG_SIZE],

    pub written_oam_dma: bool,    // OAM_DMAが書かれた

    /// DMAが稼働中か示す
    /// DMAには513cycかかるが、Emulation上ppuのstep2回341cyc*2で完了するので実行中フラグで処理する
    /// 先頭でDMA開始されたとして、前半341cycで67%(170byte/256byte)処理できる(ので、次のstepで残りを処理したら次のDMA要求を受けても行ける)
    pub is_dma_running: bool,
    /// DMAのCPU側のベースアドレス。ページ指定なのでlower byteは0
    pub dma_cpu_src_addr: u16,
    /// DMAのOAM側のベースアドレス。256byteしたらwrapする(あまり使われないらしい)
    pub dma_oam_dst_addr: u8,

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
                        // Start OAM DMA
                        self.written_oam_dma = true; // OAM DMA
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
            ppu,
            cartridge,

            wram: [0; WRAM_SIZE],
            io_reg: [0; APU_IO_REG_SIZE],

            written_oam_dma: false,
            is_dma_running: false,
            dma_cpu_src_addr: 0,
            dma_oam_dst_addr: 0,

            pad1: Default::default(),
            pad2: Default::default(),
        }
    }

    /*************************** 0x4014: OAM_DMA ***************************/
    /// Returns whether DMA should be started and the forwarding address
    /// DMA開始が必要かどうかと、転送元アドレスを返す
    /// 面倒なので読み取ったらtriggerは揮発させる
    pub fn read_oam_dma(&mut self) -> (bool, u16) {
        let start_addr = u16::from(self.io_reg[APU_IO_OAM_DMA_OFFSET]) << 8;
        if self.written_oam_dma {
            self.written_oam_dma = false;
            (true, start_addr)
        } else {
            (false, start_addr)
        }
    }

    /// Perform DMA transfer (in two steps)
    /// `is_pre_transfer` --true for transfer immediately after receipt, false after ppu 1step
    fn run_dma(&mut self, is_pre_transfer: bool) {
        //println!("run dma");
        debug_assert!(
            (!self.is_dma_running && is_pre_transfer) || (self.is_dma_running && !is_pre_transfer)
        );
        debug_assert!((self.dma_cpu_src_addr & 0x00ff) == 0x0000);

        // address計算
        let start_offset: u8 = if is_pre_transfer {
            0
        } else {
            OAM_DMA_COPY_SIZE_PER_PPU_STEP
        };
        let cpu_start_addr: u16 = self.dma_cpu_src_addr.wrapping_add(u16::from(start_offset));
        let oam_start_addr: u8 = self.dma_oam_dst_addr.wrapping_add(start_offset);
        // 転送サイズ
        let transfer_size: u16 = if is_pre_transfer {
            OAM_DMA_COPY_SIZE_PER_PPU_STEP as u16
        } else {
            (OAM_SIZE as u16) - u16::from(OAM_DMA_COPY_SIZE_PER_PPU_STEP)
        };

        // 転送
        for offset in 0..transfer_size {
            let cpu_addr = cpu_start_addr.wrapping_add(offset);
            let oam_addr = usize::from(oam_start_addr.wrapping_add(offset as u8));

            let cpu_data = self.read_u8(cpu_addr);
            //println!("oam {cpu_data}");
            self.ppu.oam[oam_addr] = cpu_data;
        }

        // ステータス更新
        self.is_dma_running = is_pre_transfer;
    }

    pub fn step(&mut self, cpu_cyc: usize, apu: &mut Apu, fb: *mut u8) -> Option<Interrupt> {
        // OAM DMA
        if self.is_dma_running {
            // Do the rest of the last OAM DMA
            self.run_dma(false);
        }
        let (is_dma_req, dma_cpu_src_addr) = self.read_oam_dma();
        if is_dma_req {
            // Set and execute a new DMA descriptor
            self.dma_cpu_src_addr = dma_cpu_src_addr;
            self.dma_oam_dst_addr = self.ppu.oam_offset;
            self.run_dma(true);
        }

        self.ppu.step(cpu_cyc, &mut self.cartridge, fb)
    }
}