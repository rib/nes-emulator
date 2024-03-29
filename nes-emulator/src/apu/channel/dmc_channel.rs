use std::ops::Index;

use super::frame_sequencer::FrameSequencerStatus;
use crate::system::{DmcDmaRequest, Model};

// Ref: https://www.nesdev.org/wiki/APU_DMC
// Measured in CPU clock cycles
const DMC_PERIODS_TABLE_NTSC: [u16; 16] = [
    428, 380, 340, 320, 286, 254, 226, 214, 190, 160, 142, 128, 106, 84, 72, 54,
];

#[allow(dead_code)]
const DMC_PERIODS_TABLE_PAL: [u16; 16] = [
    398, 354, 316, 298, 276, 236, 210, 198, 176, 148, 132, 118, 98, 78, 66, 50,
];

/// Newtype so we can impl Default
#[derive(Clone)]
pub struct PeriodsTable(&'static [u16; 16]);
impl Default for PeriodsTable {
    fn default() -> Self {
        Self(&DMC_PERIODS_TABLE_NTSC)
    }
}
impl Index<usize> for PeriodsTable {
    type Output = u16;
    fn index(&self, index: usize) -> &Self::Output {
        self.0.index(index)
    }
}

#[derive(Clone, Default)]
pub struct DmcChannel {
    nes_model: Model,

    periods_table: PeriodsTable,

    pub interrupt_enable: bool,
    pub interrupt_flagged: bool,

    dma_in_progress: bool,

    pub loop_flag: bool,

    pub pending_sample_address: u16,
    sample_address: u16,

    pub pending_sample_bytes_remaining: u16,
    sample_bytes_remaining: u16,
    sample_buffer: Option<u8>,

    output_shift: u8,
    output_bits_remaining: u8,
    output_silence_flag: bool,

    pub timer_period: u16,
    pub timer: u16, // counts down from `timer_period`

    pub output: u8,
}

impl DmcChannel {
    pub fn new(nes_model: Model) -> Self {
        let periods_table = match nes_model {
            Model::Ntsc => PeriodsTable(&DMC_PERIODS_TABLE_NTSC),
            Model::Pal => PeriodsTable(&DMC_PERIODS_TABLE_PAL),
        };

        let timer_period = periods_table[0];
        let timer = periods_table[0];
        DmcChannel {
            nes_model,
            periods_table,
            pending_sample_address: 0xc000,
            pending_sample_bytes_remaining: 1,
            output_silence_flag: true,
            output_bits_remaining: 8,
            timer_period,
            timer,
            ..Default::default() /*
                                 interrupt_enable: false,
                                 interrupt_flagged: false,

                                 dma_in_progress: false,

                                 loop_flag: false,

                                 // DMA reader...
                                 sample_address: 0,
                                 sample_bytes_remaining: 0,

                                 sample_buffer: None,

                                 output_bits_remaining: 8,
                                 output_shift: 0,

                                 output: 0,
                                 */
        }
    }

    pub fn power_cycle(&mut self) {
        *self = Self::new(self.nes_model);
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        //println!("DMC enabled = {enabled}");

        // NB: "If the DMC bit is clear, the DMC bytes remaining will be set to 0 and the DMC will silence when it empties."
        //     "If the DMC bit is set, the DMC sample will be restarted only if its bytes remaining is 0. If there are bits
        //      remaining in the 1-byte sample buffer, these will finish playing before the next sample is fetched."

        if enabled {
            if self.sample_bytes_remaining == 0 {
                self.start_sample();
            }

            // Regarding what the DCM memory reader does when the "sample buffer is in an empty state and bytes remaining is not zero"
            // nesdev explicitly clarifies:
            // "(including just after a write to $4015 that enables the channel, regardless of where that write occurs relative to
            //   the bit counter mentioned below)"
            //self.step_dma_reader();
        } else {
            self.sample_bytes_remaining = 0;
        }
    }

    fn start_sample(&mut self) {
        self.sample_address = self.pending_sample_address;
        self.sample_bytes_remaining = self.pending_sample_bytes_remaining;
        //println!("DMC: start sample: address = {}, len = {}", self.sample_address, self.sample_bytes_remaining);
    }

    pub fn step_dma_reader(&mut self) -> Option<DmcDmaRequest> {
        if self.dma_in_progress {
            return None;
        }

        if self.sample_buffer.is_none() && self.sample_bytes_remaining > 0 {
            let dma_addr = self.sample_address;

            // "The address is incremented; if it exceeds $FFFF, it is wrapped around to $8000"
            self.sample_address = self.sample_address.wrapping_add(1);
            if self.sample_address == 0 {
                self.sample_address = 0x8000;
            }

            // "The bytes counter is decremented;
            //  if it becomes zero and the loop flag is set, the sample is restarted (see above),
            //  otherwise if the bytes counter becomes zero and the interrupt enabled flag is set,
            //  the interrupt flag is set."
            self.sample_bytes_remaining -= 1;
            if self.sample_bytes_remaining == 0 {
                //println!("DMC: reading last sample byte");
                if self.loop_flag {
                    self.start_sample();
                } else if self.interrupt_enable {
                    self.interrupt_flagged = true;
                    //println!("DMC: flagging interrupt");
                }
            }

            // We have to defer to the System to do the DMA for us and we will
            // get a .completed_dma() callback
            // NB: "When the DMA reader accesses a byte of memory, the CPU is suspended for 4 clock cycles"
            //println!("DCM: request DMA address = {dma_addr:x}");
            self.dma_in_progress = true;
            Some(DmcDmaRequest { address: dma_addr })
        } else {
            None
        }
    }

    /// After we request a DMC DMA we will get a `completed_dma` callback after 2-4 clock cycles
    /// with the read sample `value`
    pub fn completed_dma(&mut self, _address: u16, value: u8) {
        debug_assert!(self.dma_in_progress);

        self.sample_buffer = Some(value);
        self.dma_in_progress = false;
    }

    fn start_output_cycle(&mut self) {
        // When an output cycle is started, the counter is loaded with 8 and if the sample
        // buffer is empty, the silence flag is set, otherwise the silence flag is cleared
        // and the sample buffer is emptied into the shift register.
        self.output_bits_remaining = 8;
        if let Some(sample) = self.sample_buffer {
            self.output_shift = sample;
            self.sample_buffer = None;
            self.output_silence_flag = false;
        } else {
            self.output_silence_flag = true;
        }
    }

    fn step_output(&mut self) {
        // The output unit continually outputs complete sample bytes or silences of equal
        // duration. It contains an 8-bit right shift register, a counter, and a silence
        // flag.

        // When an output cycle is started, the counter is loaded with 8 and if the sample
        // buffer is empty, the silence flag is set, otherwise the silence flag is cleared
        // and the sample buffer is emptied into the shift register.

        // On the arrival of a clock from the timer, the following actions occur in order:

        //     1. If the silence flag is clear, bit 0 of the shift register is applied to
        // the DAC counter: If bit 0 is clear and the counter is greater than 1, the
        // counter is decremented by 2, otherwise if bit 0 is set and the counter is less
        // than 126, the counter is incremented by 2.

        //     1) The shift register is clocked.

        //     2) The counter is decremented. If it becomes zero, a new cycle is started.

        if !self.output_silence_flag {
            let up = self.output_shift & 1 == 1;
            if up && self.output < 126 {
                self.output += 2;
            } else if !up && self.output > 1 {
                self.output -= 2;
            }
        }
        self.output_shift >>= 1;
        self.output_bits_remaining -= 1;
        if self.output_bits_remaining == 0 {
            self.start_output_cycle();
        }

        //println!("DMC: clock output: bits remaining = {}, bytes remaining = {}, silence = {}", self.output_bits_remaining, self.sample_bytes_remaining, self.output_silence_flag);
    }

    pub fn output(&self) -> u8 {
        self.output
    }

    // Returns: number of cycles to pause cpu if sample buffer DMA started
    pub fn odd_step(&mut self, _sequencer_state: FrameSequencerStatus) {
        if self.timer == 0 {
            self.timer = self.timer_period;
            //println!("DMC: step: reset timer = {}", self.timer);
            self.step_output();
        } else {
            self.timer -= 2; // The periods in DMC_PERIODS_TABLE_NTSC are for cpu clock cycles (2 x cpu cycles per APU cycle)
                             //println!("DMC: step: dec timer = {}", self.timer);
        }
    }

    pub fn write(&mut self, address: u16, value: u8) {
        //println!("DMC write ${address:x} = {value:x}");
        match address % 4 {
            0 => {
                self.interrupt_enable = (value & 0b1000_0000) != 0;

                // "IRQ enabled flag. If clear, the interrupt flag is cleared."
                if !self.interrupt_enable {
                    self.interrupt_flagged = false;
                }

                self.loop_flag = (value & 0b0100_0000) != 0;

                self.timer_period = self.periods_table[(value & 0xf) as usize];
                //println!("$4010 write: rate[{}] = {} (timer = {})", (value & 0xf), self.timer_period, self.timer);
            }
            1 => {
                // Direct Load
                self.output = value & 0b0111_1111; // 7-bit output
            }
            2 => {
                self.pending_sample_address = 0xc000 + 64 * value as u16;
            }
            3 => {
                self.pending_sample_bytes_remaining = (value as u16 * 16) + 1;
                //println!("DMC: $4013 write, set (pending) sample length = {}", self.pending_sample_bytes_remaining);
            }
            _ => unreachable!(),
        }
    }

    pub fn clear_interrupt(&mut self) {
        //println!("DMC: clear interrupt");
        self.interrupt_flagged = false;
    }

    // For the $4015 status register, DMC active bit:
    // ""D will read as 1 if the DMC bytes remaining is more than 0.""
    pub fn is_active(&self) -> bool {
        self.sample_bytes_remaining > 0
    }
}
