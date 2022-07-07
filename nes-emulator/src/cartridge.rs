
#[allow(unused_imports)]
use log::{error, debug, trace};

use anyhow::anyhow;
use anyhow::Result;

use crate::binary::{self, NsfConfig, INesConfig};
use crate::mappers::*;

pub const fn page_offset(page_no: usize, page_size: usize) -> usize {
    page_no * page_size
}

#[derive(Copy, Clone, Debug)]
pub enum TVSystem {
    Ntsc,
    Pal,
    Dual,
    Unknown
}

struct NoCartridge;
impl Mapper for NoCartridge {
    fn reset(&mut self) {}
    fn system_bus_read(&mut self, _addr: u16) -> (u8, u8) { (0, 0) }
    fn system_bus_peek(&mut self, _addr: u16) -> (u8, u8) { (0, 0) }
    fn system_bus_write(&mut self, _addr: u16, _data: u8) { }
    fn ppu_bus_read(&mut self, _addr: u16) -> u8 { 0 }
    fn ppu_bus_peek(&mut self, _addr: u16) -> u8 { 0 }
    fn ppu_bus_write(&mut self, _addr: u16, _data: u8) { }
    fn irq(&self) -> bool { false }
}


#[derive(Copy, Clone, Debug)]
pub enum NameTableMirror {
    Unknown,
    Horizontal,
    Vertical,
    SingleScreen,
    FourScreen,
}

pub struct Cartridge {
    pub mapper: Box<dyn Mapper>,
    pub nametable_mirror: NameTableMirror,
}

impl Cartridge {

    pub fn from_nsf_binary(config: &NsfConfig, nsf: &[u8]) -> Result<Cartridge> {
        if !matches!(binary::check_type(nsf), binary::Type::NSF) {
            return Err(anyhow!("Missing NSF file marker"));
        }
        let prg_len = config.prg_len;
        if nsf.len() < (128 + prg_len as usize) {
            return Err(anyhow!("Inconsistent binary size"));
        }

        println!("NSF Config = {config:#?}");

        let mapper = Box::new(Mapper31::new(&config, &nsf[128..(prg_len as usize)]));
        Ok(Cartridge{
            mapper,
            nametable_mirror: NameTableMirror::Vertical, // Arbitrary
        })
    }

    pub fn from_ines_binary(config: &INesConfig, ines: &[u8]) -> Result<Cartridge> {
        if !matches!(binary::check_type(ines), binary::Type::INES) {
            return Err(anyhow!("Missing iNES file marker"));
        }

        debug!("iNes: Mapper Number {}", config.mapper_number);

        let prg_rom_bytes = config.prg_rom_bytes();
        let mut prg_rom = vec![0u8; prg_rom_bytes];

        let chr_data_bytes = usize::max(config.chr_ram_bytes(), config.chr_rom_bytes());
        let mut chr_data = vec![0u8; chr_data_bytes];

        // Load PRG-ROM
        {
            let ines_start = config.prg_rom_baseaddr;
            let ines_end = ines_start + prg_rom_bytes;
            if ines.len() < ines_end {
                return Err(anyhow!("Inconsistent binary size: couldn't read PRG ROM data"));
            }
            prg_rom[0..prg_rom_bytes].copy_from_slice(&ines[ines_start..ines_end]);
        }

        // Load CHR-ROM
        let chr_rom_bytes = config.chr_rom_bytes();
        if chr_rom_bytes > 0 {
            let ines_start = config.chr_rom_baseaddr;
            let ines_end = ines_start + chr_rom_bytes;
            if ines.len() < ines_end {
                return Err(anyhow!("Inconsistent binary size: couldn't read CHR ROM data"));
            }
            chr_data[0..chr_rom_bytes].copy_from_slice(&ines[ines_start..ines_end]);
        }

        let mut mapper: Box<dyn Mapper> = match config.mapper_number {
            0 => Box::new(Mapper0::new_from_ines(config, prg_rom, chr_data)),
            1 => Box::new(Mapper1::new(config, prg_rom, chr_data)),
            3 => Box::new(Mapper3::new(config, prg_rom, chr_data)),
            4 => Box::new(Mapper4::new(config, prg_rom, chr_data)),
            _ => {
                return Err(anyhow!("Unsupported mapper number {}", config.mapper_number));
            }
        };

        if config.has_trainer {
            let ines_start = config.trainer_baseaddr.unwrap();
            let ines_end = ines_start + 512;
            if ines.len() < ines_end {
                return Err(anyhow!("Inconsistent binary size: couldn't read trainer data"));
            }
            for i in ines_start..ines_end {
                mapper.system_bus_write(0x7000 + (i as u16), ines[i]);
            }
        }

        Ok(Cartridge {
            mapper,
            nametable_mirror: config.nametable_mirror
        })
    }

    pub fn none() -> Cartridge {
        Cartridge {
            mapper: Box::new(NoCartridge),
            nametable_mirror: NameTableMirror::Horizontal
        }
    }

    pub fn system_bus_read(&mut self, addr: u16) -> (u8, u8) {
        self.mapper.system_bus_read(addr)
    }
    pub fn system_bus_peek(&mut self, addr: u16) -> (u8, u8) {
        self.mapper.system_bus_peek(addr)
    }
    pub fn system_bus_write(&mut self, addr: u16, data: u8) {
        self.mapper.system_bus_write(addr, data);
    }

    pub fn ppu_bus_read(&mut self, addr: u16) -> u8 {
        self.mapper.ppu_bus_read(addr)
    }
    pub fn ppu_bus_peek(&mut self, addr: u16) -> u8 {
        self.mapper.ppu_bus_peek(addr)
    }
    pub fn ppu_bus_write(&mut self, addr: u16, data: u8) {
        self.mapper.ppu_bus_write(addr, data);
    }
}