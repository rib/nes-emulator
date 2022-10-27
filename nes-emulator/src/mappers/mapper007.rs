#[allow(unused_imports)]
use log::{debug, error, trace};

use crate::binary::INesConfig;
use crate::cartridge::NameTableMirror;
use crate::constants::*;
use crate::mappers::Mapper;

use super::bank_select_mask;
use super::mirror_vram_address;

/// iNES Mapper 007: aka AxROM
///
/// # Properties
/// |                     |                        |
/// |---------------------|------------------------|
/// | PRG ROM capacity | 256K |
/// | PRG ROM window | 32K |
/// | PRG RAM capacity | None |
/// | PRG RAM window | n/a |
/// | CHR capacity | 8K |
/// | CHR window | n/a |
/// | Nametable mirroring | 1 page switchable |
/// | Bus conflicts | AMROM/AOROM only |
///
/// # Banks
/// - CPU $8000-$FFFF: 32 KB switchable PRG ROM bank
///
/// # Nesdev
/// https://www.nesdev.org/wiki/AxROM
///
#[derive(Clone)]
pub struct Mapper7 {
    vram: [u8; 2048],
    single_screen_offset: usize,

    prg_rom: Vec<u8>,
    chr_data: Vec<u8>,

    // TODO: support NES 2.0 submapper information to configure this...
    // Ref: https://www.nesdev.org/wiki/NES_2.0_submappers
    has_bus_conflicts: bool,

    // The original CNROM only supported two bits for selecting the page
    // (limited to 32k), but the iNes mapper 003 supports an 8bit page
    // index. Nesdev doesn't have any specific notes warning about this
    // but in practice we see that games do set higher bits so we have
    // to take into account the size of the CHR ROM to know how many
    // bits of the CHR page select are valid
    prg_bank_select_mask: u8,
    n_prg_pages: u8,
    prg_bank: usize,
}

impl Mapper7 {
    pub fn new(config: &INesConfig, prg_rom: Vec<u8>, chr_data: Vec<u8>) -> Self {
        // Although we expect the PRG / CHR data to be padded to have a page aligned size
        // when they are loaded (16K and 8K alignment respectively), for this mapper
        // we have 32k PRG pages
        debug_assert_eq!(prg_rom.len() % PAGE_SIZE_32K, 0);
        debug_assert!(config.n_prg_rom_pages >= 2); // 32k window = 2 * 16k pages
        debug_assert!(config.n_prg_rom_pages <= 32); // Supports up to 512k (32 * 16k pages)

        // An appropriate iNes config should mean code will never try to access
        // non-existent pages, but just in case we count the number of pages we
        // have and will wrap out-of-bounds page selections.
        let n_prg_pages = config.n_prg_rom_pages as u8;
        debug_assert_eq!(n_prg_pages % 2, 0); // Same assertion as above for the len() really
        let n_prg_pages = n_prg_pages / 2; // convert from 16k page count to 32k pages
        let prg_bank_select_mask = bank_select_mask(n_prg_pages);
        log::debug!("Mapper007: PRG page select mask = {prg_bank_select_mask:08b}");
        Self {
            vram: [0u8; 2048],
            single_screen_offset: 0,

            prg_rom,
            chr_data,

            has_bus_conflicts: false, // Some AxROM games known to require no bus conflicts, none known to require conflicts

            prg_bank_select_mask,
            n_prg_pages,
            prg_bank: 0,
        }
    }

    fn system_bus_read_direct(&self, addr: u16) -> u8 {
        match addr {
            0x8000..=0xffff => {
                let off = addr as usize - 0x8000 + self.prg_bank;
                arr_read!(self.prg_rom, off as usize)
            }
            _ => 0,
        }
    }
}

impl Mapper for Mapper7 {
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
            data &= conflicting_read;
        }

        if let 0x8000..=0xffff = addr {
            // PRG Bank Select
            let page_select = (data & 0b1111 & self.prg_bank_select_mask) % self.n_prg_pages;
            self.prg_bank = PAGE_SIZE_32K * page_select as usize;

            self.single_screen_offset = if data & 0b1_0000 == 0 {
                0
            } else {
                PAGE_SIZE_1K
            };
        }
    }

    fn ppu_bus_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x1fff => {
                arr_read!(self.chr_data, addr as usize)
            }
            0x2000..=0x3fff => {
                // VRAM
                arr_read!(
                    self.vram,
                    mirror_vram_address(addr, NameTableMirror::SingleScreenA)
                )
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
                arr_write!(
                    self.vram,
                    mirror_vram_address(addr, NameTableMirror::SingleScreenA),
                    data
                );
            }
            _ => {}
        }
    }

    fn mirror_mode(&self) -> NameTableMirror {
        NameTableMirror::SingleScreenA
    }
}
