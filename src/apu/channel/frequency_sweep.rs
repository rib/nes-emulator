use super::frame_sequencer::FrameSequencerStatus;

pub struct FrequencySweep {
    sweep_divider_period: i8,
    period_load: u8,
    sweep_period_counter: u8,
    negate: bool,
    shift: u8,
    enabled: bool,

    /// Two conditions cause the sweep unit to mute the channel:
    /// 1. If the current period is less than 8, the sweep unit mutes the channel.
    /// 2. If at any time the target period is greater than $7FF, the sweep unit mutes the channel.
    /// (this is separate from the explicit `enable` flag)
    muting: bool,

    reload_flag: bool,
    twos_compliment_negate: bool,
}



impl FrequencySweep {
    pub fn new(twos_compliment_negate: bool) -> Self {
        FrequencySweep {
            sweep_divider_period: 0,
            period_load: 0,
            sweep_period_counter: 0,
            negate: false,
            shift: 0,
            enabled: false,
            muting: false,
            reload_flag: false,
            twos_compliment_negate
        }
    }

    // "The shifter continuously calculates a result based on the channel's period. The
    // channel's period (from the third and fourth registers) is first shifted right
    // by s bits. If negate is set, the shifted value's bits are inverted, and on the
    // second square channel, the inverted value is incremented by 1. The resulting
    // value is added with the channel's current period, yielding the final result."
    pub fn target_period(&self, channel_period: u16) -> u16 {
        let mut delta = channel_period >> self.shift;

        if self.negate {
            if self.twos_compliment_negate {
                channel_period.saturating_sub(delta)
            } else {
                channel_period.saturating_sub(delta).saturating_sub(1)
            }
        } else {
            channel_period.saturating_add(delta)
        }
    }

    pub fn write_register(&mut self, value: u8) {
        // Ref: https://www.nesdev.org/wiki/APU_Sweep
        self.enabled = (value & 0b1000_000) != 0;

        self.period_load = ((value & 0b0111_0000) >> 4) + 1; // (period measured in half frames)
        self.negate = (value & 0b1000) != 0;
        self.shift = value & 0b111;

        self.reload_flag = true;
    }

    pub fn step(&mut self, sequencer_state: FrameSequencerStatus) -> FrequencySweepResult {
        if !self.enabled {
            return FrequencySweepResult::None;
        }

        // "When the sweep unit is clocked, the divider is *first* clocked and then if
        // there was a write to the sweep register since the last sweep clock, the divider
        // is reset.""

        match sequencer_state {
            FrameSequencerStatus::HalfFrame => {

            }
        }


        self.sweep_divider_period -= 1;

        if self.sweep_divider_period <= 0 {
            self.sweep_divider_period = self.period_load as i8;
            if self.sweep_divider_period == 0 {
                return FrequencySweepResult::None;
            }

            if !self.calculate_frequency() {
                return FrequencySweepResult::Overflowed;
            }

            return FrequencySweepResult::Sweeped(self.frequency);
        }

        FrequencySweepResult::None
    }

    pub fn trigger(&mut self, frequency: u16) -> FrequencySweepResult {
        self.frequency = frequency;
        self.sweep_period_counter = 0;

        if self.shift > 0 && self.period_load > 0 {
            self.sweep_divider_period = self.period_load as i8;
            self.enabled = true;
        } else {
            self.enabled = false;
        }

        FrequencySweepResult::None
    }
}
