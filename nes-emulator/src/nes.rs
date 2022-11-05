use instant::{Instant, Duration};

use anyhow::Result;

use crate::apu::core::Apu;
//use crate::binary;
use crate::binary::NesBinaryConfig;
use crate::cpu::instruction::FetchedOperand;
use crate::cpu::instruction::Instruction;
//use crate::cartridge;
use crate::cartridge::*;
use crate::constants::*;
use crate::cpu::core::*;
use crate::framebuffer::*;
use crate::genie::GameGenieCode;
#[cfg(feature = "trace")]
use crate::hook::{HookHandle, HooksList};
use crate::ppu::*;
use crate::system::*;

#[cfg(feature = "nsf-player")]
use crate::binary::NsfConfig;

pub type FnInstructionTraceHook = dyn FnMut(&mut Nes, &TraceState);

/// A target for progressing the emulator that provides a limit on how much time
/// is spent emulating before giving control back to the caller.
#[derive(Clone, Copy)]
pub enum ProgressTarget {
    /// Progress until the CPU clock reaches the given count
    ///
    /// It's recommended that graphical front ends should calculate clock targets
    /// according to the known frequency of the CPU combined with monitoring
    /// the performance of the emulator so that upper limits can be set to ensure
    /// the UI can remain interactive, even if emulation is not able to run
    /// at full speed (such as for debug builds)
    ///
    /// To help with calculating target clock counts, see:
    /// [`Nes::cpu_clocks_for_duration`] and [`Nes::cpu_clocks_for_time_since_power_cycle`]
    Clock(u64),

    /// Progress up to the given time, relative to the [`Nes::power_cycle`] time
    ///
    /// Internally the target is converted into a clock target by
    /// measuring the duration between this instant and `start_timestamp` (given to
    /// [`Nes::power_cycle`]) and using the CPU's clock frequency to calculate
    /// what the clock counter should be at the given timestamp in the future.
    ///
    /// If wall-clock timestamps are given (`Instant::now()`) then the emulation
    /// speed will appear to match that of the original hardware, so long as the
    /// emulator is keeping up.
    ///
    /// If the emulator can't keep up with wall-clock time (for example with
    /// a debug build) then the target will take increasingly longer and longer
    /// to reach and it will not be possible to interact with the emulator.
    ///
    /// It's therefore generally recommended for graphical front ends to use a
    /// carefully managed [`ProgressTarget::Clock`] target instead that can
    /// impose strict limits on the `ProgressTarget` that can ensure any UI
    /// remains interactive even if the emulator is not currently able to keep
    /// up.
    Time(Instant),

    /// Progress until a new frame is ready
    ///
    /// This is generally only recommended for batched, non-interactive emulation
    /// in case the emulator is unable to run at full speed (such as for debug
    /// builds)
    FrameReady,
}

pub enum ProgressStatus {
    FrameReady,
    ReachedTarget,
    Breakpoint,
}

#[cfg(feature = "nsf-player")]
#[derive(Clone, Debug, Default)]
struct NsfPlayer {
    nsf_config: Option<NsfConfig>,
    nsf_initialized: bool,
    nsf_waiting: bool,
    nsf_step_period: u64,
    nsf_last_step_cycle: u64,
    nsf_current_track: u8,
}
#[cfg(feature = "nsf-player")]
impl NsfPlayer {
    pub(crate) fn restart(&mut self) {
        *self = Self {
            nsf_config: self.nsf_config.clone(),
            ..Default::default()
        };
    }
}

/// The top-level representation of a full NES console
pub struct Nes {
    pub model: Model,
    cpu_clock_hz: u32,
    cpu_clocks_per_frame: f32,

    cpu: Cpu,
    reference_timestamp: Instant,
    reference_cpu_clock: u64,

    system: System,

    #[cfg(feature = "ppu-sim")]
    ppu_sim_visible: bool,

    #[cfg(feature = "nsf-player")]
    nsf_player: NsfPlayer,

    #[cfg(feature = "trace")]
    trace_hooks: HooksList<FnInstructionTraceHook>,
}

impl Nes {
    /// Creates a new Nes console, powered on but with no cartridge inserted.
    ///
    /// The next step is to load and insert a cartridge, either manually via
    /// [`Cartridge::from_binary`] and [`Nes::insert_cartridge`] or by calling [`Nes::open_binary`]
    ///
    /// After a cartridge has been inserted then the Nes should be reset again
    /// either via [`Nes::reset`] or [`Nes:power_cycle`]
    ///
    /// Nes emulation can then be progressed by repeatedly calling [`Nes:progress`]
    pub fn new(model: Model, audio_sample_rate: u32, start_timestamp: Instant) -> Nes {
        let cpu = Cpu::default();
        let system = System::new(model, audio_sample_rate, Cartridge::none());
        let (cpu_clock_hz, cpu_clocks_per_frame) = match model {
            Model::Ntsc => (NTSC_CPU_CLOCK_HZ, 29780.5),
            Model::Pal => (PAL_CPU_CLOCK_HZ, 33247.5),
        };
        let mut nes = Nes {
            model,
            cpu_clock_hz,
            cpu_clocks_per_frame,

            reference_timestamp: start_timestamp,
            reference_cpu_clock: 0,

            cpu,
            system,

            #[cfg(feature = "ppu-sim")]
            ppu_sim_visible: true,

            #[cfg(feature = "nsf-player")]
            nsf_player: NsfPlayer {
                nsf_config: None,
                nsf_initialized: false,
                nsf_waiting: false,
                nsf_step_period: 0,
                nsf_last_step_cycle: 0,
                nsf_current_track: 0,
            },

            #[cfg(feature = "trace")]
            trace_hooks: HooksList::default(),
        };

        nes.power_cycle(start_timestamp);
        nes
    }

    /// Loads the given `binary` as a `Cartridge` and inserts the loaded cartridge
    ///
    /// It's also necessary to explicitly power cycle or reset the Nes via [`Nes::power_cycle`]
    /// or [`Nes::reset`].
    ///
    /// This may fail if opening an NSF binary if the "nsf-player" feature is not enabled.
    pub fn open_binary(&mut self, binary: &[u8]) -> Result<()> {
        match Cartridge::from_binary(binary) {
            Ok(cartridge) => {
                if let Err(err) = self.insert_cartridge(Some(cartridge)) {
                    log::error!("Failed to insert cartridge: {:?}", err);
                    self.insert_cartridge(None)?;
                    Err(err)?
                }
            }
            Err(err) => {
                log::error!("Failed to open binary: {:?}", err);
                self.insert_cartridge(None)?;
                Err(err)?
            }
        }
        Ok(())
    }

    /// Inserts the given cartridge
    ///
    /// It's also necessary to explicitly power cycle or reset the Nes via [`Nes::power_cycle`]
    /// or [`Nes::reset`].
    ///
    /// This may fail if inserting an NSF cartridge if the "nsf-player" feature is not enabled.
    pub fn insert_cartridge(&mut self, cartridge: Option<Cartridge>) -> Result<()> {
        if let Some(cartridge) = cartridge {
            if let NesBinaryConfig::Nsf(nsf_config) = &cartridge.config {
                #[cfg(feature = "nsf-player")]
                {
                    self.nsf_player.nsf_config = Some(nsf_config.clone());
                    self.system.insert_cartridge(cartridge);
                }
                #[cfg(not(feature = "nsf-player"))]
                {
                    let _ = nsf_config;
                    Err(anyhow::anyhow!(
                        "NSF cartridges not supported (missing \"nsf-player\" feature"
                    ))?
                }
            } else {
                self.system.insert_cartridge(cartridge);
            }
        } else {
            self.system.insert_cartridge(Cartridge::none());
        }
        Ok(())
    }

    #[cfg(feature = "nsf-player")]
    fn nsf_init(&mut self) {
        #[cfg(feature = "nsf-player")]
        if let Some(ref nsf_config) = self.nsf_player.nsf_config {
            // TODO: handle PAL...
            self.nsf_player.nsf_step_period = ((nsf_config.ntsc_play_speed as u64
                * NTSC_CPU_CLOCK_HZ as u64)
                / 1_000_000u64) as u64;
            self.nsf_player.nsf_last_step_cycle = self.cpu.clock;

            // "1. Write $00 to all RAM at $0000-$07FF and $6000-$7FFF."
            // (already assumed to be the poweron state)

            // 2. Initialize the sound registers by writing $00 to $4000-$4013, and $00 then $0F to $4015.
            for i in 0..0x13 {
                self.system.cpu_write(0x4000 + i, 0x00);
                self.cpu.clock += 1;
            }
            self.system.cpu_write(0x4015, 0x00);
            self.cpu.clock += 1;
            self.system.cpu_write(0x4015, 0x0f);
            self.cpu.clock += 1;

            // 3. Initialize the frame counter to 4-step mode ($40 to $4017).
            self.system.cpu_write(0x4017, 0x40);
            self.cpu.clock += 1;

            // 4. If the tune is bank switched, load the bank values from $070-$077 into $5FF8-$5FFF.
            // (handled by Mapper 031)

            // 5. Set the A register for the desired song.
            let first_track = nsf_config.first_song - 1;
            self.cpu.a = first_track;

            // 6. Set the X register for PAL or NTSC.
            self.cpu.x = nsf_config.tv_system_byte;

            // 7. Call the music INIT routine.
            let init = nsf_config.init_address;
            self.system.cpu_write(0x5001, (init & 0xff) as u8);
            self.cpu.clock += 1;
            self.system.cpu_write(0x5002, ((init & 0xff00) >> 8) as u8);
            self.cpu.clock += 1;
            //self.cpu.add_break(0x5003, false); // break when we hit the infinite loop in the NSF bios
            self.nsf_player.nsf_initialized = false;
            self.nsf_player.nsf_current_track = first_track;
            self.cpu.pc = 0x5000;

            println!(
                "Calling NSF init code: period = {}",
                self.nsf_player.nsf_step_period
            );
        }
    }

    /// Initializes the hardware as if the Nes were powered off and then on again.
    ///
    /// Where appropriate this will preserve debug state (such as breakpoints and PPU hooks)
    pub fn power_cycle(&mut self, start_timestamp: Instant) {
        self.system.power_cycle();
        self.cpu.power_cycle();

        debug_assert_eq!(self.cpu.clock, self.system.apu.clock);

        self.reference_cpu_clock = 0;
        self.reference_timestamp = start_timestamp;

        #[cfg(feature = "nsf-player")]
        {
            self.nsf_player.restart();

            if self.nsf_player.nsf_config.is_some() {
                self.nsf_init();
            } else {
                self.cpu
                    .handle_interrupt(&mut self.system, Interrupt::RESET);
            }
        }
        #[cfg(not(feature = "nsf-player"))]
        {
            self.cpu
                .handle_interrupt(&mut self.system, Interrupt::RESET);
        }
    }

    /// Raises a reset interrupt for the CPU as if the reset button were pressed on the Nes
    pub fn reset(&mut self) {
        self.system.reset();
        self.cpu.reset(&mut self.system);

        #[cfg(feature = "nsf-player")]
        self.nsf_player.restart();
    }

    /// Get a mutable reference to the system bus, which also owns the PPU and APU
    pub fn system_mut(&mut self) -> &mut System {
        &mut self.system
    }

    /// Get a mutable reference to the CPU
    pub fn cpu_mut(&mut self) -> &mut Cpu {
        &mut self.cpu
    }

    /// Get a mutable reference to the PPU
    pub fn ppu_mut(&mut self) -> &mut Ppu {
        &mut self.system.ppu
    }

    /// Get a mutable reference to the APU
    pub fn apu_mut(&mut self) -> &mut Apu {
        &mut self.system.apu
    }

    /// Read a value from the system bus, without side effects
    pub fn peek_system_bus(&mut self, addr: u16) -> u8 {
        self.system.peek(addr)
    }

    /// Read a value from the PPU bus, without side effects
    pub fn peek_ppu_bus(&mut self, addr: u16) -> u8 {
        self.system
            .ppu
            .unbuffered_ppu_bus_peek(&mut self.system.cartridge, addr)
    }

    /// Allocate a new framebuffer that is suitable for using as a PPU render target
    ///
    /// To configure the PPU to start rendering to this framebuffer then call [`Ppu:swap_framebuffer`]
    pub fn allocate_framebuffer(&self) -> Framebuffer {
        #[cfg(feature = "ppu-sim")]
        {
            self.system.debug.ppu_sim.alloc_framebuffer()
        }

        #[cfg(not(feature = "ppu-sim"))]
        {
            self.system.ppu.alloc_framebuffer()
        }
    }

    pub fn swap_framebuffer(&mut self, framebuffer: Framebuffer) -> Result<Framebuffer> {
        #[cfg(feature = "ppu-sim")]
        {
            if self.ppu_sim_visible {
                self.system.debug.ppu_sim.swap_framebuffer(framebuffer)
            } else {
                self.system.ppu.swap_framebuffer(framebuffer)
            }
        }

        #[cfg(not(feature = "ppu-sim"))]
        {
            self.system.ppu.swap_framebuffer(framebuffer)
        }
    }

    /// Add a hook function to trace all CPU instructions executed
    ///
    /// Debuggers can use this to display a real-time disassembly trace of instructions or
    /// to write a disassembly log to a file.
    ///
    /// The `Instruction` referenced in the `TraceState` can be disassembled via [`Instruction::disassemble`]
    /// for display
    #[cfg(feature = "trace")]
    pub fn add_cpu_instruction_trace_hook(
        &mut self,
        func: Box<FnInstructionTraceHook>,
    ) -> HookHandle {
        self.trace_hooks.add_hook(func)
    }

    /// Remove a CPU instruction tracing hook
    #[cfg(feature = "trace")]
    pub fn remove_cpu_instruction_trace_hook(&mut self, handle: HookHandle) {
        self.trace_hooks.remove_hook(handle);
    }

    #[cfg(feature = "trace")]
    fn call_cpu_instruction_trace_hooks(&mut self) {
        let trace = &mut self.cpu.trace;
        if trace.last_hook_cycle_count == trace.cpu_clock {
            return;
        }
        trace.last_hook_cycle_count = trace.cpu_clock;

        let trace = self.cpu.trace.clone();
        if !self.trace_hooks.hooks.is_empty() {
            let mut hooks = std::mem::take(&mut self.trace_hooks);
            for hook in hooks.hooks.iter_mut() {
                (hook.func)(self, &trace);
            }
            std::mem::swap(&mut self.trace_hooks, &mut hooks);
        }
    }

    /// Returns the number of CPU clocks that will cover the given `duration`
    ///
    /// Note: this currently assumes we're emulating an NTSC NES!
    pub fn cpu_clocks_for_duration(&self, duration: Duration) -> u64 {
        const NANOS_PER_SEC: f64 = 1_000_000_000.0;

        let mut delta_clocks = duration.as_secs() * NTSC_CPU_CLOCK_HZ as u64;
        delta_clocks +=
            ((duration.subsec_nanos() as f64 / NANOS_PER_SEC) * NTSC_CPU_CLOCK_HZ as f64) as u64;

        delta_clocks
    }

    /// Based on the power on time for the NES, (or the last reference point that was saved) this
    /// calculates the number of (cpu) clock cycles that should have elapsed by the time we reach
    /// the `target_timestamp`
    pub fn cpu_clocks_for_time_since_power_cycle(&self, target_timestamp: Instant) -> u64 {
        let delta = target_timestamp - self.reference_timestamp;
        let delta_clocks = self.cpu_clocks_for_duration(delta);
        self.reference_cpu_clock + delta_clocks
    }

    /// Returns the PPU clock, derived from the current CPU clock
    ///
    /// This is the mapping that's used by the emulator to ensure the PPU is
    /// always caught up with the CPU clock which is treated like the main
    /// clock.
    ///
    /// Note: When the cpu clock = N then N cycles have really elapsed, since we
    /// always post-increment clocks for all units of the emulator. So at
    /// clock 0, zero cycles have actually elapsed.
    ///
    /// This function effectively returns the number of PPU clocks
    /// that should have elapsed for a given (post-incremented) CPU clock.
    /// E.g. after a single step of the CPU with a clock of 1, then the function
    /// would return 3. This is our target clock for the PPU, which is also post
    /// incremented.
    ///
    /// Note: this function effectively returns a floor() of the number of PPU clocks
    /// that should have elapsed for PAL (which runs a 3.2x the CPU clock) so
    /// that the PPU will never progress into the future based on this function.
    pub fn cpu_to_ppu_clock(&self, cpu_clock: u64) -> u64 {
        match self.model {
            Model::Ntsc => cpu_clock * 3,

            // Calculate clock * 3.2 in fixed point where decimals are divided
            // into 1000 units.  Do the calculation in integer arithmetic to be
            // sure of consistency across architectures (e.g. considering ARM
            // systems that don't natively support double precision, and also
            // just general consistency, considering the varying internal precision
            // for float calculations across systems)
            //
            // NB: we want the floor() value here to ensure the PPU isn't progressed
            // into the future, past the CPU clock. This is the opposite of the
            // ppu_to_cpu_clock function that will return the ceil()
            //
            // Note: we don't use a power-of-two fixed point representation
            // here, like 48.16 because 3.2 factors perfectly into 10s not 2s.
            //
            // A trade off with this is that the ppu_to_cpu_clock function (only
            // really used for debug visualization) needs to use use modulus
            // with non-power-of-two values instead of simply shifting but the
            // main concern here is consistency not performance.
            Model::Pal => cpu_clock * 3200 / 1000,
        }
    }

    /// Returns a function that can map a CPU clock to PPU clock
    ///
    /// This is like [`Self::cpu_to_ppu_clock`] but with the benefit of not
    /// needing to borrow `Nes` and it avoids repeatedly checking the [`Nes::model`]
    ///
    /// See: [`Self::cpu_to_ppu_clock`] for more details.
    pub fn cpu_to_ppu_clock_mapper(&self) -> impl Fn(u64) -> u64 {
        match self.model {
            Model::Ntsc => |clk: u64| clk * 3,
            Model::Pal => |clk: u64| clk * 3200 / 1000,
        }
    }

    /// Calculate the ceil() of a fixed-point number where the decimal is divided into
    /// units of 1/1000.
    #[inline(always)]
    fn fixed_1000x_ceil(fixed_val: u64) -> u64 {
        if fixed_val % 1000 == 0 {
            fixed_val / 1000
        } else {
            (fixed_val + 1000) / 1000
        }
    }

    /// Returns the CPU clock, derived from the current PPU clock
    ///
    /// This is consistent with the [`Self::cpu_to_ppu_clock`] function.
    ///
    /// Note: that while the [`Self::cpu_to_ppu_clock`] function effectively
    /// returns a `floor()` of the true value for PAL systems (where the PPU
    /// runs at 3.2x the CPU clock) to ensure the PPU isn't stepped into the
    /// future; this function returns the `ceil()` of the reverse mapping.
    ///
    /// This function will consistently represent where CPU cycles map to 4 PPU cycles
    /// instead of 3 for PAL timing where the PPU runs at 3.2x the CPU clock.
    ///
    /// This can be used to visualize the relationship between CPU and PPU cycles in
    /// debugging tools.
    pub fn ppu_to_cpu_clock(&self, ppu_clock: u64) -> u64 {
        match self.model {
            Model::Ntsc => ppu_clock / 3,

            // Calculate clock / 3.2 in fixed point where decimals are divided into 1000 units.
            //
            // Note: we want the ceil() value here, considering we get the floor() when mapping cpu
            // clocks to ppu clocks.
            Model::Pal => Self::fixed_1000x_ceil(ppu_clock * 1000 * 1000 / 3200),
        }
    }

    /// Returns a function that can map a CPU clock to PPU clock
    ///
    /// This is like [`Self::ppu_to_cpu_clock`] but with the benefit of not
    /// needing to borrow `Nes` and it avoids repeatedly checking the [`Nes::model`]
    ///
    /// See: [`Self::ppu_to_cpu_clock`] for more details.
    pub fn ppu_to_cpu_clock_mapper(&self) -> impl Fn(u64) -> u64 {
        match self.model {
            Model::Ntsc => |clk: u64| clk / 3,
            Model::Pal => |clk: u64| Self::fixed_1000x_ceil(clk * 1000 * 1000 / 3200),
        }
    }

    /// Account of any PPU clock drift, either due to having a a non-integer CPU:PPU clock ratio
    /// with PAL or due to PPU dot breakpoints that may have stalled the PPU for part of a CPU
    /// instruction
    ///
    /// Returns false if the catch up was aborted (e.g. if a a PPU dot breakpoint is hit)
    fn catch_up_ppu_drift(&mut self) -> bool {
        // When the cpu clock = N then N cycles have really elapsed, since we
        // always post-increment clocks for all units of the emulator. So at
        // clock 0, zero cycles have actually elapsed.
        //
        // The cpu_to_ppu_clock() function effectively returns the number of PPU clocks
        // that should have elapsed for a given (post-incremented) CPU clock.
        // E.g. after a single step of the CPU with a clock of 1, then the function
        // would return 3. This is our target clock for the PPU, which is also post
        // incremented.
        //
        // Note: cpu_to_ppu_clock() effectively returns a floor() of the number of PPU clocks
        // that should have elapsed for PAL (which runs a 3.2x the CPU clock) so
        // that the PPU will never progress into the future based on this function.
        let expected_ppu_clock = self.cpu_to_ppu_clock(self.cpu.clock);
        self.system.catch_up_ppu_drift(expected_ppu_clock)
    }

    #[cfg(feature = "nsf-player")]
    fn nsf_player_step(&mut self) {
        if let Some(ref config) = self.nsf_player.nsf_config {
            println!("Calling NSF play code");
            let play = config.play_address;
            self.system.cpu_write(0x5001, (play & 0xff) as u8);
            self.cpu.clock += 1;
            self.system.cpu_write(0x5002, ((play & 0xff00) >> 8) as u8);
            self.cpu.clock += 1;
            self.cpu.pc = 0x5000;
        } else {
            unreachable!();
        }
        self.nsf_player.nsf_last_step_cycle = self.cpu.clock;
    }

    #[cfg(feature = "nsf-player")]
    #[inline]
    fn nsf_player_progress(&mut self) {
        if self.nsf_player.nsf_config.is_some() {
            if self.cpu.pc == 0x5003 {
                self.nsf_player.nsf_waiting = true;
                if !self.nsf_player.nsf_initialized {
                    self.nsf_player.nsf_initialized = true;
                    log::debug!("Initialized NSF playback");
                }
            }

            #[allow(clippy::collapsible_if)]
            if self.nsf_player.nsf_initialized {
                if self.cpu.clock - self.nsf_player.nsf_last_step_cycle
                    > self.nsf_player.nsf_step_period
                    && self.nsf_player.nsf_waiting
                {
                    self.nsf_player_step();
                }
                //println!("progress = {} / {}", self.nsf_step_progress, self.nsf_step_period);
            }
        }
    }

    #[cfg(feature = "debugger")]
    fn clear_breakpoint_flags(&mut self) {
        self.cpu.debug.breakpoint_hit = false;
        self.system.debug.watch_hit = false;
        self.system.ppu.debug.breakpoint_hit = false;
    }

    #[cfg(feature = "debugger")]
    fn check_for_breakpoint(&mut self) -> bool {
        if self.cpu.debug.breakpoint_hit
            | self.system.debug.watch_hit
            | self.system.ppu.debug.breakpoint_hit
        {
            self.clear_breakpoint_flags();
            true
        } else {
            false
        }
    }

    /// Progresses the emulation of all the NES components, including the CPU, PPU and APU
    ///
    /// Note: if progress is paused by the user then [`Self::set_progress_time`] should
    /// be called when resuming to update the internal time for the emulator, so that
    /// it won't try and catch up for lost time (when using a [`ProgressTarget::Time`] target)
    pub fn progress(&mut self, target: ProgressTarget) -> ProgressStatus {
        //println!("NES: progress()");

        // We treat the CPU as our master clock and the PPU is driven according
        // to the forward progress of the CPU's clock.
        let cpu_clock_target = match target {
            ProgressTarget::Time(target_timestamp) => {
                self.cpu_clocks_for_time_since_power_cycle(target_timestamp)
            }
            ProgressTarget::Clock(clock) => clock,
            ProgressTarget::FrameReady => u64::MAX,
        };

        loop {
            // Let the PPU catch up with the CPU clock before progressing the CPU
            //
            // Note: if we abort the catch up due to hitting a PPU break point then
            // we will simply resume caching up when `progress()` is re-called later.
            self.catch_up_ppu_drift();

            #[cfg(feature = "debugger")]
            if self.check_for_breakpoint() {
                return ProgressStatus::Breakpoint;
            }

            if self.system.take_frame_ready() {
                self.system.port1.update_button_press_latches();
                self.system.port2.update_button_press_latches();
                return ProgressStatus::FrameReady;
            }

            #[cfg(feature = "trace")]
            {
                self.cpu.trace.ppu_line = self.ppu_mut().line;
                self.cpu.trace.ppu_dot = self.ppu_mut().dot;
            }
            self.cpu.step_instruction(&mut self.system);
            debug_assert_eq!(self.cpu.clock, self.system.apu_clock());

            #[cfg(feature = "nsf-player")]
            self.nsf_player_progress();

            #[cfg(feature = "trace")]
            self.call_cpu_instruction_trace_hooks();

            if self.cpu.clock >= cpu_clock_target {
                return ProgressStatus::ReachedTarget;
            }
        }
    }

    /// Set the current time that is referenced whenever a [`ProgrssTarget::Time`] target is given to [`Self::progress`]
    ///
    /// This should be called (when resuming) if the emulator is explicitly paused by the user to
    /// stop the emulator from trying to catch up for lost time.
    pub fn set_progress_time(&mut self, timestamp: Instant) {
        self.reference_cpu_clock = self.cpu.clock;
        self.reference_timestamp = timestamp;
    }

    /// Steps the CPU (and system) forward by a single instruction
    pub fn step_instruction_in(&mut self) {
        #[cfg(feature = "trace")]
        {
            self.cpu.trace.ppu_line = self.ppu_mut().line;
            self.cpu.trace.ppu_dot = self.ppu_mut().dot;
        }
        self.cpu.step_instruction(&mut self.system);
        debug_assert_eq!(self.cpu.clock, self.system.apu_clock());

        // Ignore break/watch points while single stepping
        #[cfg(feature = "debugger")]
        self.clear_breakpoint_flags();

        self.catch_up_ppu_drift();

        #[cfg(feature = "nsf-player")]
        self.nsf_player_progress();

        #[cfg(feature = "trace")]
        self.call_cpu_instruction_trace_hooks();
    }

    /// Creates a temporary breakpoint for stepping over an instruction
    ///
    /// Returns the address of the breakpoint which should be cleared when
    /// execution next stops.
    ///
    /// NB: It's possible a different breakpoint will be hit and so this
    /// should always be explicitly removed via [`Cpu::remove_break`]
    #[cfg(feature = "debugger")]
    pub fn add_tmp_step_over_breakpoint(&mut self) -> BreakpointHandle {
        let current_instruction = self.cpu.pc_peek_instruction(&mut self.system);
        let break_addr = self.cpu.pc.wrapping_add(current_instruction.len() as u16);
        self.cpu.add_break(
            break_addr,
            Box::new(|_cpu, _addr| BreakpointCallbackAction::Remove),
        )
    }

    /// Creates a temporary breakpoint for stepping out of a function
    ///
    /// Returns the address of the breakpoint which should be cleared when
    /// execution next stops. Will return None if no outer frame was found.
    ///
    /// NB: It's possible a different breakpoint will be hit and so this
    /// should always be explicitly removed via [`Cpu::remove_break`]
    #[cfg(feature = "debugger")]
    pub fn add_tmp_step_out_breakpoint(&mut self) -> Option<BreakpointHandle> {
        let out_addr = self
            .cpu
            .backtrace(&mut self.system)
            .next()
            .map(|frame| frame.0);
        if let Some(out_addr) = out_addr {
            Some(self.cpu.add_break(
                out_addr,
                Box::new(|_cpu, _addr| BreakpointCallbackAction::Remove),
            ))
        } else {
            None
        }
    }

    pub fn cpu_clock_hz(&self) -> u64 {
        self.cpu_clock_hz as u64
    }

    pub fn cpu_clocks_per_frame(&self) -> f32 {
        self.cpu_clocks_per_frame
    }

    pub fn cpu_clock(&self) -> u64 {
        self.cpu.clock
    }

    pub fn debug_sample_nametable(&mut self, x: usize, y: usize) -> [u8; 3] {
        self.system
            .ppu
            .peek_vram_four_screens(x, y, &mut self.system.cartridge)
    }

    #[cfg(feature = "ppu-sim")]
    pub fn set_ppu_sim_visible(&mut self, sim_visible: bool) {
        self.ppu_sim_visible = sim_visible;
    }
    #[cfg(feature = "ppu-sim")]
    pub fn ppu_sim_visible(&mut self) -> bool {
        self.ppu_sim_visible
    }

    pub fn peek_instruction(&mut self, addr: u16) -> (Instruction, FetchedOperand) {
        let opcode = self.system.peek(addr);
        let instruction = Instruction::from(opcode);
        let operand = self.cpu.peek_operand(
            &mut self.system,
            addr.wrapping_add(1),
            instruction.mode,
            instruction.oops_handling,
        );
        (instruction, operand)
    }

    pub fn backtrace(&mut self) -> Backtrace {
        self.cpu.backtrace(&mut self.system)
    }

    pub fn game_genie_codes(&self) -> &Vec<GameGenieCode> {
        self.system.game_genie_codes()
    }

    pub fn set_game_genie_codes(&mut self, codes: Vec<GameGenieCode>) {
        self.system.set_game_genie_codes(codes);
    }
}
