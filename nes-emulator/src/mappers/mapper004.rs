#[allow(unused_imports)]
use log::{error, trace, debug};

use crate::constants::*;
use crate::mappers::Mapper;
use crate::binary::INesConfig;


/// iNES Mapper 004: AKA MMC3
/// TODO
pub struct Mapper4 {
    prg_rom: Vec<u8>,
    prg_ram: Vec<u8>,
    chr_data: Vec<u8>,
}

impl Mapper4 {
    pub fn new(config: &INesConfig, prg_rom: Vec<u8>, chr_data: Vec<u8>) -> Self {
        Self {
            prg_rom,
            prg_ram: vec![0u8; config.n_prg_ram_pages * PAGE_SIZE_16K],
            chr_data,
        }
    }
}

impl Mapper for Mapper4 {
    fn reset(&mut self) {}

    fn system_bus_read(&mut self, addr: u16) -> (u8, u8) {
        let value = match addr {
            0x6000..=0x7fff => { // 8 KB PRG RAM bank, (optional)
                let ram_offset = (addr - 0x6000) as usize;
                self.prg_ram[ram_offset]
            }
            _ => {
                todo!()
            }
        };

        (value, 0) // no undefined bits
    }

    fn system_bus_peek(&mut self, addr: u16) -> (u8, u8) {
        self.system_bus_read(addr)
    }

    fn system_bus_write(&mut self, addr: u16, mut data: u8) {
        match addr {
            _ => {
                todo!()
            }
        }
    }

    fn ppu_bus_read(&mut self, addr: u16) -> u8 {
        todo!()
    }

    fn ppu_bus_peek(&mut self, addr: u16) -> u8 {
        todo!()
    }

    fn ppu_bus_write(&mut self, addr: u16, data: u8) {
        todo!()
    }

    fn irq(&self) -> bool {
        todo!()
    }
}