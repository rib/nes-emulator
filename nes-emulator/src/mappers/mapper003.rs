#[allow(unused_imports)]
use log::{error, trace, debug};

use crate::constants::*;
use crate::mappers::Mapper;
use crate::binary::INesConfig;
use crate::prelude::NameTableMirror;

use super::mirror_vram_address;


/// iNES Mapper 003: AKA CNROM
///
/// PRG ROM size: 16 KiB or 32 KiB
/// PRG ROM bank size: Not bankswitched
/// PRG RAM: None
/// CHR capacity: Up to 2048 KiB ROM
/// CHR bank size: 8 KiB
/// Nametable mirroring: Fixed vertical or horizontal mirroring
/// Subject to bus conflicts: Yes (CNROM), but not all compatible boards have bus conflicts.
///
/// Example Games:
/// * Solomon's Key
/// * Arkanoid
/// * Arkista's Ring
/// * Bump 'n' Jump
/// * Cybernoid
///
/// "For 16 KB PRG ROM testing, Joust (NES) makes a worthwhile test subject."
///
/// "Many CNROM games such as Milon's Secret Castle store data tables in
/// otherwise unused portions of CHR ROM and access them through PPUDATA ($2007)
/// reads. If an emulator can show the title screen of the NROM game Super Mario
/// Bros., but CNROM games don't work, the emulator's PPUDATA readback is likely
/// failing to consider CHR ROM bankswitching."
///
/// "The game Cybernoid seems to behave very strangely. It uses unprepared
/// system RAM, and it actually relies on bus conflicts (AND written value with
/// value read from address)! This bug manifests by CHR corruptions when the
/// player changes the audio from sound effects to music playback."
///
#[derive(Clone)]
pub struct Mapper3 {
    vram_mirror: NameTableMirror,
    vram: [u8; 2048],

    // TODO: support NES 2.0 submapper information to configure this...
    // Ref: https://www.nesdev.org/wiki/NES_2.0_submappers
    has_bus_conflicts: bool,

    prg_rom0: Vec<u8>, // first 16k bank
    prg_rom1: Vec<u8>, // second 16k bank

    chr_data: Vec<u8>,

    // The original CNROM only supported two bits for selecting the page
    // (limited to 32k), but the iNes mapper 003 supports an 8bit page
    // index. Nesdev doesn't have any specific notes warning about this
    // but in practice we see that games do set higher bits so we have
    // to take into account the size of the CHR ROM to know how many
    // bits of the CHR page select are valid
    chr_bank_select_mask: u8,
    chr_bank: usize,
}

impl Mapper3 {

    /// Determine the minimal mask of bits needed to be able to index
    /// all the CHR ROM pages
    fn chr_bank_select_mask(num_chr_rom_pages: u8) -> u8 {
        let l = num_chr_rom_pages.leading_zeros();
        let shift = 8 - l;
        let mask = ((1u16<<shift)-1) as u8;
        mask
    }

    pub fn new(config: &INesConfig, prg_rom: Vec<u8>, chr_data: Vec<u8>) -> Self {

        let prg_rom0 = prg_rom[0..PAGE_SIZE_16K].to_vec();

        let prg_rom1 = if prg_rom.len() >= (PAGE_SIZE_16K * 2) {
            prg_rom[PAGE_SIZE_16K..(PAGE_SIZE_16K * 2)].to_vec()
        } else {
            prg_rom0.clone()
        };

        let n_chr_pages = config.n_chr_rom_pages as u8;
        let chr_bank_select_mask = Mapper3::chr_bank_select_mask(n_chr_pages - 1);
        debug!("Mapper3: CHR Bank Select Mask = {chr_bank_select_mask:08b} ({n_chr_pages} CHR pages)");
        Self {
            vram_mirror: config.nametable_mirror,
            vram: [0u8; 2048],

            has_bus_conflicts: true,

            prg_rom0,
            prg_rom1,

            chr_bank_select_mask,
            chr_data,
            chr_bank: 0,
        }
    }

    fn system_bus_read_direct(&self, addr: u16) -> u8 {
         match addr {
            0x8000..=0xbfff => {
                let off = addr - 0x8000;
                arr_read!(self.prg_rom0, off as usize)
            }
            0xc000..=0xffff => {
                let off = addr - 0xc000;
                arr_read!(self.prg_rom1, off as usize)
            }
            _ => {
                return 0
            }
        }
    }
}

impl Mapper for Mapper3 {
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
            0x8000..=0xffff => { // CHR Bank Select
                let page_select = data & self.chr_bank_select_mask;
                self.chr_bank = PAGE_SIZE_8K * page_select as usize;
            }
            _ => {}
        }
    }

    fn ppu_bus_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x1fff => {
                arr_read!(self.chr_data, self.chr_bank + addr as usize)
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
                arr_write!(self.chr_data, self.chr_bank + addr as usize, data);
            }
            0x2000..=0x3fff => { // VRAM
                arr_write!(self.vram, mirror_vram_address(addr, self.vram_mirror), data);
            }
            _ => {}
        }
    }

    fn mirror_mode(&self) -> NameTableMirror { self.vram_mirror }
    fn irq(&self) -> bool { false }
}