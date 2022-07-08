#[allow(unused_imports)]
use log::{error, trace, debug};

use crate::constants::*;
use crate::mappers::Mapper;
use crate::binary::INesConfig;
use crate::prelude::NameTableMirror;

use super::mirror_vram_address;

#[derive(Clone)]
pub struct Mapper0 {
    pub vram_mirror: NameTableMirror,
    pub vram: [u8; 2048],
    pub prg_rom: Vec<u8>,
    pub prg_ram: Vec<u8>,
    pub has_chr_ram: bool,
    pub chr_data: Vec<u8>, // may be ROM or RAM
}

impl Mapper0 {
    pub(crate) fn new_full(prg_rom: Vec<u8>, chr_data: Vec<u8>, has_writeable_chr_ram: bool, n_prg_ram_pages: usize, mirror: NameTableMirror) -> Self {
        Self {
            vram_mirror: mirror,
            vram: [0u8; 2048],
            prg_rom,
            prg_ram: vec![0u8; n_prg_ram_pages * PAGE_SIZE_16K],
            has_chr_ram: has_writeable_chr_ram,
            chr_data,
         }
    }

    pub fn new(config: &INesConfig, prg_rom: Vec<u8>, chr_data: Vec<u8>) -> Self {
        Mapper0::new_full(prg_rom, chr_data, config.has_chr_ram, config.n_prg_ram_pages, config.nametable_mirror)
    }
}

impl Mapper for Mapper0 {
    fn reset(&mut self) {}

    fn clone_mapper(&self) -> Box<dyn Mapper> {
        Box::new(self.clone())
    }

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
            0x2000..=0x3fff => { // VRAM
                let off = mirror_vram_address(addr, self.vram_mirror);
                arr_read!(self.vram, off)
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
        match addr {
            0x0000..=0x1fff => {
                if self.has_chr_ram {
                    let index = addr as usize;
                    arr_write!(self.chr_data, index, data);
                }
            },
            0x2000..=0x3fff => { // VRAM
                let off = mirror_vram_address(addr, self.vram_mirror);
                arr_write!(self.vram, off, data);
            }
            _ => {
                panic!("Unexpected PPU write via mapper, address = 0x{addr:x}");
            }
        }
    }

    fn mirror_mode(&self) -> NameTableMirror { self.vram_mirror }
    fn irq(&self) -> bool { false }
}


#[test]
fn test_mapper0_vram_mirroring() {

    let cfg = INesConfig {
        version: 1,
        mapper_number: 0,
        tv_system: crate::prelude::TVSystem::Ntsc,
        n_prg_rom_pages: 2,
        n_prg_ram_pages: 2,
        n_chr_rom_pages: 2,
        n_chr_ram_pages: 0,
        has_chr_ram: true,
        has_battery: false,
        has_trainer: false,
        nametable_mirror: NameTableMirror::Vertical,
        four_screen_vram: false,
        trainer_baseaddr: None,
        prg_rom_baseaddr: 0,
        chr_rom_baseaddr: 0
    };
    let prg_rom = vec![0u8; cfg.prg_rom_bytes()];
    let chr_data = vec![0u8; cfg.chr_rom_bytes()];

    let mut mapper = Mapper0::new(&cfg, prg_rom, chr_data);

    mapper.vram = [0u8; 2048];
    mapper.vram_mirror = NameTableMirror::Vertical;
    mapper.ppu_bus_write(0x2001, 1);
    mapper.ppu_bus_write(0x2401, 2);
    debug_assert_eq!(mapper.vram[1], 1);
    debug_assert_eq!(mapper.ppu_bus_read(0x2801), 1);
    debug_assert_eq!(mapper.vram[1025], 2);
    debug_assert_eq!(mapper.ppu_bus_read(0x2c01), 2);

    mapper.vram = [0u8; 2048];
    mapper.vram_mirror = NameTableMirror::Vertical;
    mapper.ppu_bus_write(0x2801, 1);
    mapper.ppu_bus_write(0x2c01, 2);
    debug_assert_eq!(mapper.vram[1], 1);
    debug_assert_eq!(mapper.ppu_bus_read(0x2001), 1);
    debug_assert_eq!(mapper.vram[1025], 2);
    debug_assert_eq!(mapper.ppu_bus_read(0x2401), 2);
}