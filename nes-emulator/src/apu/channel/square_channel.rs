use crate::apu::channel::length_counter::LengthCounter;
use crate::apu::channel::volume_envelope::VolumeEnvelope;
use crate::system::Model;
use super::frame_sequencer::FrameSequencerStatus;

// Ref: https://www.nesdev.com/apu_ref.txt
// Note: we step through the DUTY_MAP with an _incrementing_ counter, not decrementing
const DUTY_MAP: [[u8; 8]; 4] = [
    [0, 1, 0, 0, 0, 0, 0, 0], // 12.5%
    [0, 1, 1, 0, 0, 0, 0, 0], // 25%
    [0, 1, 1, 1, 1, 0, 0, 0], // 50%
    [1, 0, 0, 1, 1, 1, 1, 1], // 25% negated
];

#[derive(Clone, Default)]
pub struct SquareChannel {
    model: Model,
    debug_channel_name: String,

    // We don't currently encapsulate the sweep state since it interacts
    // with the channel state
    pub sweep_enabled: bool,
    pub sweep_negate: bool,
    pub sweep_shift: u8,
    twos_compliment_sweep_negate: bool,
    pub sweep_divider_period: u8,
    pub sweep_divider_value: u8,
    sweep_target_period: u16,
    sweep_reload_flag: bool,

    pub timer_period: u16,
    pub timer: u16,

    pub duty: u8,
    pub duty_offset: u8,

    volume_envelope: VolumeEnvelope,
    pub length_counter: LengthCounter,

    pub output: u8
}

impl SquareChannel {
    pub fn new(model: Model, debug_channel_name: String, twos_compliment_sweep_negate: bool) -> Self {

        Self {
            model,
            debug_channel_name: debug_channel_name.clone(),
            twos_compliment_sweep_negate,
            volume_envelope: VolumeEnvelope::new(debug_channel_name.clone()),
            length_counter: LengthCounter::new(debug_channel_name),
            ..Default::default()
        }
    }

    pub fn power_cycle(&mut self) {
        *self = Self::new(self.model, self.debug_channel_name.clone(), self.twos_compliment_sweep_negate);
    }

    /// "Two conditions cause the sweep unit to mute the channel:
    ///  1. If the current period is less than 8, the sweep unit mutes the channel.
    ///  2. If at any time the target period is greater than $7FF, the sweep unit mutes the channel."
    pub fn is_muted(&self) -> bool {
        //if self.timer < 8 {
        //    println!("muted: timer = {}", self.timer);
        //}
        //if self.sweep_target_period > 0x7ff {
        //    println!("muted sweep_target_period = {}", self.sweep_target_period);
        //}
        self.timer_period < 8 || self.sweep_target_period > 0x7ff
    }

    // "The shifter continuously calculates a result based on the channel's period. The
    // channel's period (from the third and fourth registers) is first shifted right
    // by s bits. If negate is set, the shifted value's bits are inverted, and on the
    // second square channel, the inverted value is incremented by 1. The resulting
    // value is added with the channel's current period, yielding the final result."
    pub fn update_sweep_target_period(&mut self) {
        let delta = self.timer_period >> self.sweep_shift;

        self.sweep_target_period = if self.sweep_negate {
            if self.twos_compliment_sweep_negate {
                self.timer_period.saturating_sub(delta)
            } else {
                self.timer_period.saturating_sub(delta).saturating_sub(1)
            }
        } else {
            self.timer_period.saturating_add(delta)
        }
    }

    pub fn frequency(&self) -> u32 {
        self.model.cpu_clock_hz() / (16*(self.timer_period as u32 + 1))
    }

    pub fn set_frequency(&mut self, freq: u32) {
        self.timer_period = ((self.model.cpu_clock_hz() / (16 * freq) - 1) as u16) & 0b0111_1111_1111;
    }

    pub fn period(&self) -> u16 {
        self.timer_period
    }

    pub fn set_period(&mut self, period: u16) {
        //println!("square set period = {}", period);
        self.timer_period = period;
        self.update_sweep_target_period();
    }

    pub fn timer(&self) -> u16 {
        self.timer
    }

    pub fn set_timer(&mut self, timer: u16) {
        self.timer = timer.min(self.timer_period);
    }

    fn step_sweep_half_frame(&mut self) {

        //println!("square channel: half frame");

        // "1. If the divider's counter is zero, the sweep is enabled, and the sweep unit is not
        // muting the channel: The pulse's period is set to the target period."
        if self.sweep_divider_value == 0 &&  self.sweep_enabled && !self.is_muted() {

            // "If the shift count is zero, the pulse channel's period is never updated, but
            // muting logic still applies."
            if self.sweep_shift != 0 {
                //println!("Updating period by sweep: from {}, to {}", self.timer_period, self.sweep_target_period);
                self.set_period(self.sweep_target_period);
            }
        }// else {
        //    println!("didn't update current period: sweep div = {}, sweep_enabled = {}, muted = {}, sweep_shift = {}",
        //            self.sweep_divider_value, self.sweep_enabled, self.is_muted(), self.sweep_shift);
        //}

        // "2. If the divider's counter is zero or the reload flag is true: The divider counter
        // is set to P and the reload flag is cleared. Otherwise, the divider counter is decremented."
        if self.sweep_divider_value == 0 || self.sweep_reload_flag {
            self.sweep_reload_flag = false;
            self.sweep_divider_value = self.sweep_divider_period;
        } else {
            self.sweep_divider_value -= 1;
        }
    }

    pub fn update_output(&mut self) {
        //println!("updating square channel output");
        if self.is_muted() || self.length() == 0 {
            //if self.is_muted() {
            //    println!("square channel muted");
            //}
            //if self.length() == 0 {
            //    println!("square channel len = 0");
            //}
            self.output = 0;
        } else {
            //println!("square wave volume decay = {}", self.volume_envelope.decay_level);
            let volume = self.volume_envelope.volume();
            self.output = DUTY_MAP[self.duty as usize][self.duty_offset as usize] * volume;
            //println!("square channel output = {}", self.output);
        }
    }

    pub fn output(&self) -> u8 {
        self.output
    }

    // Only stepped for odd CPU clock cycles
    pub fn odd_step(&mut self, sequencer_state: FrameSequencerStatus) {

        if sequencer_state.contains(FrameSequencerStatus::QUARTER_FRAME) {
            self.volume_envelope.step_quarter_frame();
        }
        if sequencer_state.contains(FrameSequencerStatus::HALF_FRAME) {
            //println!("Square half frame step");
            self.step_sweep_half_frame();
            self.length_counter.step_half_frame();
            //println!("square: half frame: length = {}", self.length());
        }

        if self.timer == 0 {
            //println!("square, odd_step set timer = {}", self.timer_period);
            self.timer = self.timer_period;
            self.duty_offset  = (self.duty_offset + 1) % 8;
        } else {
            self.timer -= 1;
        }
        //println!("square: odd_step: timer = {}", self.timer);

        self.update_output();
    }

    pub fn length(&self) -> u8 {
        self.length_counter.length()
    }

    pub fn write(&mut self, address: u16, value: u8) {
        match address % 4 {
            0 => {
                // "The duty cycle is changed, but the sequencer's current position isn't affected."
                self.duty = value >> 6;

                let len_halt = (value & 0b0010_0000) != 0;

                self.length_counter.write_halt_flag(len_halt);
                self.volume_envelope.set_loop_flag(len_halt); // Dual-purpose flag

                let constant_volume = (value & 0b0001_0000) != 0;
                let envelope_volume = value & 0xf;
                self.volume_envelope.set_volume(envelope_volume, constant_volume)
            }
            1 => { // Sweep
                // Ref: https://www.nesdev.org/wiki/APU_Sweep
                self.sweep_enabled = (value & 0b1000_0000) != 0;
                //println!("square set sweep enable = {}", self.sweep_enabled);

                self.sweep_divider_period = ((value & 0b0111_0000) >> 4) + 1; // (period measured in half frames)
                self.sweep_negate = (value & 0b1000) != 0;
                self.sweep_shift = value & 0b111;
                //println!("square set sweep_shift = {}", self.sweep_shift);

                self.sweep_reload_flag = true;
            }
            2 => {
                self.set_period((self.timer_period & 0b0000_0111_0000_0000) | (value as u16));
            }
            3 => {
                //println!("{}: $4003 write: value = {value:x} / {value:08b}", self.debug_channel_name);
                self.length_counter.set_length(value >> 3);
                //println!("{}: length counter = {}", self.debug_channel_name, self.length_counter.length());

                let timer_high = ((value as u16) & 0b111) << 8;
                self.set_period((self.timer_period & 0xff) | timer_high);

                // "The sequencer is immediately restarted at the first value of the current sequence"
                // Note: "The period divider is not reset."
                self.duty = 0;

                // "The envelope is also restarted"
                self.volume_envelope.restart();
            }
            _ => unreachable!()
        }
    }
}

