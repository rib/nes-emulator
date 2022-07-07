#[allow(unused_imports)]
use log::{error, trace, debug};

use crate::constants::*;
use crate::mappers::Mapper;
use crate::binary::INesConfig;

pub struct Mapper0 {
    pub prg_rom: Vec<u8>,
    pub prg_ram: Vec<u8>,
    pub has_chr_ram: bool,
    pub chr_data: Vec<u8>, // may be ROM or RAM
}

impl Mapper0 {
    pub fn new(n_prg_ram_pages: usize, has_chr_ram: bool, prg_rom: Vec<u8>, chr_data: Vec<u8>) -> Self {
        Mapper0 {
            prg_rom,
            prg_ram: vec![0u8; n_prg_ram_pages * PAGE_SIZE_16K],
            has_chr_ram: has_chr_ram,
            chr_data,
         }
    }
    pub fn new_from_ines(config: &INesConfig, prg_rom: Vec<u8>, chr_data: Vec<u8>) -> Self {
        Self::new(config.n_prg_ram_pages, config.has_chr_ram, prg_rom, chr_data)
    }
}

impl Mapper for Mapper0 {
    fn reset(&mut self) {}

    fn system_bus_read(&mut self, addr: u16) -> (u8, u8) {
        let value = match addr {
            0x6000..=0x7fff => { // PRG RAM
                let ram_offset = (addr - 0x6000) as usize;
                self.prg_ram[ram_offset]
            }
            0x8000..=0xbfff => { // First 16 KB of ROM
                let addr = (addr - 0x8000) as usize;
                self.prg_rom[addr]
            }
            0xc000..=0xffff => { // Last 16 KB of ROM (NROM-256) or mirror of $8000-$BFFF (NROM-128)
                let addr = (addr - 0x8000) as usize;
                if addr >= self.prg_rom.len() {
                    self.prg_rom[addr - self.prg_rom.len()]
                } else {
                    self.prg_rom[addr]
                }
            }
            _ => {
                error!("Invalid mapper read @ {}", addr);
                return (0, 0xff)
            }
        };

        (value, 0) // No undefined (open bus) bits
    }

    fn system_bus_peek(&mut self, addr: u16) -> (u8, u8) {
        self.system_bus_read(addr)
    }

    fn system_bus_write(&mut self, addr: u16, data: u8) {
        match addr {
            0x6000..=0x7fff => {
                let ram_offset = (addr - 0x6000) as usize;
                self.prg_ram[ram_offset] = data;
            }
            _ => { trace!("unhandled system bus write in cartridge"); }
        }
    }

    fn ppu_bus_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x1fff => {
                let index = addr as usize;
                arr_read!(self.chr_data, index)
            }
            _ => {
                trace!("Unexpected PPU read via mapper, address = {}", addr);
                0
             }
        }
    }

    fn ppu_bus_peek(&mut self, addr: u16) -> u8 {
        self.ppu_bus_read(addr)
    }

    fn ppu_bus_write(&mut self, addr: u16, data: u8) {
        if self.has_chr_ram {
            match addr {
                0x0000..=0x1fff => {
                    let index = addr as usize;
                    arr_write!(self.chr_data, index, data);
                },
                _ => {
                    trace!("Unexpected PPU write via mapper, address = {}", addr);
                }
            }
        }
    }

    fn irq(&self) -> bool { false }
}