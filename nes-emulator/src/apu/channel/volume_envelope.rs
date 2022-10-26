
#[derive(Clone, Default)]
pub struct VolumeEnvelope {
    debug_channel_name: String,

    start_flag: bool,
    loop_flag: bool,

    /// also used for constant volume
    divider_reload_value: u8,
    divider_counter: u8,

    pub use_constant_volume: bool,

    pub decay_level: u8,
}

impl VolumeEnvelope {
    pub fn new(debug_channel_name: String) -> Self {
        Self {
            debug_channel_name,
            ..Default::default()

            /*
            start_flag: false,
            loop_flag: false,
            divider_reload_value: 0,
            divider_counter: 0,
            use_constant_volume: false,
            decay_level: 0,
            */
        }
    }

    pub fn set_volume(&mut self, vol: u8, constant_volume: bool) {
        assert!(vol < 0x10);

        self.divider_reload_value = vol;
        self.use_constant_volume = constant_volume;
    }

    pub fn restart(&mut self) {
        self.start_flag = true;
    }

    pub fn set_loop_flag(&mut self, loop_flag: bool) {
        self.loop_flag = loop_flag;
    }

    pub(crate) fn step_quarter_frame(&mut self) {
        // When clocked by the frame counter, one of two actions occurs: if the
        // start flag is clear, the divider is clocked, otherwise the start
        // flag is cleared, the decay level counter is loaded with 15, and
        // the divider's period is immediately reloaded.
        if self.start_flag {
            self.start_flag = false;
            self.decay_level = 15;
            self.divider_counter = self.divider_reload_value;
        } else {
            // When the divider is clocked while at 0, it is loaded with V
            // and clocks the decay level counter. Then one of two actions
            // occurs: If the counter is non-zero, it is decremented, otherwise
            // if the loop flag is set, the decay level counter is loaded with 15.
            if self.divider_counter == 0 {
                self.divider_counter = self.divider_reload_value;

                if self.decay_level > 0 {
                    // SAFETY: it cannot wrap around as it only subtracts when
                    // its bigger than 0
                    self.decay_level -= 1;
                } else if self.loop_flag {
                    self.decay_level = 15;
                }
            } else {
                self.divider_counter = self.divider_counter.saturating_sub(1);
            }
        }
    }

    pub fn volume(&mut self) -> u8 {
        if self.use_constant_volume {
            self.divider_reload_value
        } else {
            self.decay_level
        }
    }

}
