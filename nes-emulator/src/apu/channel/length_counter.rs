const FIXED_LENGTHS_TABLE: [u8; 32] = [
    10, 254, 20, 2, 40, 4, 80, 6,
    160, 8, 60, 10, 14, 12, 26, 14,
    12, 16, 24, 18, 48, 20, 96, 22,
    192, 24, 72, 26, 16, 28, 32, 30
];

#[derive(Clone, Default)]
pub struct LengthCounter {
    enabled: bool,
    pub counter: u8,
    pub halt: bool,
}

impl LengthCounter {
    pub fn new() -> Self {
        LengthCounter {
            ..Default::default()
        }
    }

    pub fn set_halt(&mut self, halt: bool) {
        self.halt = halt;
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
            self.counter = 0;
        }
    }

    pub fn set_length(&mut self, index: u8) {
        //println!("len counter: (enabled={}), set len, index = {index}, len = {}", self.enabled, FIXED_LENGTHS_TABLE[index as usize]);
        if self.enabled {
            self.counter = FIXED_LENGTHS_TABLE[index as usize];
        }
    }

    pub fn length(&self) ->  u8 {
        self.counter
    }

    pub fn step_half_frame(&mut self) {
        // "When clocked by the frame sequencer, if the halt flag is clear and the counter
        // is non-zero, it is decremented."

        if !self.halt && self.counter > 0 {
            self.counter -= 1;
        }
        //println!("len counter: step half frame, len = {}", self.counter);
    }

}
