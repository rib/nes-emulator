use super::channel::dmc_channel::DmcChannel;
use super::channel::noise_channel::NoiseChannel;
use super::channel::triangle_channel::TriangleChannel;
use crate::apu::channel::frame_sequencer::FrameSequencer;
use crate::apu::channel::square_channel::SquareChannel;
use crate::apu::mixer::Mixer;
use crate::constants::CPU_START_CYCLE;
use crate::system::{DmcDmaRequest, Model};
use crate::trace::TraceBuffer;

#[cfg(feature = "trace-events")]
use crate::trace::TraceEvent;

/// Debugger state attached to an APU instance that won't be cloned if the APU
/// is cloned, and some debug state may be partially preserved through a power cycle
#[derive(Default)]
pub struct NoCloneDebugState {
    #[cfg(feature = "trace-events")]
    pub trace_events_current: TraceBuffer,
    #[cfg(feature = "trace-events")]
    pub trace_events_prev: TraceBuffer,
}
impl Clone for NoCloneDebugState {
    fn clone(&self) -> Self {
        Self::default()
    }
}

#[derive(Clone, Default)]
pub struct Apu {
    pub clock: u64,
    pub sample_rate: u32,
    pub sample_buffer: Vec<f32>,
    pub frame_sequencer: FrameSequencer,
    pub square_channel1: SquareChannel,
    pub square_channel2: SquareChannel,
    pub triangle_channel: TriangleChannel,
    pub noise_channel: NoiseChannel,
    pub dmc_channel: DmcChannel,
    pub mixer: Mixer,
    output_timer: u16,
    output_step: u16,

    pub debug: NoCloneDebugState,
}

impl Apu {
    pub fn new(nes_model: Model, sample_rate: u32) -> Self {
        let cpu_clock_hz = nes_model.cpu_clock_hz();
        let output_step = (cpu_clock_hz / sample_rate) as u16;
        let clock = CPU_START_CYCLE;
        Apu {
            clock,
            sample_rate,
            output_step,
            frame_sequencer: FrameSequencer::new(clock),
            square_channel1: SquareChannel::new(nes_model, "Square 1".to_string(), false),
            square_channel2: SquareChannel::new(
                nes_model,
                "Square 2".to_string(),
                true, /* two's compliment sweep negate */
            ),
            triangle_channel: TriangleChannel::new(nes_model),
            noise_channel: NoiseChannel::new(),
            dmc_channel: DmcChannel::new(nes_model),
            mixer: Mixer::new(),
            ..Default::default() /*
                                 clock: 0,
                                 sample_buffer: vec![],
                                 output_timer: 0,
                                 */
        }
    }

    pub fn power_cycle(&mut self) {
        self.clock = CPU_START_CYCLE;
        self.sample_buffer.clear();
        self.frame_sequencer.power_cycle(self.clock);
        self.square_channel1.power_cycle();
        self.square_channel2.power_cycle();
        self.triangle_channel.power_cycle();
        self.noise_channel.power_cycle();
        self.dmc_channel.power_cycle();
        self.mixer.power_cycle();
        self.output_timer = 0;

        #[cfg(feature = "trace-events")]
        {
            self.debug.trace_events_current.clear();
            self.debug.trace_events_prev.clear();
        }

        // Keep output_step
    }

    pub fn reset(&mut self) {
        // "Power-up and reset have the effect of writing $00, silencing all channels."
        self.write(0x4015, 0);

        self.frame_sequencer.reset();
        self.dmc_channel.clear_interrupt();
    }

    /// Records an internal event into the back trace buffer
    ///
    /// This is a debug mechanism for being able to track mid-frame events which a
    /// debug tool can plot onto an expanded (341 x 262) framebuffer view covering
    /// the full dot clock range for a frame
    #[cfg(feature = "trace-events")]
    #[inline(always)]
    pub fn trace(&mut self, event: TraceEvent) {
        self.debug.trace_events_current.push(event);
    }

    #[cfg(feature = "trace-events")]
    #[inline(always)]
    pub fn trace_cpu_clock_line_sync(&mut self, cpu_clock: u64, new_frame: bool) {
        debug_assert_eq!(cpu_clock, self.clock);
        if new_frame {
            std::mem::swap(
                &mut self.debug.trace_events_current,
                &mut self.debug.trace_events_prev,
            );
            self.debug.trace_events_current.clear();
        }
        self.trace(TraceEvent::CpuClockLineSync { cpu_clk: cpu_clock });
    }

    // NB: we clock the APU with the CPU clock but many aspects of the APU
    // are only clocked every other CPU cycle
    pub fn step(&mut self) -> Option<DmcDmaRequest> {
        self.output_timer += 1;

        //println!("APU step: {}", apu_clock);

        let dma_request = self.dmc_channel.step_dma_reader();

        let frame_sequencer_output = self
            .frame_sequencer
            .step(self.clock, &mut self.debug.trace_events_current);
        if !frame_sequencer_output.is_empty() {
            //println!("frame sequencer output = {:?}, clock = {}", frame_sequencer_output, self.clock);
            debug_assert!(self.clock % 2 == 1);
        }

        // "The triangle channel's timer is clocked on every CPU cycle, but the pulse, noise, and DMC timers
        // are clocked only on every second CPU cycle and thus produce only even periods."

        self.triangle_channel.step(frame_sequencer_output);

        if self.clock % 2 == 1 {
            //println!("apu cycle: {}", self.clock);
            // "this timer is updated every APU cycle (i.e., every second CPU cycle)"
            self.square_channel1.odd_step(frame_sequencer_output);
            self.square_channel2.odd_step(frame_sequencer_output);

            self.noise_channel.odd_step(frame_sequencer_output);

            self.dmc_channel.odd_step(frame_sequencer_output);
        }

        // blargg apu 2005: 10.len_halt_timing
        // ; Changes to length counter halt occur after clocking length, not before.
        //
        // blargg apu 2005: 11.len_reload_timing
        // ; Write to length counter reload should be ignored when made during length
        // ; counter clocking and the length counter is not zero.
        self.square_channel1.length_counter.finish_apu_clock_step();
        self.square_channel2.length_counter.finish_apu_clock_step();
        self.triangle_channel.length_counter.finish_apu_clock_step();
        self.noise_channel.length_counter.finish_apu_clock_step();

        while self.output_timer >= self.output_step {
            let output = self.mixer.mix(
                self.square_channel1.output(),
                self.square_channel2.output(),
                self.triangle_channel.output(),
                self.noise_channel.output(),
                self.dmc_channel.output(),
                self.clock,
                &mut self.debug.trace_events_current,
            );

            // TODO: high-pass + low-pass filters

            //if output != 0.0 {
            //    println!("pushing sample {output}");
            //}
            self.sample_buffer.push(output);
            self.output_timer -= self.output_step;
        }

        self.clock += 1;

        dma_request
    }

    pub fn irq(&self) -> bool {
        self.frame_sequencer.interrupt_flagged || self.dmc_channel.interrupt_flagged
    }

    pub fn write(&mut self, address: u16, value: u8) {
        match address {
            0x4000..=0x4003 => self.square_channel1.write(address, value),
            0x4004..=0x4007 => self.square_channel2.write(address, value),
            0x4008..=0x400b => self.triangle_channel.write(address, value),
            0x400c..=0x400f => self.noise_channel.write(address, value),
            0x4010..=0x4013 => self.dmc_channel.write(address, value),
            0x4015 => {
                // Status/Control

                // "Writing to this register clears the DMC interrupt flag."
                // Note: the interrupt is cleared before possibly enabling DMC because
                // enabling DMC with a sample length of one can result in an immediate
                // re-trigger of the DMC interrupt
                self.dmc_channel.clear_interrupt();

                //println!("$4015 write {value:08b}");
                //println!("noise len = {}", self.noise_channel.length());
                //println!("noise enable = {noise_enable}");
                // NB: "If the DMC bit is clear, the DMC bytes remaining will be set to 0 and the DMC will silence when it empties."
                //     "If the DMC bit is set, the DMC sample will be restarted only if its bytes remaining is 0. If there are bits
                //      remaining in the 1-byte sample buffer, these will finish playing before the next sample is fetched."
                self.dmc_channel.set_enabled(value & 0b0001_0000 != 0);

                //self.dmc_channel.length_counter.set_enabled(value & 0b0001_0000 != 1);
                self.noise_channel
                    .length_counter
                    .set_enabled((value & 0b0000_1000) != 0);
                self.triangle_channel
                    .length_counter
                    .set_enabled((value & 0b0000_0100) != 0);

                //if (value & 0b0000_0010) != 0 {
                //    println!("Square channel 2 length counter enabled");
                //}
                self.square_channel2
                    .length_counter
                    .set_enabled((value & 0b0000_0010) != 0);

                //if (value & 0b0000_0001) != 0 {
                //    println!("Square channel 1 length counter enabled");
                //}
                self.square_channel1
                    .length_counter
                    .set_enabled((value & 0b0000_0001) != 0);
            }
            0x4017 => {
                //println!("Calling frame_sequencer.write_register({value:x})");
                self.frame_sequencer.write_register(value)
            }
            _ => {}
        }
    }

    fn read_4015_status(&self) -> (u8, u8) {
        // IF-D NT21	DMC interrupt (I), frame interrupt (F), DMC active (D), length counter > 0 (N/T/2/1)

        let dmc_interrupt = if self.dmc_channel.interrupt_flagged {
            0b1000_0000
        } else {
            0u8
        };
        let frame_interrupt = if self.frame_sequencer.interrupt_flagged {
            0b0100_0000
        } else {
            0u8
        };

        // "D will read as 1 if the DMC bytes remaining is more than 0."
        let dmc_active = if self.dmc_channel.is_active() {
            0b0001_0000
        } else {
            0u8
        };

        let noise_has_len = if self.noise_channel.length() > 0 {
            0b0000_1000
        } else {
            0u8
        };
        let triangle_has_len = if self.triangle_channel.length() > 0 {
            0b0000_0100
        } else {
            0u8
        };
        let square2_has_len = if self.square_channel2.length() > 0 {
            0b0000_0010
        } else {
            0u8
        };
        let square1_has_len = if self.square_channel1.length() > 0 {
            0b0000_0001
        } else {
            0u8
        };

        let value = dmc_interrupt
            | frame_interrupt
            | dmc_active
            | noise_has_len
            | triangle_has_len
            | square2_has_len
            | square1_has_len;

        //println!("$4015 read = {value:08b}");
        (value, 0b0010_0000)
    }

    // Returns: (value, undefined_bits)
    pub fn read(&mut self, address: u16) -> (u8, u8) {
        match address {
            0x4015 => {
                // Status
                // "Reading this register clears the frame interrupt flag (but not the DMC interrupt flag)."
                // TODO: "If an interrupt flag was set at the same moment of the read, it will read back as 1 but it will not be cleared"

                // Read status _before_ clearing the IRQ flag (which is part of the status)
                let status = self.read_4015_status();
                self.frame_sequencer.clear_irq();
                status
            }
            _ => (0, 0xff),
        }
    }

    // Returns: (value, undefined_bits)
    pub fn peek(&mut self, address: u16) -> (u8, u8) {
        match address {
            0x4015 => self.read_4015_status(),
            _ => (0, 0xff),
        }
    }
}
