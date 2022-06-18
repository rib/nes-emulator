pub trait SystemBus {
    fn read_u8(&mut self, addr: u16) -> u8;
    fn write_u8(&mut self, addr: u16, data: u8);
}

pub trait VideoBus {
    fn read_video_u8(&mut self, addr: u16) -> u8;
    fn write_video_u8(&mut self, addr: u16, data: u8);
}

#[cfg(feature = "unsafe-opt")]
#[allow(unused_macros)]
macro_rules! arr_read {
    ($arr:expr, $index:expr) => {
        unsafe { *$arr.get_unchecked($index) }
    };
}

#[cfg(feature = "unsafe-opt")]
#[allow(unused_macros)]
macro_rules! arr_write {
    ($arr:expr, $index:expr, $data:expr) => {
        unsafe { *$arr.get_unchecked_mut($index) = $data }
    };
}

#[cfg(not(feature = "unsafe-opt"))]
#[allow(unused_macros)]
macro_rules! arr_read {
    ($arr:expr, $index:expr) => {
        debug_assert!($index < $arr.len());
        $arr[$index]
    };
}

#[cfg(not(feature = "unsafe-opt"))]
#[allow(unused_macros)]
macro_rules! arr_write {
    ($arr:expr, $index:expr, $data:expr) => {
        debug_assert!($index < $arr.len());
        $arr[$index] = $data
    };
}
