pub trait Mapper {
    fn reset(&mut self);

    // Returns (value, undefined_bits)
    fn system_bus_read(&mut self, addr: u16) -> (u8, u8);
    fn system_bus_peek(&mut self, addr: u16) -> (u8, u8);
    fn system_bus_write(&mut self, addr: u16, data: u8);

    fn ppu_bus_read(&mut self, addr: u16) -> u8;
    fn ppu_bus_peek(&mut self, addr: u16) -> u8;
    fn ppu_bus_write(&mut self, addr: u16, data: u8);

    fn irq(&self) -> bool;
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