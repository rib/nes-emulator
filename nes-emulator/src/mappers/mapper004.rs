#[allow(unused_imports)]
use log::{error, trace, debug};

use crate::constants::*;
use crate::mappers::Mapper;
use crate::binary::INesConfig;
use crate::prelude::NameTableMirror;

use super::mirror_vram_address;

/// iNES Mapper 004: AKA MMC3
///
/// PRG ROM capacity	512K
/// PRG ROM window	8K + 8K + 16K fixed
/// PRG RAM capacity	8K
/// PRG RAM window	8K
/// CHR capacity	256K
/// CHR window	2Kx2 + 1Kx4
/// Nametable mirroring	H or V, switchable, or 4 fixed
/// Bus conflicts	No
///
/// # Banks
///
/// CPU $6000-$7FFF: 8 KB PRG RAM bank (optional)
/// CPU $8000-$9FFF (or $C000-$DFFF): 8 KB switchable PRG ROM bank
/// CPU $A000-$BFFF: 8 KB switchable PRG ROM bank
/// CPU $C000-$DFFF (or $8000-$9FFF): 8 KB PRG ROM bank, fixed to the second-last bank
/// CPU $E000-$FFFF: 8 KB PRG ROM bank, fixed to the last bank
/// PPU $0000-$07FF (or $1000-$17FF): 2 KB switchable CHR bank
/// PPU $0800-$0FFF (or $1800-$1FFF): 2 KB switchable CHR bank
/// PPU $1000-$13FF (or $0000-$03FF): 1 KB switchable CHR bank
/// PPU $1400-$17FF (or $0400-$07FF): 1 KB switchable CHR bank
/// PPU $1800-$1BFF (or $0800-$0BFF): 1 KB switchable CHR bank
/// PPU $1C00-$1FFF (or $0C00-$0FFF): 1 KB switchable CHR bank
///
#[derive(Clone)]
pub struct Mapper4 {
    vram_mirror: NameTableMirror,
    vram: [u8; 4096], // Enough for 4 full screens
    prg_rom: Vec<u8>,
    prg_ram: Vec<u8>,
    chr_data: Vec<u8>,

    n_prg_pages: usize,
    n_chr_pages: usize,

    register_select: usize,
    swap_prg_banks: bool,
    swap_chr_banks: bool,

    bank_registers: [u8; 8],

    prg_banks: [usize; 4],
    chr_banks: [usize; 8],

    irq_latch: u8,
    pending_reload: bool,
    irq_counter: u8,
    irq_enabled: bool,

    ppu_bus_address: u16,

    /// The clock cycle where A12 was last observed low
    a12_low_clock: Option<u64>,

    irq_raised: bool,

    debug: u64,
}

impl Mapper4 {
    pub fn new(config: &INesConfig, prg_rom: Vec<u8>, chr_data: Vec<u8>) -> Self {

        // We expect the PRG / CHR data to be padded to have a page aligned size
        // when they are loaded
        debug_assert_eq!(prg_rom.len() % PAGE_SIZE_8K, 0);
        debug_assert!(prg_rom.len() >= PAGE_SIZE_16K); // second-to-last 8k page always exists
        debug_assert_eq!(chr_data.len() % PAGE_SIZE_1K, 0);
        // TODO: return an Err for this validation!

        // An appropriate iNes config should mean code will never try to access
        // non-existent pages, but just in case we count the number of pages we
        // have and will wrap out-of-bounds page selections.
        let n_prg_pages = usize::min(64, (prg_rom.len() / PAGE_SIZE_8K));
        let n_chr_pages = usize::min(256, (chr_data.len() / PAGE_SIZE_1K));

        let mut mapper = Self {
            vram_mirror: if config.four_screen_vram { NameTableMirror::FourScreen } else { config.nametable_mirror },
            vram: [0u8; 4096],
            prg_rom,
            prg_ram: vec![0u8; config.n_prg_ram_pages * PAGE_SIZE_16K],
            chr_data,

            n_prg_pages,
            n_chr_pages,

            register_select: 0,
            swap_prg_banks: false,
            swap_chr_banks: false,

            bank_registers: [0; 8],

            prg_banks: [0; 4],
            chr_banks: [0; 8],

            irq_latch: 0,
            pending_reload: false,
            irq_counter: 0,
            irq_enabled: false,

            ppu_bus_address: 0,
            a12_low_clock: None,
            irq_raised: false,

            debug: 0,
        };

        mapper.update_prg_banks();
        mapper.update_chr_banks();

        mapper
    }

    fn update_prg_banks(&mut self) {
        if self.swap_prg_banks {
            self.prg_banks[0] = self.prg_rom.len() - PAGE_SIZE_16K; // second-to-last 8k page
            self.prg_banks[1] = (self.bank_registers[7] as usize) * PAGE_SIZE_8K; // R7
            self.prg_banks[2] = (self.bank_registers[6] as usize) * PAGE_SIZE_8K; // R6
            self.prg_banks[3] = self.prg_rom.len() - PAGE_SIZE_8K; // last 8k page
        } else {            self.prg_banks[0] = (self.bank_registers[6] as usize) * PAGE_SIZE_8K; // R6
            self.prg_banks[1] = (self.bank_registers[7] as usize) * PAGE_SIZE_8K; // R7
            self.prg_banks[2] = self.prg_rom.len() - PAGE_SIZE_16K; // second-to-last 8k page
            self.prg_banks[3] = self.prg_rom.len() - PAGE_SIZE_8K; // last 8k page
        }
    }

    fn update_chr_banks(&mut self) {
        if self.swap_chr_banks {
            self.chr_banks[0] = (self.bank_registers[2] as usize) * PAGE_SIZE_1K;
            self.chr_banks[1] = (self.bank_registers[3] as usize) * PAGE_SIZE_1K;
            self.chr_banks[2] = (self.bank_registers[4] as usize) * PAGE_SIZE_1K;
            self.chr_banks[3] = (self.bank_registers[5] as usize) * PAGE_SIZE_1K;
            self.chr_banks[4] = (self.bank_registers[0] as usize) * PAGE_SIZE_1K;
            self.chr_banks[5] = ((self.bank_registers[0] as usize) * PAGE_SIZE_1K) + PAGE_SIZE_1K;
            self.chr_banks[6] = (self.bank_registers[1] as usize) * PAGE_SIZE_1K;
            self.chr_banks[7] = ((self.bank_registers[1] as usize) * PAGE_SIZE_1K) + PAGE_SIZE_1K;
        } else {            self.chr_banks[0] = (self.bank_registers[0] as usize) * PAGE_SIZE_1K;
            self.chr_banks[1] = ((self.bank_registers[0] as usize) * PAGE_SIZE_1K) + PAGE_SIZE_1K;
            self.chr_banks[2] = (self.bank_registers[1] as usize) * PAGE_SIZE_1K;
            self.chr_banks[3] = ((self.bank_registers[1] as usize) * PAGE_SIZE_1K) + PAGE_SIZE_1K;
            self.chr_banks[4] = (self.bank_registers[2] as usize) * PAGE_SIZE_1K;
            self.chr_banks[5] = (self.bank_registers[3] as usize) * PAGE_SIZE_1K;
            self.chr_banks[6] = (self.bank_registers[4] as usize) * PAGE_SIZE_1K;
            self.chr_banks[7] = (self.bank_registers[5] as usize) * PAGE_SIZE_1K;
        }
    }

    #[inline]
    fn prg_offset_from_address(&self, addr: u16) -> usize {
        match addr {
            0x8000..=0x9fff => addr as usize - 0x8000 + self.prg_banks[0],
            0xa000..=0xbfff => addr as usize - 0xa000 + self.prg_banks[1],
            0xc000..=0xdfff => addr as usize - 0xc000 + self.prg_banks[2],
            0xe000..=0xffff => addr as usize - 0xe000 + self.prg_banks[3],
            _ => unreachable!()
        }
    }

    #[inline]
    fn chr_offset_from_address(&self, addr: u16) -> usize {
        match addr {
            0x0000..=0x03ff => addr as usize - 0x0000 + self.chr_banks[0],
            0x0400..=0x07ff => addr as usize - 0x0400 + self.chr_banks[1],
            0x0800..=0x0bff => addr as usize - 0x0800 + self.chr_banks[2],
            0x0c00..=0x0fff => addr as usize - 0x0c00 + self.chr_banks[3],
            0x1000..=0x13ff => addr as usize - 0x1000 + self.chr_banks[4],
            0x1400..=0x17ff => addr as usize - 0x1400 + self.chr_banks[5],
            0x1800..=0x1bff => addr as usize - 0x1800 + self.chr_banks[6],
            0x1c00..=0x1fff => addr as usize - 0x1c00 + self.chr_banks[7],
            _ => unreachable!()
        }
    }

    /// Read without side effects (doesn't update self.ppu_bus_address)
    fn ppu_bus_read_direct(&mut self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x1fff => {
                let off = self.chr_offset_from_address(addr);
                arr_read!(self.chr_data, off)
            }
            0x2000..=0x3fff => { // VRAM
                let off = mirror_vram_address(addr, self.vram_mirror);
                arr_read!(self.vram, off)
            }
            _ => {
                trace!("Unexpected PPU read via mapper, address = {}", addr);
                0
            }
        }
    }

    fn step_irq(&mut self) {
        // "When the IRQ is clocked (filtered A12 0→1), the counter value is
        // checked - if zero or the reload flag is true, it's reloaded with the
        // IRQ latched value at $C000; otherwise, it decrements."
        if self.irq_counter == 0 || self.pending_reload {
            self.irq_counter = self.irq_latch;
            self.pending_reload = false;
            //println!("Mapper004: IRQ counter reloaded = {}", self.irq_counter);
        } else {
            self.irq_counter -= 1;
        }

        self.update_irq_raised();

        //println!("Mapper004: step IRQ counter = {}, enabled = {}, raised = {}", self.irq_counter, self.irq_enabled, self.irq_raised);
    }

    fn update_irq_raised(&mut self) {
        // "If the IRQ counter is zero and IRQs are enabled ($E001), an IRQ is
        // triggered. The "alternate revision" checks the IRQ counter transition
        // 1→0, whether from decrementing or reloading."
        self.irq_raised = self.irq_counter == 0 && self.irq_enabled;
    }
}

impl Mapper for Mapper4 {
    fn reset(&mut self) {}

    fn clone_mapper(&self) -> Box<dyn Mapper> {
        Box::new(self.clone())
    }

    fn system_bus_read(&mut self, addr: u16) -> (u8, u8) {
        let value = match addr {
            0x6000..=0x7fff => { // 8 KB PRG RAM bank, (optional)
                let ram_offset = (addr - 0x6000) as usize;
                arr_read!(self.prg_ram, ram_offset)
            }
            0x8000..=0xffff => { // PRG ROM Banks
                let off = self.prg_offset_from_address(addr);
                arr_read!(self.prg_rom, off)
            }
            _ => unreachable!()
        };

        (value, 0) // no undefined bits
    }

    fn system_bus_peek(&mut self, addr: u16) -> (u8, u8) {
        self.system_bus_read(addr)
    }

    fn system_bus_write(&mut self, addr: u16, data: u8) {
        match addr {
            0x6000..=0x7fff => { // 8 KB PRG RAM bank, (optional)
                let ram_offset = (addr - 0x6000) as usize;
                arr_write!(self.prg_ram, ram_offset, data);
            }
            0x8000..=0x9fff => {
                if addr & 1 == 0 { // Bank Select
                    self.register_select = (data & 0b111) as usize;
                    let swap_prg_banks = if data & 0b0100_0000 == 0 { false } else { true };
                    let swap_chr_banks = if data & 0b1000_0000 == 0 { false } else { true };

                    if self.swap_prg_banks != swap_prg_banks {
                        self.swap_prg_banks = swap_prg_banks;
                        self.update_prg_banks();
                    }
                    if self.swap_chr_banks != swap_chr_banks {
                        self.swap_chr_banks = swap_chr_banks;
                        self.update_chr_banks();
                    }
                } else { // Bank Data
                    match self.register_select {
                        // "R6 and R7 will ignore the top two bits, as the MMC3 has only 6 PRG ROM address lines"
                        6 | 7 => {
                            self.bank_registers[self.register_select] = (((data & 0b11_1111) as usize) % self.n_prg_pages) as u8;
                            self.update_prg_banks();
                        }
                        // "R0 and R1 ignore the bottom bit, as the value written still counts banks in 1KB units but odd numbered banks can't be selected.""
                        0 | 1 => {
                            self.bank_registers[self.register_select] = (((data & 0b1111_1110) as usize) % self.n_chr_pages) as u8;
                            self.update_chr_banks();
                        }
                        _ => {
                            self.bank_registers[self.register_select] = ((data as usize) % self.n_chr_pages) as u8;
                            self.update_chr_banks();
                        }
                    }
                }
            }
            0xa000..=0xbfff => {
                if addr & 1 == 0 { // Mirroring
                    if self.vram_mirror != NameTableMirror::FourScreen {
                        self.vram_mirror = if data & 1 == 0 { NameTableMirror::Vertical } else { NameTableMirror::Horizontal };
                    }
                } else { // RAM Protect (ignored)
                    // "Though these bits are functional on the MMC3, their main
                    // purpose is to write-protect save RAM during power-off.
                    // Many emulators choose not to implement them as part of
                    // iNES Mapper 4 to avoid an incompatibility with the MMC6."
                }
            }
            0xc000..=0xdfff => {
                if addr & 1 == 0 {
                    // "This register specifies the IRQ counter reload value.
                    // When the IRQ counter is zero (or a reload is requested
                    // through $C001), this value will be copied to the IRQ
                    // counter at the NEXT rising edge of the PPU address,
                    // presumably at PPU cycle 260 of the current scanline."
                    self.irq_latch = data;
                    //println!("Mapper004: IRQ latch counter = {}", self.irq_latch);
                } else {
                    // "Writing any value to this register clears the MMC3 IRQ
                    // counter immediately, and then reloads it at the NEXT
                    // rising edge of the PPU address, presumably at PPU cycle
                    // 260 of the current scanline."
                    self.irq_counter = 0;
                    self.pending_reload = true;
                    //println!("Mapper004: queued IRQ counter reload");
                    //self.update_irq_raised();
                }
            }
            0xe000..=0xffff => {
                if addr & 1 == 0 {
                    //println!("Mapper004: disabled IRQ");
                    self.irq_enabled = false;
                } else {
                    //println!("Mapper004: enabled IRQ");
                    self.irq_enabled = true;
                }
                //self.update_irq_raised();
            }
            _ => unreachable!()
        }
    }

    fn ppu_bus_read(&mut self, addr: u16) -> u8 {
        //println!("Mapper004: read addr = {:04x}", self.ppu_bus_address);
        self.ppu_bus_address = addr;
        self.ppu_bus_read_direct(addr)
    }

    fn ppu_bus_peek(&mut self, addr: u16) -> u8 {
        self.ppu_bus_read_direct(addr)
    }

    fn ppu_bus_write(&mut self, addr: u16, data: u8) {
        self.ppu_bus_address = addr;
        //println!("Mapper004: write addr = {:04x}", self.ppu_bus_address);
        match addr {
            0x0000..=0x1fff => {
                let off = self.chr_offset_from_address(addr);
                arr_write!(self.chr_data, off, data)
            }
            0x2000..=0x3fff => { // VRAM
                let off = mirror_vram_address(addr, self.vram_mirror);
                arr_write!(self.vram, off, data);
            }
            _ => {
                trace!("Unexpected PPU write via mapper, address = {}", addr);
            }
        }
    }

    fn ppu_bus_nop_io(&mut self, addr: u16) {
        self.ppu_bus_address = addr;
        //println!("Mapper004: notify addr = {:04x}", self.ppu_bus_address);
    }

    fn mirror_mode(&self) -> NameTableMirror { self.vram_mirror }

    fn step_m2_phi2(&mut self, cpu_clock: u64) {
        let a12_set = self.ppu_bus_address & (1u16<<12) != 0;
        //println!("Mapper004: step M2: a12 = {a12_set} (addr = {:04x}", self.ppu_bus_address);
        if a12_set {
            if let Some(a12_low_clock) = self.a12_low_clock {
                // "The IRQ timer is ticked after PPU_A12 has been low for 3 falling edges of M2"
                // ref: https://github.com/furrtek/VGChips/tree/master/Nintendo/MMC3C
                //
                // also nesdev:
                // "The MMC3 scanline counter is based entirely on PPU A12,
                // triggered on a rising edge after the line has remained low
                // for three falling edges of M2"
                if cpu_clock - a12_low_clock >= 3 {
                    self.step_irq();
                }

                self.a12_low_clock = None;
            }
        } else {
            if self.a12_low_clock.is_none() {
                self.a12_low_clock = Some(cpu_clock);
            }
        }
    }

    fn irq(&self) -> bool {
        self.irq_raised
    }
}