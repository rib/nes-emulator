
use log::{error, debug, trace};
use anyhow::anyhow;
use anyhow::Result;

use crate::binary::{self, NsfConfig, INesConfig};
use crate::constants::*;
use super::interface::*;




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

pub enum MapperId {
    None,
    Mapper000,
    Mapper001,
    Mapper031,
}

pub trait Mapper {
    fn id(&self) -> MapperId;

    // FIXME: find a less hacky way of special casing the 031 mapper
    // for NSF playback
    //fn nsf_config(&self) -> Option<NsfConfig>;

    fn reset(&mut self);

    fn system_bus_read_u8(&mut self, addr: u16) -> u8;
    fn system_bus_write_u8(&mut self, addr: u16, data: u8);

    fn ppu_bus_read_u8(&mut self, addr: u16) -> u8;
    fn ppu_bus_write_u8(&mut self, addr: u16, data: u8);
}

struct NoCartridge;
impl Mapper for NoCartridge {
    fn id(&self) -> MapperId { MapperId::None }
    //fn nsf_config(&self) -> Option<NsfConfig> { None }
    fn reset(&mut self) {}
    fn system_bus_read_u8(&mut self, _addr: u16) -> u8 { 0 }
    fn system_bus_write_u8(&mut self, _addr: u16, _data: u8) { }
    fn ppu_bus_read_u8(&mut self, _addr: u16) -> u8 { 0 }
    fn ppu_bus_write_u8(&mut self, _addr: u16, _data: u8) { }
}

struct Mapper0 {
    pub prg_rom: Vec<u8>,
    pub prg_ram: Vec<u8>,
    pub has_chr_ram: bool,
    pub chr_data: Vec<u8>, // may be ROM or RAM
}

impl Mapper0 {
    pub fn new(n_prg_ram_pages: usize, has_chr_ram: bool, prg_rom: Vec<u8>, chr_data: Vec<u8>) -> Mapper0 {
        Mapper0 {
            prg_rom,
            prg_ram: vec![0u8; n_prg_ram_pages * PAGE_SIZE_16K],
            has_chr_ram: has_chr_ram,
            chr_data,
         }
    }
    pub fn new_from_ines(config: &INesConfig, prg_rom: Vec<u8>, chr_data: Vec<u8>) -> Mapper0 {
        Self::new(config.n_prg_ram_pages, config.has_chr_ram, prg_rom, chr_data)
    }
}

impl Mapper for Mapper0 {
    fn id(&self) -> MapperId { MapperId::Mapper000 }
    fn reset(&mut self) {}

    fn system_bus_read_u8(&mut self, addr: u16) -> u8 {
        match addr {
            0x6000..=0x7fff => { // PRG RAM
                let ram_offset = (addr - 0x6000) as usize;
                self.prg_ram[ram_offset]
            }
            0x8000..=0xbfff => { // First 16 KB of ROM
                let addr = (addr - 0x8000) as usize;
                self.prg_rom[addr]
            }
            0xc00..=0xffff => { // Last 16 KB of ROM (NROM-256) or mirror of $8000-$BFFF (NROM-128)
                let addr = (addr - 0x8000) as usize;
                if addr >= self.prg_rom.len() {
                    self.prg_rom[addr - self.prg_rom.len()]
                } else {
                    self.prg_rom[addr]
                }
            }
            _ => {
                trace!("Invalid mapper read @ {}", addr);
                0
            }
        }
    }

    fn system_bus_write_u8(&mut self, addr: u16, data: u8) {
        match addr {
            0x6000..=0x7fff => {
                let ram_offset = (addr - 0x6000) as usize;
                self.prg_ram[ram_offset] = data;
            }
            _ => { trace!("unhandled system bus write in cartridge"); }
        }
    }

    fn ppu_bus_read_u8(&mut self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x1fff => {
                let index = addr as usize;
                arr_read!(self.chr_data, index)
            }
            _ => {
                trace!("Unexpected PPU read via mapper, address = {}", addr);
                0
             }
        }
    }

    fn ppu_bus_write_u8(&mut self, addr: u16, data: u8) {
        if self.has_chr_ram {
            match addr {
                0x0000..=0x1fff => {
                    let index = addr as usize;
                    arr_write!(self.chr_data, index, data);
                },
                _ => {
                    trace!("Unexpected PPU write via mapper, address = {}", addr);
                }
            }
        }
    }
}

#[derive(Debug)]
enum Mapper1PrgMode {

    /// 0x8000-0xdfff switchable to two consecutive pages
    Switch32KConsecutive,

    /// 0x8000 = first page of PRG ROM, 0xc000 page is switchable
    Fixed16KFirstSwitch16K,

    /// 0x8000 page is switchable, 0xc000 = last page of PRG ROM
    Switch16KFixed16KLast
}

#[derive(Debug)]
enum Mapper1ChrMode {
    Switch8K,
    Switch4KSwitch4K
}

struct Mapper1 {

    // Load register
    pub shift_register: u8,
    pub shift_register_pos: u8,

    // Control register
    pub mirroring: u8,

    pub prg_bank_mode: Mapper1PrgMode,
    pub chr_bank_mode: Mapper1ChrMode,

    pub chr_bank_0: u8,
    pub chr_bank_1: u8,
    pub prg_ram_enable: bool,
    pub prg_bank: u8,

    pub prg_rom: Vec<u8>,
    pub prg_rom_last_16k_page: usize,
    pub prg_ram: Vec<u8>,
    pub chr_data: Vec<u8>, // may be ROM or RAM
    pub has_chr_ram: bool,
}

impl Mapper1 {
    pub fn new(config: &INesConfig, prg_rom: Vec<u8>, chr_data: Vec<u8>) -> Mapper1 {
        Mapper1 {
            shift_register: 0,
            shift_register_pos: 0,

            prg_bank_mode: Mapper1PrgMode::Switch16KFixed16KLast,
            chr_bank_mode: Mapper1ChrMode::Switch8K,
            mirroring: 0,

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
                        self.mirroring = value & 0b00011;
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
                        trace!("Control: mirring = {}, PRG mode = {:?}, CHR mode = {:?}",
                                self.mirroring, self.prg_bank_mode, self.chr_bank_mode);
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
    fn id(&self) -> MapperId { MapperId::Mapper001 }
    fn reset(&mut self) {}

    fn system_bus_read_u8(&mut self, addr: u16) -> u8 {
        match addr {
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
                trace!("MMC1: unhandled system bus read");
                0
            }
        }

    }

    fn system_bus_write_u8(&mut self, addr: u16, data: u8) {
        match addr {
            0x6000..=0x7fff => {
                let ram_offset = (addr - 0x6000) as usize;
                arr_write!(self.prg_ram, ram_offset, data);
            }
            0x8000..=0xffff => {// Load register
                self.load_register_write(addr, data);
            }

            _ => {
                trace!("MMC1: unhandled system bus write");
            }
        }
    }

    fn ppu_bus_read_u8(&mut self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x0fff => {
                let offset = self.chr_bank_0_data_offset(addr as usize);
                arr_read!(self.chr_data, offset)
            }
            0x1000..=0x1fff => {
                let offset = self.chr_bank_1_data_offset((addr - 0x1000) as usize);
                arr_read!(self.chr_data, offset)
            }
            _ => {
                trace!("MMC1: unhandled PPU bus read");
                0
            }
        }
    }

    fn ppu_bus_write_u8(&mut self, addr: u16, data: u8) {
        match addr {
            0x0000..=0x0fff => {
                let offset = self.chr_bank_0_data_offset(addr as usize);
                arr_write!(self.chr_data, offset, data)
            }
            0x1000..=0x1fff => {
                let offset = self.chr_bank_1_data_offset((addr - 0x1000) as usize);
                arr_write!(self.chr_data, offset, data)
            }
            _ => {
                trace!("MMC1: unhandled system bus write");
            }
        }
    }
}

struct Mapper031 {
    pub prg_rom: Vec<u8>,
    pub prg_ram: Vec<u8>, // 32k
    pub chr_ram: Vec<u8>, // 8k
    pub prg_bank_offsets: [u8; 8], // 8 x 4k banks
    pub nsf_bios: Vec<u8>,
}


impl Mapper031 {
    pub fn new(config: &NsfConfig, prg_rom_in: &[u8]) -> Mapper031 {
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

        Mapper031 {
            prg_rom,
            prg_ram: vec![0u8; 2 * PAGE_SIZE_16K],
            chr_ram: vec![0u8; 1 * PAGE_SIZE_8K],
            prg_bank_offsets: prg_bank_offsets,
            nsf_bios,
         }

    }
}

impl Mapper for Mapper031 {
    fn id(&self) -> MapperId { MapperId::Mapper031 }
    fn reset(&mut self) {}

    fn system_bus_read_u8(&mut self, addr: u16) -> u8 {
        match addr {
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
        }
    }

    fn system_bus_write_u8(&mut self, addr: u16, data: u8) {
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

    fn ppu_bus_read_u8(&mut self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x1fff => {
                let index = addr as usize;
                arr_read!(self.chr_ram, index)
            }
            _ => {
                trace!("Unexpected PPU read via mapper, address = {}", addr);
                0
             }
        }
    }

    fn ppu_bus_write_u8(&mut self, addr: u16, data: u8) {
        match addr {
            0x0000..=0x1fff => {
                let index = addr as usize;
                arr_write!(self.chr_ram, index, data);
            },
            _ => {
                trace!("Unexpected PPU write via mapper, address = {}", addr);
            }
        }
    }
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

        let mapper = Box::new(Mapper031::new(&config, &nsf[128..(prg_len as usize)]));
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

        let mut chr_data_bytes = usize::max(config.chr_ram_bytes(), config.chr_rom_bytes());
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
            0 => {
                Box::new(Mapper0::new_from_ines(config, prg_rom, chr_data))
            },
            1 => {
                Box::new(Mapper1::new(config, prg_rom, chr_data))
            },
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
                mapper.system_bus_write_u8(0x7000 + (i as u16), ines[i]);
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

}

impl SystemBus for Cartridge {
    fn read_u8(&mut self, addr: u16) -> u8 {
        self.mapper.system_bus_read_u8(addr)
    }
    fn write_u8(&mut self, addr: u16, data: u8) {
        self.mapper.system_bus_write_u8(addr, data);
    }
}
impl VideoBus for Cartridge {
    fn read_video_u8(&mut self, addr: u16) -> u8 {
        self.mapper.ppu_bus_read_u8(addr)
    }
    fn write_video_u8(&mut self, addr: u16, data: u8) {
        self.mapper.ppu_bus_write_u8(addr, data);
    }
}