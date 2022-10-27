#[allow(unused_imports)]
use log::{debug, error, trace};

use anyhow::anyhow;
use anyhow::Result;

use crate::binary::NesBinaryConfig;
use crate::binary::{self, INesConfig, NsfConfig};
use crate::mappers::*;

pub const fn page_offset(page_no: usize, page_size: usize) -> usize {
    page_no * page_size
}

#[derive(Copy, Clone, Debug)]
pub enum TVSystemCompatibility {
    Ntsc,
    Pal,
    Dual,
    Unknown,
}
impl Default for TVSystemCompatibility {
    fn default() -> Self {
        TVSystemCompatibility::Ntsc
    }
}

struct NoCartridge;
impl Mapper for NoCartridge {
    fn reset(&mut self) {}
    fn clone_mapper(&self) -> Box<dyn Mapper> {
        Box::new(NoCartridge)
    }
    fn system_bus_read(&mut self, _addr: u16) -> (u8, u8) {
        (0, 0)
    }
    fn system_bus_peek(&mut self, _addr: u16) -> (u8, u8) {
        (0, 0)
    }
    fn system_bus_write(&mut self, _addr: u16, _data: u8) {}
    fn ppu_bus_read(&mut self, _addr: u16) -> u8 {
        0
    }
    fn ppu_bus_peek(&mut self, _addr: u16) -> u8 {
        0
    }
    fn ppu_bus_write(&mut self, _addr: u16, _data: u8) {}
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum NameTableMirror {
    Unknown,
    Horizontal,
    Vertical,
    SingleScreenA,
    SingleScreenB,
    FourScreen,
}

impl Default for NameTableMirror {
    fn default() -> Self {
        NameTableMirror::Vertical
    }
}

pub struct Cartridge {
    pub config: NesBinaryConfig,
    pub mapper: Box<dyn Mapper>,
}
impl Clone for Cartridge {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            mapper: self.mapper.clone_mapper(),
        }
    }
}
impl Default for Cartridge {
    fn default() -> Self {
        Cartridge::none()
    }
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

        let mapper = Box::new(Mapper31::new(config, &nsf[128..(prg_len as usize)]));
        Ok(Cartridge {
            config: NesBinaryConfig::Nsf(config.clone()),
            mapper,
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
                return Err(anyhow!(
                    "Inconsistent binary size: couldn't read PRG ROM data"
                ));
            }
            prg_rom[0..prg_rom_bytes].copy_from_slice(&ines[ines_start..ines_end]);
        }

        // Load CHR-ROM
        let chr_rom_bytes = config.chr_rom_bytes();
        if chr_rom_bytes > 0 {
            let ines_start = config.chr_rom_baseaddr;
            let ines_end = ines_start + chr_rom_bytes;
            if ines.len() < ines_end {
                return Err(anyhow!(
                    "Inconsistent binary size: couldn't read CHR ROM data"
                ));
            }
            chr_data[0..chr_rom_bytes].copy_from_slice(&ines[ines_start..ines_end]);
        }

        let mut mapper: Box<dyn Mapper> = match config.mapper_number {
            0 => Box::new(Mapper0::new(config, prg_rom, chr_data)),
            1 => Box::new(Mapper1::new(config, prg_rom, chr_data)),
            2 => Box::new(Mapper2::new(config, prg_rom, chr_data)),
            3 => Box::new(Mapper3::new(config, prg_rom, chr_data)),
            4 => Box::new(Mapper4::new(config, prg_rom, chr_data)),
            7 => Box::new(Mapper7::new(config, prg_rom, chr_data)),
            66 => Box::new(Mapper66::new(config, prg_rom, chr_data)),
            _ => {
                return Err(anyhow!(
                    "Unsupported mapper number {}",
                    config.mapper_number
                ));
            }
        };

        if config.has_trainer {
            let ines_start = config.trainer_baseaddr.unwrap();
            let ines_end = ines_start + 512;
            if ines.len() < ines_end {
                return Err(anyhow!(
                    "Inconsistent binary size: couldn't read trainer data"
                ));
            }
            #[allow(clippy::needless_range_loop)] // clippy suggestion is ridiculous!
            for i in ines_start..ines_end {
                mapper.system_bus_write(0x7000 + (i as u16), ines[i]);
            }
        }

        Ok(Cartridge {
            config: NesBinaryConfig::INes(config.clone()),
            mapper,
        })
    }

    pub fn from_binary(binary: &[u8]) -> Result<Cartridge> {
        match binary::parse_any_header(binary)? {
            NesBinaryConfig::INes(ines_config) => {
                Ok(Cartridge::from_ines_binary(&ines_config, binary)?)
            }
            NesBinaryConfig::Nsf(nsf_config) => {
                Ok(Cartridge::from_nsf_binary(&nsf_config, binary)?)
            }
            NesBinaryConfig::None => Err(anyhow!("Unknown binary format"))?,
        }
    }

    pub fn none() -> Cartridge {
        Cartridge {
            config: NesBinaryConfig::None,
            mapper: Box::new(NoCartridge),
        }
    }

    pub(crate) fn power_cycle(&mut self) {
        self.mapper.power_cycle();
    }

    pub(crate) fn reset(&mut self) {
        self.mapper.reset();
    }

    /// What TV system is the cartridge built for
    pub fn tv_system(&self) -> TVSystemCompatibility {
        match &self.config {
            NesBinaryConfig::INes(ines_config) => ines_config.tv_system,
            NesBinaryConfig::Nsf(nsf_config) => nsf_config.tv_system,
            NesBinaryConfig::None => TVSystemCompatibility::Ntsc,
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
        //println!("PPU BUS: read {addr:04x}");

        self.mapper.ppu_bus_read(addr)
    }
    pub fn ppu_bus_peek(&mut self, addr: u16) -> u8 {
        self.mapper.ppu_bus_peek(addr)
    }
    pub fn ppu_bus_write(&mut self, addr: u16, data: u8) {
        //println!("PPU BUS: writing {data} to {addr:04x}");

        self.mapper.ppu_bus_write(addr, data);
    }

    /// An alias for [`Self::ppu_bus_read`]
    ///
    /// Although this just calls through to ppu_bus_read we might integrate
    /// debugging / tracing that will want to easily differentiate VRAM
    /// I/O
    pub fn vram_read(&mut self, addr: u16) -> u8 {
        //println!("VRAM: read {addr:04x}");
        self.mapper.ppu_bus_read(addr)

        //let val = self.mapper.ppu_bus_read(addr);
        //if addr < 0x2400 {
        //    println!("read {val} from {addr}");
        //}
        //val
    }

    /// An alias for [`Self::ppu_bus_peek`]
    ///
    /// Although this just calls through to ppu_bus_peek we might integrate
    /// debugging / tracing that will want to easily differentiate VRAM
    /// I/O
    pub fn vram_peek(&mut self, addr: u16) -> u8 {
        self.mapper.ppu_bus_peek(addr)

        //let val = self.mapper.ppu_bus_peek(addr);
        //if addr < 0x2400 {
        //    println!("read {val} from {addr}");
        //}
        //val
    }

    /// An alias for [`Self::ppu_bus_write`]
    ///
    /// Although this just calls through to ppu_bus_write we might integrate
    /// debugging / tracing that will want to easily differentiate VRAM
    /// I/O
    pub fn vram_write(&mut self, addr: u16, data: u8) {
        //println!("VRAM: writing {data} to {addr:04x}");
        //if addr < 0x2400 {
        //    println!("writing {data} to {addr}");
        //}
        self.mapper.ppu_bus_write(addr, data);
    }

    /// Lets the PPU notify the cartridge of a new address without any I/O required
    ///
    /// As an optimization for being able to update the PPU bus address (such as
    /// when entering vblank or disabling rendering) this simply notifies mappers
    /// of the new address.
    ///
    /// Mappers like MMC3 should use this to track the A12 address bit
    pub fn ppu_bus_nop_io(&mut self, addr: u16) {
        self.mapper.ppu_bus_nop_io(addr);
    }

    /// Steps the m2 (aka phi2 / φ2) wire (i.e. clocked once for each CPU cycle)
    ///
    /// Mappers like MMC3 can use this to filter the A12 bit of the PPU address bus
    /// (which can be tracked via ppu_bus_reads/writes)
    pub fn step_m2_phi2(&mut self, cpu_clock: u64) {
        self.mapper.step_m2_phi2(cpu_clock);
    }
}
