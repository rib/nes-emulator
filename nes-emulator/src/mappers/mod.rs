pub trait Mapper {
    fn reset(&mut self);
    fn clone_mapper(&self) -> Box<dyn Mapper>;

    // Returns (value, undefined_bits)
    fn system_bus_read(&mut self, addr: u16) -> (u8, u8);
    fn system_bus_peek(&mut self, addr: u16) -> (u8, u8);
    fn system_bus_write(&mut self, addr: u16, data: u8);

    fn ppu_bus_read(&mut self, addr: u16) -> u8;
    fn ppu_bus_peek(&mut self, addr: u16) -> u8;
    fn ppu_bus_write(&mut self, addr: u16, data: u8);

    fn mirror_mode(&self) -> NameTableMirror;
    fn irq(&self) -> bool;
}

#[inline]
pub fn mirror_vram_address(mut addr: u16, mode: NameTableMirror) -> usize {
    debug_assert!(addr >= 0x2000 && addr < 0x4000);

    let save = addr;

    // NB: each Nametable (+attribute table) = 1024 bytes: 960 + 64
    //
    // There is typically just 2k of VRAM which is made up into four logical
    // nametables with mirroring.
    //
    // The PPU address space then mirrors this further across 8k (but the
    // latter 4k mirror is overlayed with the pallets)

    addr %= 4096;

    let off = match mode {
        NameTableMirror::Horizontal => {
            match addr {
                0..=1023 => { addr }, // Top left
                1024..=2047 => { addr - 1024}, // Top right
                2048..=3071 => { addr - 2048 + 1024 }, // Bottom left
                3072..=4095 => { addr - 3072 + 1024 }, // Bottom right
                _ => unreachable!()
            }
        }
        NameTableMirror::Vertical => {
            match addr {
                0..=1023 => {
                    //println!("mirroring 0x{save:x} to 'A' (0..1024) = {addr} (NOP)");
                    addr
                }, // Top left
                1024..=2047 => {
                    //println!("mirroring 0x{save:x} to 'B' (1024..2048) = {addr} (NOP)");
                    addr
                }, // Top right
                2048..=3071 => {
                    let off = addr - 2048;
                    //println!("mirroring 0x{save:x} to 'A' (0-1024) = {off}");
                    off
                }, // Bottom left
                3072..=4095 => {
                    let off = addr - 3072 + 1024;
                    //println!("mirroring 0x{save:x} to 'B' (1024..2048) = {off}");
                    off
                }, // Bottom right
                _ => unreachable!()
            }
        }
        NameTableMirror::SingleScreen => {
            match addr {
                0..=1023 => { addr }, // Top left
                1024..=2047 => { addr - 1024 }, // Top right
                2048..=3071 => { addr - 2048 }, // Bottom left
                3072..=4095 => { addr - 3072 }, // Bottom right
                _ => unreachable!()
            }
        }
        NameTableMirror::FourScreen => {
            addr - 0x2000
        }
        _ => panic!("Unknown mirror mode")
    };

    //println!("Mirrored {save:x} to vram[{off}] ({mode:?}");
    off as usize
}

pub mod mapper000;
pub use mapper000::Mapper0;

pub mod mapper001;
pub use mapper001::Mapper1;

pub mod mapper003;
pub use mapper003::Mapper3;

pub mod mapper004;
pub use mapper004::Mapper4;

pub mod mapper031;
pub use mapper031::Mapper31;

use crate::prelude::NameTableMirror;