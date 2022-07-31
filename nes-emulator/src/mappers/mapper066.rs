#[allow(unused_imports)]
use log::{error, trace, debug};

use crate::constants::*;
use crate::mappers::Mapper;
use crate::binary::INesConfig;
use crate::cartridge::NameTableMirror;

use super::mirror_vram_address;
use super::bank_select_mask;


/// iNES Mapper 066: AKA GxROM
///
/// Boards	GNROM,MHROM
/// PRG ROM capacity	128KiB (512KiB oversize)
/// PRG ROM window	32KiB
/// PRG RAM capacity	None
/// CHR capacity	32KiB (128KiB oversize)
/// CHR window	8KiB
/// Nametable mirroring	Fixed H or V, controlled by solder pads
/// Bus conflicts	Yes
///
/// # Example games:
/// * Doraemon
/// * Dragon Power
/// * Gumshoe
/// * Thunder & Lightning
/// * Super Mario Bros. + Duck Hunt (MHROM)
///
/// # Board Types
/// -------------------------------
/// | Board |   PRG  |     CHR    |
/// |-------|--------|------------|
/// | GNROM	| 128 KB | 32 KB      |
/// | MHROM	| 64 KB	 | 16 / 32 KB |
/// -------------------------------
///
/// # Banks
/// CPU $8000-$FFFF: 32 KB switchable PRG ROM bank
/// PPU $0000-$1FFF: 8 KB switchable CHR ROM bank
///
#[derive(Clone)]
pub struct Mapper66 {
    vram_mirror: NameTableMirror,
    vram: [u8; 2048],
    prg_rom: Vec<u8>,
    chr_data: Vec<u8>,
    has_chr_ram: bool,

    prg_bank_select_mask: u8,
    n_prg_pages: u8,
    prg_bank_offset: usize,

    chr_bank_select_mask: u8,
    n_chr_pages: u8,
    chr_bank_offset: usize,
}

impl Mapper66 {
    pub fn new(config: &INesConfig, prg_rom: Vec<u8>, chr_data: Vec<u8>) -> Self {
        // Although we expect the PRG / CHR data to be padded to have a page aligned size
        // when they are loaded (16K and 8K alignment respectively), for this mapper
        // we have 32k PRG pages
        debug_assert_eq!(prg_rom.len() % PAGE_SIZE_32K, 0);
        debug_assert!(config.n_prg_rom_pages >= 2); // 32k window = 2 * 16k pages
        debug_assert!(config.n_prg_rom_pages <= 32); // Supports up to 512k (32 * 16k pages)

        debug_assert_eq!(chr_data.len() % PAGE_SIZE_8K, 0);
        let n_chr_pages = chr_data.len() / PAGE_SIZE_8K;
        debug_assert!(n_chr_pages >= 1);
        debug_assert!(n_chr_pages <= 16); // Supports up to 256k (16 * 8k pages)
        // TODO: return an Err for this validation!

        // An appropriate iNes config should mean code will never try to access
        // non-existent pages, but just in case we count the number of pages we
        // have and will wrap out-of-bounds page selections.
        let n_prg_pages = config.n_prg_rom_pages as u8;
        debug_assert_eq!(n_prg_pages % 2, 0); // Same assertion as above for the len() really
        let n_prg_pages = n_prg_pages / 2; // convert from 16k page count to 32k pages
        let prg_bank_select_mask = bank_select_mask(n_prg_pages);
        log::debug!("Mapper066: PRG page select mask = {prg_bank_select_mask:08b}");

        let n_chr_pages = n_chr_pages as u8;
        let chr_bank_select_mask = bank_select_mask(n_chr_pages);
        log::debug!("Mapper066: CHR page select mask = {prg_bank_select_mask:08b}");

        Self {
            vram_mirror: config.nametable_mirror,
            vram: [0u8; 2048],
            prg_rom,
            chr_data,
            has_chr_ram: config.has_chr_ram,

            prg_bank_select_mask,
            n_prg_pages,
            //prg_bank_offset: PAGE_SIZE_32K * (n_prg_pages as usize - 1),
            prg_bank_offset: 0,

            chr_bank_select_mask,
            n_chr_pages,
            chr_bank_offset: 0,
        }
    }

    fn system_bus_read_direct(&self, addr: u16) -> u8 {
         match addr {
            0x8000..=0xffff => {
                let off = addr as usize - 0x8000 + self.prg_bank_offset;
                //println!("PRG reading @ {off}/{off:x} (bank off = {}/{:x}) (address = {addr:04x})", self.prg_bank_offset, self.prg_bank_offset);
                arr_read!(self.prg_rom, off)
            }
            _ => {
                return 0
            }
        }
    }
}

impl Mapper for Mapper66 {
    fn clone_mapper(&self) -> Box<dyn Mapper> {
        Box::new(self.clone())
    }

    fn system_bus_read(&mut self, addr: u16) -> (u8, u8) {
        (self.system_bus_read_direct(addr), 0) // no undefined bits
    }

    fn system_bus_peek(&mut self, addr: u16) -> (u8, u8) {
        self.system_bus_read(addr)
    }

    fn system_bus_write(&mut self, addr: u16, mut data: u8) {
        // Apply bus conflicts
        let conflicting_read = self.system_bus_read_direct(addr);
        data = data & conflicting_read;

        match addr {
            0x8000..=0xffff => { // Bank Select
                let prg_page_select = (((data & 0b1111_0000) >> 4) & self.prg_bank_select_mask) % self.n_prg_pages;
                self.prg_bank_offset = PAGE_SIZE_32K * (prg_page_select as usize);
                let chr_page_select = ((data & 0b1111) & self.chr_bank_select_mask) % self.n_chr_pages;
                self.chr_bank_offset = PAGE_SIZE_8K * (chr_page_select as usize);
                //log::debug!("Mapper066: Bank Select via {addr:4x} {data:08b}, prg page = {prg_page_select}, prg offset = {}/{:x}", self.prg_bank_offset, self.prg_bank_offset);
            }
            _ => {}
        }
    }

    fn ppu_bus_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x1fff => {
                let off = addr as usize + self.chr_bank_offset;
                arr_read!(self.chr_data, off)
            }
            0x2000..=0x3fff => { // VRAM
                arr_read!(self.vram, mirror_vram_address(addr, self.vram_mirror))
            }
            _ => { 0 }
        }
    }

    fn ppu_bus_peek(&mut self, addr: u16) -> u8 {
        self.ppu_bus_read(addr)
    }

    fn ppu_bus_write(&mut self, addr: u16, data: u8) {
        match addr {
            0x0000..=0x1fff => {
                if self.has_chr_ram {
                    let off = addr as usize + self.chr_bank_offset;
                    arr_write!(self.chr_data, off, data);
                }
            },
            0x2000..=0x3fff => { // VRAM
                arr_write!(self.vram, mirror_vram_address(addr, self.vram_mirror), data);
            }
            _ => {}
        }
    }

    fn mirror_mode(&self) -> NameTableMirror { self.vram_mirror }
}