use crate::apu::channel::length_counter::LengthCounter;
use super::frame_sequencer::FrameSequencerStatus;

const OUTPUT_SEQUENCE: [u8; 32] = [ 15, 14, 13, 12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1, 0, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15 ];

pub struct TriangleChannel {
    pub length_counter: LengthCounter,

    timer_period: u16,
    timer: u16,

    sequence_pos: u8,

    output: u8,
}

impl TriangleChannel {
    pub fn new() -> Self {
        Self {
            length_counter: LengthCounter::new(),

            timer_period: 0,
            timer: 0,

            sequence_pos: 0,

            output: 0,
        }
    }

    pub fn update_output(&mut self) {
        if self.length() == 0 {
            self.output = 0;
        } else {
            self.output = OUTPUT_SEQUENCE[self.sequence_pos as usize];
        }
    }

    pub fn output(&self) -> u8 {
        self.output
    }

    // "Unlike the pulse channels, this timer ticks at the rate of the CPU clock rather than the APU (CPU/2) clock"
    pub fn step(&mut self, sequencer_state: FrameSequencerStatus) {
        match sequencer_state {
            FrameSequencerStatus::HalfFrame => {
                self.length_counter.step_half_frame();
            }
            _ => {}
        }

        // FIXME: double check this has a period of self.timer_period + 1
        if self.timer == 0 {
            self.timer = self.timer_period;
            self.sequence_pos = (self.sequence_pos + 1) % 32;
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
            }
            1 => { } // Sweep N/A
            2 => {
                self.timer_period = (self.timer_period & 0b0000_0111_0000_0000) | (value as u16);
            }
            3 => {
                self.length_counter.set_length(value >> 3);

                let timer_high = ((value as u16) & 0b111) << 8;
                self.timer_period = (self.timer_period & 0xff) | timer_high;
            }
            _ => unreachable!()
        }
    }
}

