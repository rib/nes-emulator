//use crate::emulation::CPU_CLOCK_HZ;

#[derive(Clone, Copy, Debug)]
pub enum FrameSequencerStatus {
    None,
    QuarterFrame,
    HalfFrame,
}

pub enum FrameSequencerMode {
    FourStep,
    FiveStep
}

pub struct FrameSequencer {
    clock: u16,
    queue_clock_reset: bool, // Can only reset clock on an even cycle
    pub mode: FrameSequencerMode,
    pub interrupt_enable: bool,

    pending_register_write: Option<u8>,
    pending_register_write_delay: Option<u8>,

    pub interrupt_flagged: bool,
}

impl FrameSequencer {
    pub fn new() -> Self {
        FrameSequencer {
            clock: 0,
            queue_clock_reset: false,
            mode: FrameSequencerMode::FourStep,
            interrupt_enable: true,
            interrupt_flagged: false,
            pending_register_write: None,
            pending_register_write_delay: None,

            //pending_4017_write_clock: false,
        }
    }

    pub fn write_register(&mut self, value: u8) {

        // "Writing to $4017 with bit 7 set ($80) will immediately clock all of its controlled units at the
        // beginning of the 5-step sequence; with bit 7 clear, only the sequence is reset without clocking
        // any of its units."
        //
        // XXX: This seems to conflict with the observation below, but there are explicit tests for this
        // behaviour. e.g. apu_test/rom_singles/1-len_ctr.nes does two back-to-back writes of $80 to $4017
        // with a length counter of two and expects that the APU will be silenced.
        if value & 0b1000_0000 != 0 {

        } else {

        }

        // "If the write occurs during an APU cycle, the effects occur 3 CPU cycles after the $4017 write cycle,
        // and if the write occurs between APU cycles, the effects occurs 4 CPU cycles after the write cycle."
        self.pending_register_write = Some(value);
        //println!("Queue pending 4017 write: clock = {}", self.clock);

        self.interrupt_enable = (value & 0b0100_0000) == 0;
        // "Interrupt inhibit flag. If set, the frame interrupt flag is cleared, otherwise it is unaffected"
        if self.interrupt_enable == false {
            self.interrupt_flagged = false;
        }
    }

    fn set_irq(&mut self) {
        if self.interrupt_enable {
            self.interrupt_flagged = true;
        }
    }

    pub fn clear_irq(&mut self) {
        self.interrupt_flagged = false;
    }

    pub fn step(&mut self, apu_clock: u64) -> FrameSequencerStatus {
        self.clock += 1;

        if self.queue_clock_reset {
            // Note: we must only ever reset the clock to zero on an even clock cycle
            // to ensure that the apu_clock and self.clock maintain the same polarity.
            debug_assert!(apu_clock % 2 == 0);
            self.clock = 0;
            self.queue_clock_reset = false;
        }

        // Considering that the square waves are only stepped on odd, APU clock cycles it's
        // important that the APU clock maintained at the system level maintains the same
        // polarity as the self.clock (otherwise the square wave channels will never
        // see the half/quarter frame clocks)
        debug_assert!((apu_clock % 2 == 1) == (self.clock % 2 == 1));

        // "The sequencer is clocked on every other CPU cycle, so 2 CPU cycles = 1 APU cycle"
        let is_apu_clock = apu_clock % 2 == 1;

        // APU sequencer constants come from https://www.nesdev.org/wiki/APU_Frame_Counter
        // except doubled because we step the frame sequencer by CPU clock cycles

        // Note: we only ever return a Half/QuarterFrame status on an odd/APU clock cycle
        // Note: we only ever reset the clock to zero on an even clock cycle
        let mut status = match self.mode {
            FrameSequencerMode::FourStep => {
                match self.clock {
                    7457 => FrameSequencerStatus::QuarterFrame,
                    14913 => FrameSequencerStatus::HalfFrame,
                    22371 => FrameSequencerStatus::QuarterFrame,
                    29828 => {
                        self.set_irq();
                        FrameSequencerStatus::None
                    }
                    29829 => {
                        self.set_irq();
                        FrameSequencerStatus::QuarterFrame
                    }
                    29830 => {
                        self.clock = 0;
                        self.set_irq();
                        FrameSequencerStatus::None
                    }
                    _ => FrameSequencerStatus::None
                }
            }
            FrameSequencerMode::FiveStep => {
                match self.clock {
                    7457 => FrameSequencerStatus::QuarterFrame,
                    14913 => FrameSequencerStatus::HalfFrame,
                    22371 => FrameSequencerStatus::QuarterFrame,
                    37281 => FrameSequencerStatus::HalfFrame,
                    37282 => {
                        self.clock = 0;
                        FrameSequencerStatus::None
                    }
                    _ => FrameSequencerStatus::None
                }
            }
        };
        //if !matches!(status, FrameSequencerStatus::None) {
        //    println!("Frame Sequencer Status = {:?}, clock = {}", status, self.clock);
        //}

        // The effects of $4017 register writes are delayed, and since the delay
        // also depends on whether the write happens on an odd or even cycle we
        // have to wait until we're stepping the APU to calculate the delay for
        // any pending register write we have.
        if let Some(value) = self.pending_register_write {
            //println!("Pending $4017 write: clock = {}", self.clock);
            let delay = self.pending_register_write_delay;
            match delay {
                Some(count) if count == 1 => {
                    self.mode = if value & 0b1000_0000 == 0 {
                        //println!("Applying $4017 write, four step: clock = {}", self.clock);
                        FrameSequencerMode::FourStep
                    } else {
                        //println!("Applying $4017 write, five step: clock = {}", self.clock);
                        // "If the mode flag is set, then both "quarter frame" and "half frame" signals are also generated."
                        status = FrameSequencerStatus::HalfFrame;

                        FrameSequencerMode::FiveStep
                    };

                    self.pending_register_write = None;
                    self.pending_register_write_delay = None;

                    // Note: Half/QuarterFrame clocks happen on odd, APU clocks but we
                    // must only ever reset the clock to zero on an even clock cycle
                    // to ensure that the apu_clock and self.clock maintain the same polarity
                    // so the clock reset will be queued for the next clock cycle.
                    debug_assert!(is_apu_clock);
                    self.queue_clock_reset = true;
                },
                Some(value) => {
                    self.pending_register_write_delay = Some(value - 1);
                },
                None => {
                    // "After 3 or 4 CPU clock cycles*, the timer is reset.
                    //
                    // * If the write occurs during an APU cycle, the effects occur 3 CPU cycles after the $4017
                    //   write cycle, and if the write occurs between APU cycles, the effects occurs 4 CPU cycles
                    //   after the write cycle."
                    if is_apu_clock {
                        //println!("Setting $4017 write delay = 3 cycles: apu_clock = {apu_clock}, self.clock = {}", self.clock);
                        self.pending_register_write_delay = Some(2); // skip 1 APU clock (2 cpu clocks) + 1 cycle extra delay for the clock reset (must happen on an even cycle)
                    } else {
                        //println!("Setting $4017 write delay = 4 cycles: apu_clock = {apu_clock}, self.clock = {}", self.clock);
                        self.pending_register_write_delay = Some(3); // 1 cycle to get to next APU clock, skip one APU clock, then + 1 cycle extra delay for the clock reset (must happen on an even cycle)
                    }
                }
            }
        }
        //if self.clock == 7457 {
        //    println!("quarter frame: status = {status:?}");
        //}
        //if self.clock == 14913 {
        //    println!("half frame: status = {status:?}");
        //}
        status
    }

}
