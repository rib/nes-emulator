#[allow(unused_imports)]
use log::{error, trace, debug};

use crate::cartridge::NameTableMirror;
use crate::constants::*;
use crate::mappers::Mapper;
use crate::binary::INesConfig;

use super::mirror_vram_address;

#[derive(Clone)]
pub struct Mapper0 {
    pub vram_mirror: NameTableMirror,
    pub vram: [u8; 2048],
    pub prg_rom: Vec<u8>,
    pub prg_ram: Vec<u8>,
    pub last_prg_page_off: usize,
    pub has_chr_ram: bool,
    pub chr_data: Vec<u8>, // may be ROM or RAM
}

impl Mapper0 {
    pub(crate) fn new_full(prg_rom: Vec<u8>, chr_data: Vec<u8>, has_writeable_chr_ram: bool, n_prg_ram_pages: usize, mirror: NameTableMirror) -> Self {
        // We expect the PRG / CHR data to be padded to have a page aligned size
        // when they are loaded
        debug_assert_eq!(prg_rom.len() % PAGE_SIZE_16K, 0);

        let last_prg_page_off = if prg_rom.len() > PAGE_SIZE_16K { PAGE_SIZE_16K } else { 0 };
        Self {
            vram_mirror: mirror,
            vram: [0u8; 2048],
            prg_rom,
            prg_ram: vec![0u8; n_prg_ram_pages * PAGE_SIZE_16K],
            has_chr_ram: has_writeable_chr_ram,
            last_prg_page_off,
            chr_data,
         }
    }

    pub fn new(config: &INesConfig, prg_rom: Vec<u8>, chr_data: Vec<u8>) -> Self {
        Mapper0::new_full(prg_rom, chr_data, config.has_chr_ram, config.n_prg_ram_pages, config.nametable_mirror)
    }
}

impl Mapper for Mapper0 {
    fn clone_mapper(&self) -> Box<dyn Mapper> {
        Box::new(self.clone())
    }

    fn system_bus_read(&mut self, addr: u16) -> (u8, u8) {
        match addr {
            0x6000..=0x7fff => { // PRG RAM, 8k window (2k or 4k physical ram for FamilyBasic)
                if self.prg_ram.len() > 0 {
                    let offset = (addr - 0x6000) as usize % self.prg_ram.len();
                    (arr_read!(self.prg_ram, offset), 0)
                } else {
                    //log::warn!("Invalid mapper read @ {}", addr);
                    (0, 0xff)
                }
            }
            0x8000..=0xbfff => { // First 16 KB of ROM
                let addr = (addr - 0x8000) as usize;
                (arr_read!(self.prg_rom, addr), 0)
            }
            0xc000..=0xffff => { // Last 16 KB of ROM (NROM-256) or mirror of $8000-$BFFF (NROM-128)
                let addr = (addr - 0xc000) as usize + self.last_prg_page_off;
                (arr_read!(self.prg_rom, addr), 0)
            }
            _ => {
                //log::warn!("Invalid mapper read @ {}", addr);
                (0, 0xff)
            }
        }
    }

    fn system_bus_peek(&mut self, addr: u16) -> (u8, u8) {
        self.system_bus_read(addr)
    }

    fn system_bus_write(&mut self, addr: u16, data: u8) {
        match addr {
            0x6000..=0x7fff => {
                let ram_offset = (addr - 0x6000) as usize;
                arr_write!(self.prg_ram, ram_offset, data);
            }
            _ => { trace!("unhandled system bus write in cartridge"); }
        }
    }

    fn ppu_bus_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x1fff => {
                arr_read!(self.chr_data, addr as usize)
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
                    let off = addr as usize;
                    //println!("cartridge CHR RAM write 0x{:04x} ({}) = 0x{:02x}", addr, off, data);
                    arr_write!(self.chr_data, off, data);
                }
            },
            0x2000..=0x3fff => { // VRAM
                let off = mirror_vram_address(addr, self.vram_mirror);
                //println!("cartridge vram write 0x{:04x} ({}) = 0x{:02x}", addr, off, data);
                arr_write!(self.vram, off, data);
            }
            _ => {
                panic!("Unexpected PPU write via mapper, address = 0x{addr:x}");
            }
        }
    }

    fn mirror_mode(&self) -> NameTableMirror { self.vram_mirror }
}


#[test]
fn test_mapper0_vram_mirroring() {

    use crate::cartridge::TVSystemCompatibility;

    let cfg = INesConfig {
        version: 1,
        mapper_number: 0,
        tv_system: TVSystemCompatibility::Ntsc,
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