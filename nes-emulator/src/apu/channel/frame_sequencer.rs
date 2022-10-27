use bitflags::bitflags;

//use crate::emulation::CPU_CLOCK_HZ;

use crate::trace::{TraceBuffer, TraceEvent};

/*
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameSequencerStatus {
    None,
    QuarterFrame,
    HalfFrame,
}
*/
bitflags! {
    #[derive(Default)]
    pub struct FrameSequencerStatus: u8 {
        const QUARTER_FRAME = 1;
        const HALF_FRAME = 2;
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum FrameSequencerMode {
    #[default]
    FourStep,
    FiveStep,
}

#[derive(Clone, Default)]
pub struct FrameSequencer {
    clock: u16,
    queue_clock_reset: bool, // Can only reset clock on an even cycle
    pub mode: FrameSequencerMode,
    pub interrupt_enable: bool,

    last_register_write: u8,
    pending_register_write: bool,
    pending_register_write_delay: u8,

    pub interrupt_flagged: bool,
}

impl FrameSequencer {
    pub fn new(start_apu_clock: u64) -> Self {
        // blargg apu 2005: 09.reset_timing:
        // ; After reset or power-up, APU acts as if $4017 were written with
        // ; $00 from 9 to 12 clocks before first instruction begins.
        //
        // These values work to pass the reset_timing test - not sure why
        // >=9 didn't work
        //
        // NB: make sure polarity matches start_apu_clock
        let clock = if start_apu_clock % 2 == 0 { 6 } else { 7 };
        Self {
            interrupt_enable: true,
            clock,

            ..Default::default()
        }
    }

    pub fn power_cycle(&mut self, start_apu_clock: u64) {
        *self = Self::new(start_apu_clock);
    }

    pub fn reset(&mut self) {
        self.clear_irq();

        // apu_reset:4017_written
        // "At reset, $4017 should should be rewritten with last value written"
        self.write_register(self.last_register_write);
    }

    // "The sequencer is clocked on every other CPU cycle, so 2 CPU cycles = 1 APU cycle"
    #[inline(always)]
    fn is_apu_cycle(&self) -> bool {
        self.clock % 2 == 1
    }

    pub fn write_register(&mut self, value: u8) {
        // "Writing to $4017 with bit 7 set ($80) will immediately clock all of its controlled units at the
        // beginning of the 5-step sequence; with bit 7 clear, only the sequence is reset without clocking
        // any of its units."
        //
        // XXX: This seems to conflict with the observation below, but there are explicit tests for this
        // behaviour. e.g. apu_test/rom_singles/1-len_ctr.nes does two back-to-back writes of $80 to $4017
        // with a length counter of two and expects that the APU will be silenced.
        //if value & 0b1000_0000 != 0 {
        //} else {
        //}

        // "If the write occurs during an APU cycle, the effects occur 3 CPU cycles after the $4017 write cycle,
        // and if the write occurs between APU cycles, the effects occurs 4 CPU cycles after the write cycle."
        self.pending_register_write = true;
        self.last_register_write = value;

        // "Writing to $4017 resets the frame counter and the quarter/half frame triggers happen simultaneously,
        // but only on "odd" cycles (and only after the first "even" cycle after the write occurs) - thus, it happens
        // either 2 or 3 cycles after the write (i.e. on the 2nd or 3rd cycle of the next instruction).
        //
        // After 2 or 3 clock cycles (depending on when the write is performed), the timer is reset.
        //
        // Writing to $4017 with bit 7 set ($80) will immediately clock all of its controlled units at the beginning
        // of the 5-step sequence; with bit 7 clear, only the sequence is reset without clocking any of its units."

        if self.is_apu_cycle() {
            //println!("Setting $4017 write delay = 3 cycles: apu_clock = {}, self.clock = {}", self.clock, self.is_apu_cycle());
            self.pending_register_write_delay = 2;
        } else {
            //println!("Setting $4017 write delay = 2 cycles: apu_clock = {}, self.clock = {}", self.clock, self.is_apu_cycle());
            self.pending_register_write_delay = 1;
        }

        /*
        {
            let is_apu_clock = self.clock % 2 == 1;
            let apply_target = if is_apu_clock {
                self.clock + 3
            } else {
                self.clock + 4
            };
            println!("Queue pending 4017 write: clock = {}, expect write apply @ {}", self.clock, apply_target);
        }
        */

        self.interrupt_enable = (value & 0b0100_0000) == 0;
        // "Interrupt inhibit flag. If set, the frame interrupt flag is cleared, otherwise it is unaffected"
        if !self.interrupt_enable {
            self.interrupt_flagged = false;
        }
    }

    fn set_irq(&mut self) {
        if self.interrupt_enable {
            self.interrupt_flagged = true;
        }
    }

    pub fn clear_irq(&mut self) {
        //println!("Frame Sequencer: clear irq flag, clock = {}", self.clock);
        self.interrupt_flagged = false;
    }

    pub fn step(&mut self, apu_clock: u64, trace: &mut TraceBuffer) -> FrameSequencerStatus {
        if self.queue_clock_reset {
            // Note: we must only ever reset the clock to zero on an even clock cycle
            // to ensure that the apu_clock and self.clock maintain the same polarity.
            debug_assert!(apu_clock % 2 == 0);
            self.clock = 0;
            self.queue_clock_reset = false;
        }

        //println!("Frame Sequencer step: apu clock = {}, seq clock = {}", apu_clock, self.clock);
        // Considering that the square waves are only stepped on odd, APU clock cycles it's
        // important that the APU clock maintained at the system level maintains the same
        // polarity as the self.clock (otherwise the square wave channels will never
        // see the half/quarter frame clocks)
        debug_assert!((apu_clock % 2 == 1) == (self.clock % 2 == 1));

        // APU sequencer constants come from https://www.nesdev.org/wiki/APU_Frame_Counter
        // except doubled because we step the frame sequencer by CPU clock cycles

        // Note: we only ever return a Half/QuarterFrame status on an odd/APU clock cycle
        // Note: we only ever reset the clock to zero on an even clock cycle
        //let mut status = FrameSequencerStatus::default();
        let mut status = match self.mode {
            FrameSequencerMode::FourStep => {
                match self.clock {
                    7457 => FrameSequencerStatus::QUARTER_FRAME,
                    14913 => FrameSequencerStatus::QUARTER_FRAME | FrameSequencerStatus::HALF_FRAME,
                    22371 => FrameSequencerStatus::QUARTER_FRAME,
                    29828 => {
                        //println!("Frame Sequencer: set_irq, clock = {}", self.clock);
                        self.set_irq();
                        FrameSequencerStatus::default()
                    }
                    29829 => {
                        //println!("Frame Sequencer: set_irq, clock = {}", self.clock);
                        self.set_irq();
                        FrameSequencerStatus::QUARTER_FRAME | FrameSequencerStatus::HALF_FRAME
                    }
                    29830 => {
                        self.clock = 0;
                        //println!("Frame Sequencer: set_irq, clock = {}", self.clock);
                        self.set_irq();
                        FrameSequencerStatus::default()
                    }
                    _ => FrameSequencerStatus::default(),
                }
            }
            FrameSequencerMode::FiveStep => match self.clock {
                7457 => FrameSequencerStatus::QUARTER_FRAME,
                14913 => FrameSequencerStatus::QUARTER_FRAME | FrameSequencerStatus::HALF_FRAME,
                22371 => FrameSequencerStatus::QUARTER_FRAME,
                37281 => FrameSequencerStatus::QUARTER_FRAME | FrameSequencerStatus::HALF_FRAME,
                37282 => {
                    self.clock = 0;
                    FrameSequencerStatus::default()
                }
                _ => FrameSequencerStatus::default(),
            },
        };
        //if status != FrameSequencerStatus::default() {
        //    println!("Frame Sequencer Status = {:?}, clock = {}", status, self.clock);
        //}
        //println!("SEQ: seq clock = {}, clock = {}, status = {:?}, mode = {:?}", self.clock, apu_clock,  status, self.mode);

        if self.pending_register_write {
            //println!("Pending $4017 write: clock = {}", self.clock);
            if self.pending_register_write_delay == 0 {
                self.mode = if self.last_register_write & 0b1000_0000 == 0 {
                    //println!("Applying $4017 write, four step: clock = {}", self.clock);
                    FrameSequencerMode::FourStep
                } else {
                    //println!("Applying $4017 write, five step: clock = {}", self.clock);
                    // "If the mode flag is set, then both "quarter frame" and "half frame" signals are also generated."
                    status = FrameSequencerStatus::QUARTER_FRAME | FrameSequencerStatus::HALF_FRAME;

                    FrameSequencerMode::FiveStep
                };

                self.pending_register_write = false;

                // Note: Half/QuarterFrame clocks happen on odd, APU clocks but we
                // must only ever reset the clock to zero on an even clock cycle
                // to ensure that the apu_clock and self.clock maintain the same polarity
                // so the clock reset will be queued for the next clock cycle.
                debug_assert!(self.is_apu_cycle());
                self.queue_clock_reset = true;
            }
        }
        //if self.clock == 7457 {
        //    println!("quarter frame: status = {status:?}");
        //}
        //if self.clock == 14913 {
        //    println!("half frame: status = {status:?}");
        //}

        #[cfg(feature = "trace-events")]
        if !status.is_empty() {
            trace.push(TraceEvent::ApuFrameSeqFrame {
                clk_lower: (apu_clock & 0xff) as u8,
                status,
            });
        }

        // If other units are only stepped on APU cycles then they could miss half/quarter frames if
        // they spuriously happen on a non-apu cycle
        #[cfg(debug_assertions)]
        if !status.is_empty() {
            debug_assert!(self.is_apu_cycle());
        }

        self.clock += 1;
        if self.pending_register_write_delay > 0 {
            self.pending_register_write_delay -= 1;
        }
        status
    }
}
