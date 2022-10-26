#[allow(unused_imports)]
use log::{error, trace, debug};

use crate::constants::*;
use crate::mappers::Mapper;
use crate::binary::INesConfig;
use crate::cartridge::NameTableMirror;

use super::mirror_vram_address;

#[derive(Debug, Clone, Copy)]
enum Mapper1PrgMode {

    /// 0x8000-0xdfff switchable to two consecutive pages
    Switch32KConsecutive,

    /// 0x8000 = first page of PRG ROM, 0xc000 page is switchable
    Fixed16KFirstSwitch16K,

    /// 0x8000 page is switchable, 0xc000 = last page of PRG ROM
    Switch16KFixed16KLast
}

#[derive(Debug, Clone, Copy)]
enum Mapper1ChrMode {
    Switch8K,
    Switch4KSwitch4K
}

/// iNes mapper 001, aka MMC1
///
/// PRG ROM capacity	256K (512K)
/// PRG ROM window	16K + 16K fixed or 32K
/// PRG RAM capacity	32K
/// PRG RAM window	8K
/// CHR capacity	128K
/// CHR window	4K + 4K or 8K
/// Nametable mirroring	H, V, or 1, switchable
/// Bus conflicts	No
///
/// # Banks
///
/// CPU $6000-$7FFF: 8 KB PRG RAM bank, (optional)
/// CPU $8000-$BFFF: 16 KB PRG ROM bank, either switchable or fixed to the first bank
/// CPU $C000-$FFFF: 16 KB PRG ROM bank, either fixed to the last bank or switchable
/// PPU $0000-$0FFF: 4 KB switchable CHR bank
/// PPU $1000-$1FFF: 4 KB switchable CHR bank
///
#[derive(Clone)]
pub struct Mapper1 {
    vram_mirror: NameTableMirror,
    vram: [u8; 2048],

    // Load register
    shift_register: u8,
    shift_register_pos: u8,

    prg_bank_mode: Mapper1PrgMode,
    chr_bank_mode: Mapper1ChrMode,

    chr_bank_0: u8,
    chr_bank_1: u8,
    prg_ram_enable: bool,
    prg_bank: u8,

    prg_rom: Vec<u8>,
    prg_rom_last_16k_page: usize,
    prg_ram: Vec<u8>,
    chr_data: Vec<u8>, // may be ROM or RAM
    has_chr_ram: bool,
}

impl Mapper1 {
    pub fn new(config: &INesConfig, prg_rom: Vec<u8>, chr_data: Vec<u8>) -> Self {
        Self {
            vram_mirror: config.nametable_mirror,
            vram: [0u8; 2048],

            shift_register: 0,
            shift_register_pos: 0,

            prg_bank_mode: Mapper1PrgMode::Switch16KFixed16KLast,
            chr_bank_mode: Mapper1ChrMode::Switch8K,

            chr_bank_0: 0,
            chr_bank_1: 0,

            prg_ram_enable: true,
            prg_bank: 0,

            prg_rom,
            prg_rom_last_16k_page: config.n_prg_rom_pages - 1,
            prg_ram: vec![0u8; config.n_prg_ram_pages * PAGE_SIZE_16K],
            chr_data,
            has_chr_ram: config.has_chr_ram
        }
    }

    pub fn load_register_write(&mut self, addr: u16, data: u8) {
        trace!("MMC1: load register write {}", data);

        if data & 0b1000_0000 != 0 {
            self.shift_register = 0;
            self.shift_register_pos = 0;
        } else {
            self.shift_register |= (data & 1) << self.shift_register_pos;
            self.shift_register_pos += 1;

            if self.shift_register_pos == 5 {
                // Only bits 13 and 14 of the address are checked by the HW
                //let addr = addr & 0b0110_0000_0000_0000;
                let value = self.shift_register;

                // Shift register full; write to internal register...
                match addr {
                    0x8000..=0x9fff => { // Control register
                        self.vram_mirror = match value & 0b00011 {
                            0 => NameTableMirror::SingleScreenA,
                            1 => NameTableMirror::SingleScreenB,
                            2 => NameTableMirror::Vertical,
                            3 => NameTableMirror::Horizontal,
                            _ => unreachable!()
                        };
                        self.prg_bank_mode = match (value & 0b01100) >> 2 {
                            0 | 1 => Mapper1PrgMode::Switch32KConsecutive,
                            2 => Mapper1PrgMode::Fixed16KFirstSwitch16K,
                            3 => Mapper1PrgMode::Switch16KFixed16KLast,

                            _ => { unreachable!() } // Rust compiler should know this is unreachable :/
                        };
                        self.chr_bank_mode = match (value & 0b10000) >> 4 {
                            0 => Mapper1ChrMode::Switch8K,
                            1 => Mapper1ChrMode::Switch4KSwitch4K,

                            _ => { unreachable!() } // Rust compiler should know this is unreachable :/
                        };
                        trace!("Control: mirring = {:#?}, PRG mode = {:?}, CHR mode = {:?}",
                                self.vram_mirror, self.prg_bank_mode, self.chr_bank_mode);
                    }
                    0xa000..=0xbfff => { // CHR bank 0
                        trace!("CHR bank 0 = {}", value);
                        self.chr_bank_0 = value;
                    }
                    0xc000..=0xdfff => { // CHR bank 1
                        trace!("CHR bank 1 = {}", value);
                        self.chr_bank_1 = value;
                    }
                    0xe000..=0xffff => { // PRG bank
                        trace!("PRG bank = {}", value);

                        // XXX: 0 = enabled, 1 = disabled!
                        self.prg_ram_enable = (value & 0b10000) != 0b010000;
                        self.prg_bank = value & 0xf;
                    }
                    _ => {
                        trace!("Invalid MMC1 internal register {}", addr);
                    }
                }

                self.shift_register = 0;
                self.shift_register_pos = 0;
            }
        }
    }

    fn chr_bank_0_data_offset(&mut self, offset: usize) -> usize {
        let chr_bank_offset = match self.chr_bank_mode {
            Mapper1ChrMode::Switch8K => {
                // mask out (ignore) bit zero from the bank selector
                let page_no = (self.chr_bank_0 & !1) as usize;
                page_no * PAGE_SIZE_4K
            }
            Mapper1ChrMode::Switch4KSwitch4K => {
                let page_no = self.chr_bank_0 as usize;
                page_no * PAGE_SIZE_4K
            }
        };
        chr_bank_offset + offset
    }

    fn chr_bank_1_data_offset(&mut self, offset: usize) -> usize {
        let chr_bank_offset = match self.chr_bank_mode {
            Mapper1ChrMode::Switch8K => {
                let page_no = (self.chr_bank_0 | 1) as usize;
                page_no * PAGE_SIZE_4K
            }
            Mapper1ChrMode::Switch4KSwitch4K => {
                let page_no = self.chr_bank_1 as usize;
                page_no * PAGE_SIZE_4K
            }
        };
        chr_bank_offset + offset
    }
}

impl Mapper for Mapper1 {
    fn clone_mapper(&self) -> Box<dyn Mapper> {
        Box::new(self.clone())
    }

    fn system_bus_read(&mut self, addr: u16) -> (u8, u8) {
        let value = match addr {
            0x6000..=0x7fff => { // 8 KB PRG RAM bank, (optional)
                let ram_offset = (addr - 0x6000) as usize;
                self.prg_ram[ram_offset]
            }
            0x8000..=0xbfff => { // 16 KB PRG ROM bank, either switchable or fixed to the first bank
                let prg_bank_offset = match self.prg_bank_mode {
                    Mapper1PrgMode::Switch32KConsecutive => {
                        // mask out (ignore) bit zero from the bank selector
                        let page_no = (self.prg_bank & !1) as usize;
                        page_no * PAGE_SIZE_16K
                    }
                    Mapper1PrgMode::Fixed16KFirstSwitch16K => {
                        0
                    }
                    Mapper1PrgMode::Switch16KFixed16KLast => {
                        let page_no = self.prg_bank as usize;
                        page_no * PAGE_SIZE_16K
                    }
                };
                let bank_offset = (addr - 0x8000) as usize;
                arr_read!(self.prg_rom, prg_bank_offset + bank_offset)
            }
            0xc000..=0xffff => { // 16 KB PRG ROM bank, either fixed to the last bank or switchable
                let prg_bank_offset = match self.prg_bank_mode {
                    Mapper1PrgMode::Switch32KConsecutive => {
                        // force odd page_no so it follows on from bank 0
                        let page_no = (self.prg_bank | 1) as usize;
                        page_no * PAGE_SIZE_16K
                    }
                    Mapper1PrgMode::Fixed16KFirstSwitch16K => {
                        let page_no = self.prg_bank as usize;
                        page_no * PAGE_SIZE_16K
                    }
                    Mapper1PrgMode::Switch16KFixed16KLast => {
                        self.prg_rom_last_16k_page * PAGE_SIZE_16K
                    }
                };
                let bank_offset = (addr - 0xc000) as usize;
                arr_read!(self.prg_rom, prg_bank_offset + bank_offset)
            }
            _ => {
                error!("MMC1: invalid system bus read");
                return (0, 0xff)
            }
        };

        (value, 0) // no undefined bits

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
            0x8000..=0xffff => {// Load register
                self.load_register_write(addr, data);
            }
            _ => {
                trace!("MMC1: invalid system bus write");
            }
        }
    }

    fn ppu_bus_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x0fff => {
                let offset = self.chr_bank_0_data_offset(addr as usize);
                arr_read!(self.chr_data, offset)
            }
            0x1000..=0x1fff => {
                let offset = self.chr_bank_1_data_offset((addr - 0x1000) as usize);
                arr_read!(self.chr_data, offset)
            }
            0x2000..=0x3fff => { // VRAM
                arr_read!(self.vram, mirror_vram_address(addr, self.vram_mirror))
            }
            _ => {
                trace!("MMC1: invalid PPU bus read");
                0
            }
        }
    }

    fn ppu_bus_peek(&mut self, addr: u16) -> u8 {
        self.ppu_bus_read(addr)
    }

    fn ppu_bus_write(&mut self, addr: u16, data: u8) {
        match addr {
            0x0000..=0x0fff => {
                if self.has_chr_ram {
                    let offset = self.chr_bank_0_data_offset(addr as usize);
                    arr_write!(self.chr_data, offset, data)
                }
            }
            0x1000..=0x1fff => {
                if self.has_chr_ram {
                    let offset = self.chr_bank_1_data_offset((addr - 0x1000) as usize);
                    arr_write!(self.chr_data, offset, data)
                }
            }
            0x2000..=0x3fff => { // VRAM
                arr_write!(self.vram, mirror_vram_address(addr, self.vram_mirror), data);
            }
            _ => {
                trace!("MMC1: invalid system bus write");
            }
        }
    }

    fn mirror_mode(&self) -> NameTableMirror { self.vram_mirror }
}