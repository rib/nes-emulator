use std::time::Duration;
use std::time::Instant;

use log::{warn};
use anyhow::anyhow;
use anyhow::Result;

use crate::apu::AudioOutput;
use crate::apu::apu::Apu;
use crate::binary;
use crate::binary::NesBinaryConfig;
use crate::binary::NsfConfig;
use crate::cartridge;
use crate::constants::*;
use crate::prelude::*;
use crate::system::*;
use crate::cpu::*;
use crate::ppu::*;

const NTSC_CPU_CLOCK_HZ: u32 = 1_789_166;

#[derive(Clone, Copy)]
pub enum ProgressTarget {
    /// Progress until the CPU clock reaches the given count
    Clock(u64),

    /// Progress up to the given wall clock time, relative to the [`Nes::poweron`] time
    Time(Instant)
}

pub enum ProgressStatus {
    FrameReady,
    ReachedTarget,
    Error
}

pub struct Nes {
    reference_timestamp: Instant,
    reference_cpu_clock: u64,

    pixel_format: PixelFormat,
    cpu: Cpu,
    cpu_clock: u64,
    ppu_clock: u64,
    apu_clock: u64,
    system: System,

    nsf_config: Option<NsfConfig>,
    nsf_initialized: bool,
    nsf_waiting: bool,
    nsf_step_period: u64,
    nsf_step_progress: u64,
    nsf_current_track: u8,

    last_clocks_per_second: u32,
}

impl Nes {
    pub fn new(pixel_format: PixelFormat, audio_sample_rate: u32) -> Nes {

        let cpu = Cpu::default();
        let mut ppu = Ppu::default();
        let mut apu = Apu::new(NTSC_CPU_CLOCK_HZ, audio_sample_rate);
        ppu.draw_option.fb_width = FRAMEBUFFER_WIDTH as u32;
        ppu.draw_option.fb_height = FRAMEBUFFER_HEIGHT as u32;
        ppu.draw_option.offset_x = 0;
        ppu.draw_option.offset_y = 0;
        ppu.draw_option.scale = 1;
        ppu.draw_option.pixel_format = pixel_format;

        let system = System::new(ppu, apu, Cartridge::none());
        Nes {
            reference_timestamp: Instant::now(),
            reference_cpu_clock: 0,

            pixel_format, cpu, cpu_clock: 0, ppu_clock: 0,
            apu_clock: 0, system,

            nsf_config: None,
            nsf_initialized: false,
            nsf_waiting: false,
            nsf_step_period: 0,
            nsf_step_progress: 0,
            nsf_current_track: 0,

            last_clocks_per_second: NTSC_CPU_CLOCK_HZ,
        }
    }

    pub fn open_binary(&mut self, binary: &[u8]) -> Result<()> {

        match binary::parse_any_header(binary)? {
            NesBinaryConfig::INes(ines_config) => {
                let cartridge = Cartridge::from_ines_binary(&ines_config, binary)?;
                self.insert_cartridge(Some(cartridge));
            }
            NesBinaryConfig::Nsf(nsf_config) => {
                let cartridge = Cartridge::from_nsf_binary(&nsf_config, binary)?;
                self.nsf_config = Some(nsf_config);
                self.insert_cartridge(Some(cartridge));
            }
        }

        Ok(())
    }

    pub fn insert_cartridge(&mut self, cartridge: Option<Cartridge>) {
        if let Some(cartridge) = cartridge {
            self.system.cartridge = cartridge;
        } else {
            self.system.cartridge = Cartridge::none();
        }
    }

    /// Assumes everything is Default initialized beforehand
    pub fn poweron(&mut self, start_timestamp: Instant) {
        debug_assert!(self.reference_cpu_clock == 0); // We don't currently support calling poweron more than once.
        self.reference_timestamp = start_timestamp;

        if let Some(ref nsf_config) = self.nsf_config {
            // TODO: handle PAL...
            self.nsf_step_period = ((nsf_config.ntsc_play_speed as u64 * NTSC_CPU_CLOCK_HZ as u64) / 1_000_000u64) as u64;
            self.nsf_step_progress = 0;

            // "1. Write $00 to all RAM at $0000-$07FF and $6000-$7FFF."
            // (already assumed to be the poweron state)

            // 2. Initialize the sound registers by writing $00 to $4000-$4013, and $00 then $0F to $4015.
            for i in 0..0x13 {
                self.system.write(0x4000 + i, 0x00);
            }
            self.system.write(0x4015, 0x00);
            self.system.write(0x4015, 0x0f);

            // 3. Initialize the frame counter to 4-step mode ($40 to $4017).
            self.system.write(0x4017, 0x40);

            // 4. If the tune is bank switched, load the bank values from $070-$077 into $5FF8-$5FFF.
            // (handled by Mapper 031)

            // 5. Set the A register for the desired song.
            let first_track = nsf_config.first_song - 1;
            self.cpu.a = first_track;

            // 6. Set the X register for PAL or NTSC.
            self.cpu.x = nsf_config.tv_system_byte;

            // 7. Call the music INIT routine.
            let init = nsf_config.init_address;
            self.system.write(0x5001, (init & 0xff) as u8);
            self.system.write(0x5002, ((init & 0xff00) >> 8) as u8);
            //self.cpu.add_break(0x5003, false); // break when we hit the infinite loop in the NSF bios
            self.nsf_initialized = false;
            self.nsf_current_track = first_track;
            self.cpu.pc = 0x5000;

            println!("Calling NSF init code: period = {}", self.nsf_step_period);

        } else {
            self.cpu.interrupt(&mut self.system, Interrupt::RESET);
        }
    }

    pub fn reset(&mut self) {
        self.cpu.p |= Flags::INTERRUPT;
        self.cpu.sp = self.cpu.sp.wrapping_sub(3);
        self.cpu.interrupt(&mut self.system, Interrupt::RESET);

        self.system.apu.reset();
    }

    pub fn system_mut(&mut self) -> &mut System {
        &mut self.system
    }
    pub fn system_cpu(&mut self) -> &mut Cpu {
        &mut self.cpu
    }
    pub fn system_ppu(&mut self) -> &mut Ppu {
        &mut self.system.ppu
    }
    pub fn system_apu(&mut self) -> &mut Apu {
        &mut self.system.apu
    }

    pub fn peek_system_bus(&mut self, addr: u16) -> u8 {
        self.system.peek(addr)
    }

    pub fn debug_read_ppu(&mut self, addr: u16) -> u8 {
        self.system.ppu.read(&mut self.system.cartridge, addr)
    }

    pub fn allocate_framebuffer(&self) -> Framebuffer {
        Framebuffer::new(FRAMEBUFFER_WIDTH, FRAMEBUFFER_HEIGHT, self.pixel_format)
    }

    // Aiming for Meson compatible trace format which can be used for cross referencing
    #[cfg(feature="trace")]
    fn display_trace(&self) {
        let trace = &self.cpu.trace;
        let pc = trace.instruction_pc;
        let op = trace.instruction_op_code;
        let operand_len = trace.instruction.len() - 1;
        let bytecode_str = if operand_len == 2 {
            let lsb = trace.instruction_operand & 0xff;
            let msb = (trace.instruction_operand & 0xff00) >> 8;
            format!("${op:02X} ${lsb:02X} ${msb:02X}")
        } else if operand_len == 1{
            format!("${op:02X} ${:02X}", trace.instruction_operand)
        } else {
            format!("${op:02X}")
        };
        let disassembly = trace.instruction.disassemble(trace.instruction_operand, trace.effective_address, trace.loaded_mem_value, trace.stored_mem_value);
        let a = trace.saved_a;
        let x = trace.saved_x;
        let y = trace.saved_y;
        let sp = trace.saved_sp & 0xff;
        let p = trace.saved_p.to_flags_string();
        let cpu_cycles = trace.cycle_count;
        println!("{pc:0X} {bytecode_str:11} {disassembly:23} A:{a:02X} X:{x:02X} Y:{y:02X} P:{p} SP:{sp:X} CPU Cycle:{cpu_cycles}");
    }

    /// Returns the number of CPU clocks that will cover the given `duration`
    ///
    /// Note: this currently assumes we're emulating an NTSC NES!
    pub fn cpu_clocks_for_duration(&self, duration: Duration) -> u64 {
        const NANOS_PER_SEC: f64 = 1_000_000_000.0;

        let mut delta_clocks = duration.as_secs() * NTSC_CPU_CLOCK_HZ as u64;
        delta_clocks += ((duration.subsec_nanos() as f64 / NANOS_PER_SEC) * NTSC_CPU_CLOCK_HZ as f64) as u64;

        delta_clocks
    }

    /// Based on the power on time for the NES, (or the last reference point that was saved) this
    /// calculates the number of (cpu) clock cycles that should have elapsed by the time we reach
    /// the `target_timestamp`
    pub fn cpu_clocks_for_time_since_poweron(&self, target_timestamp: Instant) -> u64 {
        let delta = target_timestamp - self.reference_timestamp;
        let delta_clocks = self.cpu_clocks_for_duration(delta);
        self.reference_cpu_clock + delta_clocks
    }

    pub fn progress(&mut self, target: ProgressTarget, mut framebuffer: Framebuffer) -> ProgressStatus {

        let cpu_clock_target = match target {
            ProgressTarget::Time(target_timestamp) => self.cpu_clocks_for_time_since_poweron(target_timestamp),
            ProgressTarget::Clock(clock) => clock
        };

        let rental = framebuffer.rent_data();
        if let Some(mut fb_data) = rental {
            let fb = fb_data.data.as_mut_ptr();
            loop {

                // We treat the CPU as our master clock and the PPU is driven according
                // to the forward progress of the CPU's clock.

                // For now just assuming NTSC which has an exact 1:3 ratio between cpu
                // clocks and PPU...
                let expected_ppu_clock = self.cpu_clock * 3;
                let ppu_delta = expected_ppu_clock - self.ppu_clock;

                let expected_apu_clock = self.cpu_clock;
                let apu_delta = expected_apu_clock - self.apu_clock;
                for _ in 0..apu_delta {
                    if let Some(DmcDmaRequest { address }) = self.system.apu.dmc_channel.step_dma_reader() {
                        self.system.dma_cpu_suspend_cycles += 4;
                        let dma_value = self.system.read(address);
                        self.system.apu.dmc_channel.completed_dma(address, dma_value);
                    }

                    self.system.step_apu();
                    self.apu_clock += 1;
                    if self.system.apu.irq() {
                        self.cpu.interrupt(&mut self.system, Interrupt::IRQ);
                    }
                }

                // Let the PPU catch up with the CPU clock before progressing the CPU
                // in case we need to quit to allow a redraw (so we will resume
                // catching afterwards)
                for _ in 0..ppu_delta {
                    let status = self.system.step_ppu(self.ppu_clock, fb);
                    self.ppu_clock += 1;
                    match status {
                        PpuStatus::None => { continue },
                        PpuStatus::FinishedFrame => {
                            self.system.pad1.update_button_press_latches();
                            self.system.pad2.update_button_press_latches();
                            return ProgressStatus::FrameReady;
                        },
                        PpuStatus::RaiseNmi => {
                            //println!("VBLANK NMI");
                            self.cpu.interrupt(&mut self.system, Interrupt::NMI);
                        }
                    }
                }

                let cyc = if self.system.dma_cpu_suspend_cycles == 0 {
                    let cyc = self.cpu.step(&mut self.system, self.cpu_clock) as u64;
                    if cyc == 0 && self.cpu.breakpoint_hit {
                        self.cpu.remove_break(self.cpu.pc, true);
                    }
                    cyc
                } else {
                    let cyc = self.system.dma_cpu_suspend_cycles as u64;
                    self.system.dma_cpu_suspend_cycles = 0;
                    cyc
                };
                self.cpu_clock += cyc;

                // TODO add feature check for nsf_player
                if self.nsf_config.is_some() {

                    if self.cpu.pc == 0x5003 {
                        self.nsf_waiting = true;
                        if !self.nsf_initialized {
                            self.nsf_initialized = true;
                            println!("Initialized NSF playback");
                        }
                    }

                    if self.nsf_initialized {
                        self.nsf_step_progress += cyc;
                        if self.nsf_step_progress > self.nsf_step_period && self.nsf_waiting {
                            self.nsf_player_step();
                        }
                        //println!("progress = {} / {}", self.nsf_step_progress, self.nsf_step_period);
                    }
                }

                #[cfg(feature="trace")]
                if cyc > 0 {
                    self.display_trace();
                }

                if self.cpu_clock >= cpu_clock_target {
                    return ProgressStatus::ReachedTarget;
                }
            }
        } else {
            warn!("Can't tick with framebuffer that's still in use!");
            return ProgressStatus::Error;
        }
    }

    fn nsf_player_step(&mut self) {
        if let Some(ref config) = self.nsf_config {
            println!("Calling NSF play code");
            let play = config.play_address;
            self.system.write(0x5001, (play & 0xff) as u8);
            self.system.write(0x5002, ((play & 0xff00) >> 8) as u8);
            self.cpu.pc = 0x5000;
        } else {
            unreachable!();
        }
        self.nsf_step_progress = 0;
    }

    pub fn cpu_clock_hz(&self) -> u64 {
        NTSC_CPU_CLOCK_HZ as u64
    }

    pub fn cpu_clocks_per_frame(&self) -> f32 {
        29780.5 // NTSC
    }

    pub fn cpu_clock(&self) -> u64 {
        self.cpu_clock
    }
}