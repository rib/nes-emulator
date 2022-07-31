#[allow(unused_imports)]
use log::{trace, debug};

use crate::constants::*;
use crate::mappers::Mapper;
use crate::binary::NsfConfig;
use crate::cartridge::NameTableMirror;

use super::mirror_vram_address;

#[derive(Clone)]
pub struct Mapper31 {
    pub vram: [u8; 2048],
    pub prg_rom: Vec<u8>,
    pub prg_ram: Vec<u8>, // 32k
    pub chr_ram: Vec<u8>, // 8k
    pub prg_bank_offsets: [u8; 8], // 8 x 4k banks
    pub nsf_bios: Vec<u8>,
}

impl Mapper31 {
    pub fn new(config: &NsfConfig, prg_rom_in: &[u8]) -> Mapper31 {
        let padding = if config.is_bank_switched {
            (config.load_address & 0xfff) as usize
        } else {
            (config.load_address - 0x8000) as usize
        };

        println!("NSF padding = {padding:x}, load address = {:x}", config.load_address);

        let padded_prg_rom_len = prg_rom_in.len() + padding;
        // Ensure we have at least 32k to cover 0x8000-0x7fff in the
        // unbanked case
        let padded_prg_rom_len = usize::max(padded_prg_rom_len, PAGE_SIZE_16K * 2);

        let mut prg_rom = vec![0u8; padded_prg_rom_len];
        prg_rom[padding..(padding + prg_rom_in.len())].copy_from_slice(prg_rom_in);

        let prg_bank_offsets = if config.is_bank_switched {
            config.banks
        } else {
            [0, 1, 2, 3, 4, 5, 6, 7]
        };

        let nsf_bios = include_bytes!("nsf-bios.bin");
        let nsf_bios = nsf_bios.to_vec();
        println!("NSF BIOS len = {}", nsf_bios.len());
        println!("NSF BIOS = {nsf_bios:x?}");

        Mapper31 {
            vram: [0u8; 2048],
            prg_rom,
            prg_ram: vec![0u8; 2 * PAGE_SIZE_16K],
            chr_ram: vec![0u8; 1 * PAGE_SIZE_8K],
            prg_bank_offsets: prg_bank_offsets,
            nsf_bios,
         }

    }
}

impl Mapper for Mapper31 {
    fn clone_mapper(&self) -> Box<dyn Mapper> {
        Box::new(self.clone())
    }

    fn system_bus_read(&mut self, addr: u16) -> (u8, u8) {
        let value = match addr {
            // Unused memory region according to https://www.nesdev.org/wiki/NSF
            // Used to store a minimal 'bios' that can bootstrap NSF playback.
            0x5000..=0x5200 => {
                let offset = addr - 0x5000;
                //println!("bios read {offset:x} = {:x}", self.nsf_bios[offset as usize]);
                self.nsf_bios[offset as usize]
            }
            0x6000..=0x7fff => { // PRG RAM
                let ram_offset = (addr - 0x6000) as usize;
                self.prg_ram[ram_offset]
            }
            0x8000..=0xffff => { // 8 x 4k bank switched rom
                let addr = (addr - 0x8000) as usize;
                let bank_index = (addr & 0b0111_0000_0000_0000) >> 12;

                let bank_offset = self.prg_bank_offsets[bank_index];
                let bank_offset = PAGE_SIZE_4K * bank_offset as usize;
                let page_offset = addr & 0xfff;
                let rom_addr = bank_offset + page_offset;

                self.prg_rom[rom_addr]
            }
            _ => {
                trace!("Invalid mapper read @ {}", addr);
                0
            }
        };

        (value, 0) // no undefined bits
    }

    fn system_bus_peek(&mut self, addr: u16) -> (u8, u8) {
        self.system_bus_read(addr)
    }

    fn system_bus_write(&mut self, addr: u16, data: u8) {
        match addr {
            // Unused memory region according to https://www.nesdev.org/wiki/NSF
            // Used to store a minimal 'bios' that can bootstrap NSF playback.
            0x5000..=0x5200 => {
                let offset = addr - 0x5000;

                //panic!("bios write");
                self.nsf_bios[offset as usize] = data;
            }
            0x6000..=0x7fff => {
                let ram_offset = (addr - 0x6000) as usize;
                self.prg_ram[ram_offset] = data;
            }
            0x5000..=0x5fff => {
                let bank = addr & 0b111;
                self.prg_bank_offsets[bank as usize] = data;
            }
            _ => { trace!("unhandled system bus write in cartridge"); }
        }
    }

    fn ppu_bus_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x1fff => {
                let index = addr as usize;
                arr_read!(self.chr_ram, index)
            }
            0x2000..=0x3fff => { // VRAM
                arr_read!(self.vram, mirror_vram_address(addr, NameTableMirror::Vertical))
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
                let index = addr as usize;
                arr_write!(self.chr_ram, index, data);
            },
            0x2000..=0x3fff => { // VRAM
                arr_write!(self.vram, mirror_vram_address(addr, NameTableMirror::Vertical), data);
            }
            _ => {
                trace!("Unexpected PPU write via mapper, address = {}", addr);
            }
        }
    }
}