/// hacky constant just to make CPU traces comparable with Mesen trace logs
pub(crate) const CPU_START_CYCLE: u64 = 6;

pub const NTSC_CPU_CLOCK_HZ: u32 = 1_789_166;
pub const PAL_CPU_CLOCK_HZ: u32 = 1_662_607;

pub const PAGE_SIZE_1K: usize = 1024;
pub const PAGE_SIZE_2K: usize = 2048;
pub const PAGE_SIZE_4K: usize = 4096;
pub const PAGE_SIZE_8K: usize = 8192;
pub const PAGE_SIZE_16K: usize = 16384;
pub const PAGE_SIZE_32K: usize = 32768;

pub const FRAME_WIDTH: usize = 256;
pub const FRAME_HEIGHT: usize = 240;
