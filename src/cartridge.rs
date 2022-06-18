use super::interface::*;
use log::{debug, trace};

pub trait Mapper {

    fn system_bus_read_u8(&mut self, addr: u16) -> u8;
    fn system_bus_write_u8(&mut self, addr: u16, data: u8);

    fn ppu_bus_read_u8(&mut self, addr: u16) -> u8;
    fn ppu_bus_write_u8(&mut self, addr: u16, data: u8);
}

enum INesNametableMirroring {
    Vertical,
    Horizontal
}

#[derive(Copy, Clone, Debug)]
enum INesTVSystem {
    Ntsc,
    Pal,
    Dual
}
struct INesConfig {
    mapper_number: u8,
    tv_system: INesTVSystem,
    n_prg_rom_pages: usize,
    n_prg_ram_pages: usize,
    n_chr_data_pages: usize,
    has_chr_ram: bool,
    has_battery: bool,
    has_trainer: bool,
    nametable_mirroring: INesNametableMirroring,
    ignore_mirror_control: bool,
}

struct NoCartridge;
impl Mapper for NoCartridge {
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
    pub fn new(config: &INesConfig, prg_rom: Vec<u8>, chr_data: Vec<u8>) -> Mapper0 {
        Mapper0 {
            prg_rom,
            prg_ram: vec![0u8; config.n_prg_ram_pages * PAGE_SIZE_16K],
            has_chr_ram: config.has_chr_ram,
            chr_data,
         }
    }
}

impl Mapper for Mapper0 {
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

const PAGE_SIZE_2K: usize = 2048;
const PAGE_SIZE_4K: usize = 4096;
const PAGE_SIZE_8K: usize = 8192;
const PAGE_SIZE_16K: usize = 16384;

pub const fn page_offset(page_no: usize, page_size: usize) -> usize {
    page_no * page_size
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

#[derive(Copy, Clone, Debug)]
pub enum NameTableMirror {
    Unknown,
    Horizontal,
    Vertical,
    SingleScreen,
    FourScreen,
}
/// Cassete and mapper implement
/// https://wiki.nesdev.com/w/index.php/List_of_mappers
//#[derive(Clone)]
pub struct Cartridge {
    // Mapperの種類
    pub mapper: Box<dyn Mapper>,
    /// Video領域での0x2000 ~ 0x2effのミラーリング設定
    pub nametable_mirror: NameTableMirror,
    // 0x6000 ~ 0x7fffのカセット内RAMを有効化する
    //pub is_exists_battery_backed_ram: bool,



    // data size
    //pub prg_rom_bytes: usize,
    //pub chr_rom_bytes: usize,
    // datas
    //pub prg_rom: Vec<u8>,
    //pub chr_rom: Vec<u8>,
    //pub battery_packed_ram: Vec<u8>,
}
/*
impl Clone for Cartridge {
    fn clone(&self) -> Self {
        Self {
            mapper: self.mapper.clone(),
            nametable_mirror: self.nametable_mirror.clone(),
            is_exists_battery_backed_ram: self.is_exists_battery_backed_ram.clone(),
            prg_rom_bytes: self.prg_rom_bytes.clone(),
            chr_rom_bytes: self.chr_rom_bytes.clone(),
            prg_rom: self.prg_rom.clone(),
            chr_rom: self.chr_rom.clone(),
            battery_packed_ram: self.battery_packed_ram.clone() }
    }
}
*/

/*
impl Default for Cartridge {
    fn default() -> Self {
        Self {
            mapper: Mapper::Unknown,
            nametable_mirror: NameTableMirror::Unknown,
            is_exists_battery_backed_ram: false,

            prg_rom_bytes: 0,
            chr_rom_bytes: 0,

            prg_rom: vec![],
            chr_rom: vec![],
            battery_packed_ram: vec![],
        }
    }
}
*/

impl Cartridge {
    /// inesファイルから読み出してメモリ上に展開します
    /// 組み込み環境でRAM展開されていなくても利用できるように、多少パフォーマンスを犠牲にしてもclosure経由で読み出します
    /// Read from ines file and extract to memory
    /// Read via closure at the expense of some performance so that it can be used in an embedded
    /// environment without RAM expansion
    pub fn from_ines_binary(read_func: impl Fn(usize) -> u8) -> Option<Cartridge> {
        // header : 16byte
        // trainer: 0 or 512byte
        // prg rom: prg_rom_size * 16KB(0x4000)
        // chr rom: prg_rom_size * 8KB(0x2000)
        // playchoise inst-rom: 0 or 8192byte(8KB)
        // playchoise prom: 16byte

        debug!("Parsing iNes header...");

        // header check
        if read_func(0) != 0x4e {
            // N
            return None;
        }
        if read_func(1) != 0x45 {
            // E
            return None;
        }
        if read_func(2) != 0x53 {
            // S
            return None;
        }
        if read_func(3) != 0x1a {
            // character break
            return None;
        }

        let mut has_chr_ram = false;
        let n_prg_rom_pages = usize::from(read_func(4)); // * 16KBしてあげる
        debug!("iNes: {} PRG ROM pages", n_prg_rom_pages);
        let n_chr_rom_pages = usize::from(read_func(5)); // * 8KBしてあげる
        debug!("iNes: {} CHR ROM pages", n_chr_rom_pages);
        let mut n_chr_data_pages = n_chr_rom_pages;
        if n_chr_data_pages == 0 {
            has_chr_ram = true;
            n_chr_data_pages = 1;
        }
        let n_prg_ram_pages = 2; // Need iNes 2.0 to configure properly

        let flags6 = read_func(6);
        let flags7 = read_func(7);
        let _flags8 = read_func(8);
        let _flags9 = read_func(9);
        let flags10 = read_func(10);
        let tv_system = match flags10 & 0b11 {
            0 => INesTVSystem::Ntsc,
            2 => INesTVSystem::Pal,
            1 | 3 => INesTVSystem::Dual,

            _ => { unreachable!() } // Rust compiler should know this is unreachable :/
        };
        debug!("iNes: TV System {:?}", tv_system);
        // 11~15 unused_padding
        debug_assert!(n_prg_rom_pages > 0);

        // flags parsing
        let is_mirroring_vertical = (flags6 & 0x01) == 0x01;

        // FIXME: consolidate these seperate enums...
        let nametable_mirroring = if is_mirroring_vertical {
            INesNametableMirroring::Vertical
        } else {
            INesNametableMirroring::Horizontal
        };
        let nametable_mirror = if is_mirroring_vertical {
            NameTableMirror::Vertical
        } else {
            NameTableMirror::Horizontal
        };
        debug!("iNes: Mirroring {:?}", nametable_mirror);

        let has_battery = (flags6 & 0x02) == 0x02; // 0x6000 - 0x7fffのRAMを使わせる
        debug!("iNes: Has Battery {}", has_battery);
        let has_trainer = (flags6 & 0x04) == 0x04; // 512byte trainer at 0x7000-0x71ff in ines file
        debug!("iNes: Has Trainer {}", has_trainer);

        // 領域計算
        let header_bytes = 16;
        let trainer_bytes = if has_trainer { 512 } else { 0 };
        let prg_rom_bytes = n_prg_rom_pages * PAGE_SIZE_16K;
        let chr_rom_bytes = n_chr_rom_pages * PAGE_SIZE_8K;

        let _trainer_baseaddr = header_bytes;
        let prg_rom_baseaddr = header_bytes + trainer_bytes;
        let chr_rom_baseaddr = header_bytes + trainer_bytes + prg_rom_bytes;

        let mut mapper_number: u8 = 0;
        let low_nibble = (flags6 & 0b11110000) >> 4;
        mapper_number |= low_nibble;
        let high_nibble = flags7 & 0xF0;
        mapper_number |= high_nibble;

        debug!("iNes: Mapper Number {}", mapper_number);

        // Ignore the pre-allocated buffers and lets just allocate
        // vectors dynamically instead (will probably break the
        // embedding functionally)
        let mut prg_rom = vec![0u8; n_prg_rom_pages * PAGE_SIZE_16K];
        let mut chr_data = vec![0u8; n_chr_data_pages * PAGE_SIZE_8K];

        // PRG-ROM
        for index in 0..prg_rom_bytes {
            let ines_binary_addr = prg_rom_baseaddr + index;
            let byte = read_func(ines_binary_addr);
            prg_rom[index] = byte;
        }

        // CHR-ROM
        if !has_chr_ram {
            for index in 0..chr_rom_bytes {
                let ines_binary_addr = chr_rom_baseaddr + index;
                let byte = read_func(ines_binary_addr);
                chr_data[index] = byte;
            }
        }

        let ines_config = INesConfig {
            mapper_number,
            tv_system,
            n_prg_rom_pages,
            n_prg_ram_pages,
            n_chr_data_pages,
            nametable_mirroring,
            ignore_mirror_control: false, // FIXME
            has_battery,
            has_chr_ram,
            has_trainer
        };

        let mapper: Box<dyn Mapper> = match mapper_number {
            0 => {
                Box::new(Mapper0::new(&ines_config, prg_rom, chr_data))
            },
            1 => {
                Box::new(Mapper1::new(&ines_config, prg_rom, chr_data))
            },
            _ => {
                unreachable!();
                Box::new(NoCartridge)
            }
        };
        //debug_assert!(self.mapper != Mapper::Unknown);

        /* FIXME: add trainer support back later if necessary
        // Battery Packed RAMの初期値
        if is_exists_trainer {
            // 0x7000 - 0x71ffに展開する
            for index in 0..INES_TRAINER_DATA_SIZE {
                let ines_binary_addr = trainer_baseaddr + index;
                self.prg_rom[index] = read_func(ines_binary_addr);
            }
        }
        */

        // rom sizeをセットしとく
        // Set the rom size
        //self.prg_rom_bytes = prg_rom_bytes;
        //self.chr_rom_bytes = chr_rom_bytes;

        // やったね
        Some(Cartridge {
            mapper,
            nametable_mirror
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
    /// CHR_RAM対応も込めて書き換え可能にしておく
    fn write_video_u8(&mut self, addr: u16, data: u8) {
        self.mapper.ppu_bus_write_u8(addr, data);
    }
}