const FIXED_LENGTHS_TABLE: [u8; 32] = [
    10, 254, 20, 2, 40, 4, 80, 6, 160, 8, 60, 10, 14, 12, 26, 14, 12, 16, 24, 18, 48, 20, 96, 22,
    192, 24, 72, 26, 16, 28, 32, 30,
];

#[derive(Clone, Default, Debug)]
pub struct LengthCounter {
    #[allow(dead_code)] // just for Debug
    debug_channel_name: String,
    enabled: bool,
    counter: u8,
    pending_halt: Option<bool>,
    pending_counter_load: Option<u8>,
    pub halt: bool,
}

impl LengthCounter {
    pub fn new(debug_channel_name: String) -> Self {
        LengthCounter {
            debug_channel_name,
            ..Default::default()
        }
    }

    /// Schedule updating the halt flag at the end of the current clock step (after
    /// possibly updating length counters)
    /// NB:
    /// blargg apu 2005: 10.len_halt_timing
    /// ; Changes to length counter halt occur after clocking length, not before.
    pub fn write_halt_flag(&mut self, halt: bool) {
        //println!("{}: queue pending halt = {halt} (current halt = {}, counter = {})", self.debug_channel_name, self.halt, self.counter);
        self.pending_halt = Some(halt);
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        //println!("len counter: set enabled = {enabled}");
        self.enabled = enabled;
        // "When the enabled bit is cleared (via $4015), the length counter is forced to 0 and
        // cannot be changed until enabled is set again (the length counter's previous value
        // is lost). There is no immediate effect when enabled is set."
        if !enabled {
            //println!("{}: set_enabled = {enabled}: Clearing counter", self.debug_channel_name);
            self.counter = 0;
            self.pending_counter_load = None;
        }
    }

    pub fn set_length(&mut self, index: u8) {
        //println!("{}: len counter: (enabled={}), set len, index = {index}, len = {}", self.debug_channel_name, self.enabled, FIXED_LENGTHS_TABLE[index as usize]);
        if self.enabled {
            self.pending_counter_load = Some(FIXED_LENGTHS_TABLE[index as usize]);
        }
    }

    pub fn length(&self) -> u8 {
        self.counter
    }

    pub fn step_half_frame(&mut self) {
        // blargg apu 2005: 11.len_reload_timing
        // ; Write to length counter reload should be ignored when made during length
        // ; counter clocking and the length counter is not zero.
        if self.counter > 0 {
            self.pending_counter_load = None;
        }

        // "When clocked by the frame sequencer, if the halt flag is clear and the counter
        // is non-zero, it is decremented."

        if !self.halt && self.counter > 0 {
            self.counter -= 1;
            //println!("{}: stepped len counter = {}", self.debug_channel_name, self.counter);
        } else {
            //println!("{}: not stepping len counter (halt = {}, counter = {})", self.debug_channel_name, self.halt, self.counter);
        }
        //println!("len counter: step half frame, len = {}", self.counter);
    }

    // blargg apu 2005: 10.len_halt_timing
    // ; Changes to length counter halt occur after clocking length, not before.
    //
    // blargg apu 2005: 11.len_reload_timing
    // ; Write to length counter reload should be ignored when made during length
    // ; counter clocking and the length counter is not zero.
    pub(crate) fn finish_apu_clock_step(&mut self) {
        if let Some(flag) = std::mem::take(&mut self.pending_halt) {
            self.halt = flag;
            //println!("{}: Apply halt flag = {flag}", self.debug_channel_name);
        }

        if let Some(counter) = std::mem::take(&mut self.pending_counter_load) {
            self.counter = counter;
        }
        //if self.pending_halt_delay > 0 {
        //    self.pending_halt_delay -= 1;
        //    if self.pending_halt_delay == 0 {
        //        self.halt = self.pending_halt;
        //    }
        //}
    }
}
