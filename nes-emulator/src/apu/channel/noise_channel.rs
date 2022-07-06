use crate::apu::channel::length_counter::LengthCounter;
use crate::apu::channel::volume_envelope::VolumeEnvelope;
use super::frame_sequencer::FrameSequencerStatus;

const NTSC_TIMER_PERIODS_TABLE: [u16; 16] = [ 4, 8, 16, 32, 64, 96, 128, 160, 202, 254, 380, 508, 762, 1016, 2034, 4068 ];
const PAL_TIMER_PERIODS_TABLE: [u16; 16] = [ 4, 8, 14, 30, 60, 88, 118, 148, 188, 236, 354, 472, 708,  944, 1890, 3778 ];

pub struct NoiseChannel {
    volume_envelope: VolumeEnvelope,
    pub length_counter: LengthCounter,

    timer_period: u16,
    timer: u16,

    mode_flag: bool,
    shift_register: u16,

    output: u8,
}

impl NoiseChannel {
    pub fn new() -> Self {
        Self {
            volume_envelope: VolumeEnvelope::new(),
            length_counter: LengthCounter::new(),

            timer_period: 0,
            timer: 0,

            mode_flag: false,

            // "On power-up, the shift register is loaded with the value 1"
            shift_register: 1,

            output: 0,
        }
    }

    pub fn update_output(&mut self) {
        if self.length() == 0 {
            self.output = 0;
        } else {
            let low_bit = self.shift_register & 1;
            self.output = if low_bit == 1 { self.volume_envelope.volume() } else { 0 }
        }
    }

    pub fn output(&self) -> u8 {
        self.output
    }

    fn update_shift_register(&mut self) {
        let selected_bit_location = if self.mode_flag { 6 } else { 1 };
        let bit_0 = self.shift_register & 1;
        let selected_bit = (self.shift_register >> selected_bit_location) & 1;
        let feedback = bit_0 ^ selected_bit;

        self.shift_register = (self.shift_register >> 1) & 0x3FFF;
        self.shift_register |= feedback << 14;
    }

    pub fn odd_step(&mut self, sequencer_state: FrameSequencerStatus) {
        match sequencer_state {
            FrameSequencerStatus::QuarterFrame => {
                self.volume_envelope.step_quarter_frame();
            },
            FrameSequencerStatus::HalfFrame => {
                self.length_counter.step_half_frame();
            }
            _ => {}
        }

        if self.timer == 0 {
            self.timer = self.timer_period;
            self.update_shift_register();
        } else {
            self.timer -= 1;
        }

        self.update_output();
    }

    pub fn length(&self) -> u8 {
        self.length_counter.length()
    }

    pub fn write(&mut self, address: u16, value: u8) {
        match address % 4 {
            0 => {
                self.length_counter.set_halt((value & 0b0010_0000) != 0);

                let constant_volume = (value & 0b0001_0000) != 0;
                let envelope_volume = value & 0xf;
                self.volume_envelope.set_volume(envelope_volume, constant_volume)
            }
            1 => { } // Sweep N/A
            2 => {
                let period_index = value & 0xf;
                self.timer_period = NTSC_TIMER_PERIODS_TABLE[period_index as usize];

                self.mode_flag = (value & 0b1000_0000) != 0;
            }
            3 => {
                self.length_counter.set_length(value >> 3);

                // "The envelope is also restarted"
                self.volume_envelope.restart();
            }
            _ => unreachable!()
        }
    }
}

