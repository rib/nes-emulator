#[allow(unused_imports)]
use log::{debug, error, trace};

use crate::binary::INesConfig;
use crate::cartridge::NameTableMirror;
use crate::constants::*;
use crate::mappers::Mapper;

use super::bank_select_mask;
use super::mirror_vram_address;

/// iNES Mapper 002: AKA UxROM
///
/// PRG ROM capacity	256K/4096K
/// PRG ROM window	16K + 16K fixed
/// PRG RAM capacity	None
/// CHR capacity	8K
/// CHR window	n/a
/// Nametable mirroring	Fixed H or V, controlled by solder pads
/// Bus conflicts	Yes/No (original UxROM HW had conflicts but emulators should assume no conflicts for this mapper)
///
/// # Example games:
/// * Mega Man
/// * Castlevania
/// * Contra
/// * Duck Tales
/// * Metal Gear
///
/// # Banks
/// CPU $8000-$BFFF: 16 KB switchable PRG ROM bank
/// CPU $C000-$FFFF: 16 KB PRG ROM bank, fixed to the last bank
///
/// The original UxROM boards used by Nintendo were subject to bus conflicts,
/// and the relevant games all work around this in software. Some emulators
/// (notably FCEUX) will have bus conflicts by default, but others have none.
/// NES 2.0 submappers were assigned to accurately specify whether the game
/// should be emulated with bus conflicts.
///
#[derive(Clone)]
pub struct Mapper2 {
    vram_mirror: NameTableMirror,
    vram: [u8; 2048],
    prg_rom: Vec<u8>,
    chr_data: Vec<u8>,

    has_bus_conflicts: bool,

    bank0_select_mask: u8,
    n_prg_pages: u8,

    bank0_offset: usize,
    bank1_offset: usize,
}

impl Mapper2 {
    pub fn new(config: &INesConfig, prg_rom: Vec<u8>, chr_data: Vec<u8>) -> Self {
        // We expect the PRG / CHR data to be padded to have a page aligned size
        // when they are loaded
        debug_assert_eq!(prg_rom.len() % PAGE_SIZE_16K, 0);
        debug_assert!(config.n_prg_rom_pages > 0);
        debug_assert!(config.n_prg_rom_pages <= 256);
        // TODO: return an Err for this validation!

        // An appropriate iNes config should mean code will never try to access
        // non-existent pages, but just in case we count the number of pages we
        // have and will wrap out-of-bounds page selections.
        let n_prg_pages = config.n_prg_rom_pages as u8;

        let bank0_select_mask = bank_select_mask(n_prg_pages);
        let bank1_offset = ((n_prg_pages - 1) as usize) * PAGE_SIZE_16K;
        Self {
            vram_mirror: config.nametable_mirror,
            vram: [0u8; 2048],
            prg_rom,
            chr_data,

            has_bus_conflicts: false,

            bank0_select_mask,
            n_prg_pages,
            bank0_offset: 0,
            bank1_offset,
        }
    }

    fn system_bus_read_direct(&self, addr: u16) -> u8 {
        match addr {
            0x8000..=0xbfff => {
                let off = addr as usize - 0x8000 + self.bank0_offset;
                arr_read!(self.prg_rom, off)
            }
            0xc000..=0xffff => {
                let off = addr as usize - 0xc000 + self.bank1_offset;
                arr_read!(self.prg_rom, off)
            }
            _ => return 0,
        }
    }
}

impl Mapper for Mapper2 {
    fn reset(&mut self) {}

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
        if self.has_bus_conflicts {
            let conflicting_read = self.system_bus_read_direct(addr);
            data = data & conflicting_read;
        }

        match addr {
            0x8000..=0xffff => {
                // PRG Bank Select
                let page_select = (data & self.bank0_select_mask) % self.n_prg_pages;
                self.bank0_offset = PAGE_SIZE_16K * (page_select as usize);
            }
            _ => {}
        }
    }

    fn ppu_bus_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x1fff => {
                arr_read!(self.chr_data, addr as usize)
            }
            0x2000..=0x3fff => {
                // VRAM
                arr_read!(self.vram, mirror_vram_address(addr, self.vram_mirror))
            }
            _ => 0,
        }
    }

    fn ppu_bus_peek(&mut self, addr: u16) -> u8 {
        self.ppu_bus_read(addr)
    }

    fn ppu_bus_write(&mut self, addr: u16, data: u8) {
        match addr {
            0x0000..=0x1fff => {
                arr_write!(self.chr_data, addr as usize, data);
            }
            0x2000..=0x3fff => {
                // VRAM
                arr_write!(self.vram, mirror_vram_address(addr, self.vram_mirror), data);
            }
            _ => {}
        }
    }

    fn mirror_mode(&self) -> NameTableMirror {
        self.vram_mirror
    }
}
