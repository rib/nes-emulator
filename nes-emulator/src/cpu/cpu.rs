use bitflags::bitflags;

use crate::system::DmcDmaRequest;

use crate::system::System;

use super::instruction::{Instruction, Opcode, AddressingMode};



pub const CPU_FREQ: u32 = 1790000;
pub const NMI_READ_LOWER: u16 = 0xfffa;
pub const NMI_READ_UPPER: u16 = 0xfffb;
pub const RESET_READ_LOWER: u16 = 0xfffc;
pub const RESET_READ_UPPER: u16 = 0xfffd;
pub const IRQ_READ_LOWER: u16 = 0xfffe;
pub const IRQ_READ_UPPER: u16 = 0xffff;
pub const BRK_READ_LOWER: u16 = 0xfffe;
pub const BRK_READ_UPPER: u16 = 0xffff;

#[derive(PartialEq, Eq, Clone, Copy, Debug)]
pub enum Interrupt {
    NMI,
    RESET,
    IRQ,
    BRK,
}

#[derive(Clone, Debug)]
pub struct TraceState {
    pub last_display_cycle_count: u64,
    pub cycle_count: u64,
    pub saved_a: u8,
    pub saved_x: u8,
    pub saved_y: u8,
    pub saved_sp: u8,
    pub saved_p: Flags,
    pub instruction: Instruction,
    pub instruction_pc: u16,
    pub instruction_op_code: u8,
    pub instruction_operand: u16,
    pub effective_address: u16, // The effective address used for indirect addressing modes
    pub loaded_mem_value: u8, // The value loaded from the memory location referred to by the instruction
    pub stored_mem_value: u8, // The value stored at the memory location referred to by the instruction
}
impl Default for TraceState {
    fn default() -> Self {
        Self {
            last_display_cycle_count: 0,
            cycle_count: 0,
            saved_a: 0,
            saved_x: 0,
            saved_y: 0,
            saved_sp: 0,
            saved_p: Flags::NONE,
            instruction_pc: 0,
            instruction_op_code: 0,
            instruction: Instruction { op: Opcode::ASR, mode: AddressingMode::Immediate, cyc: 0, early_intr_poll: false },
            instruction_operand: 0,
            effective_address: 0,
            loaded_mem_value: 0,
            stored_mem_value: 0,
        }
    }
}

bitflags! {
    pub struct Flags: u8 {
        const CARRY         = 0b00000001;
        const ZERO          = 0b00000010;
        const INTERRUPT     = 0b00000100;
        const DECIMAL       = 0b00001000;
        const BREAK_LOW     = 0b00010000; // pushed to stack by PHP, BRK
        const BREAK_HIGH    = 0b00100000; // pushed to stack by PHP, BRK, /IRQ /NMI
        const OVERFLOW      = 0b01000000;
        const NEGATIVE      = 0b10000000;

        const REAL          = 0b11001111; // BREAK bits are only pushed to stack
        const NONE          = 0x0;
    }
}

impl Flags {
    pub fn to_flags_string(&self) -> String {
        let c = if *self & Flags::CARRY != Flags::NONE { "C" } else { "c" };
        let z = if *self & Flags::ZERO != Flags::NONE { "Z" } else { "z" };
        let i = if *self & Flags::INTERRUPT != Flags::NONE { "I" } else { "i" };
        let d = if *self & Flags::DECIMAL != Flags::NONE { "D" } else { "d" };
        let v = if *self & Flags::OVERFLOW != Flags::NONE { "V" } else { "v" };
        let n = if *self & Flags::NEGATIVE != Flags::NONE { "N" } else { "n" };
        format!("{n}{v}--{d}{i}{z}{c}")
    }
}

/*
/// Key points within an OAM DMA that may affect DMC cycle stealing
#[derive(Debug)]
enum OamDmaProgress {
    /// Not handling an OAM DMA
    None,

    SecondToLast,
}
*/

#[derive(Clone, Debug)]
pub struct Breakpoint {
    pub addr: u16,

    /// automatically remove breakpoint when hit
    pub temp: bool
}

#[derive(Clone)]
pub struct Cpu {
    pub clock: u64,

    /// Represents the `RDY`, "input ready" input which will cause the
    /// CPU to halt during a read cycle if it's low (false)
    /// Whenever a DMA is required (either OAM or DMC) then this
    /// will be pulled low (set false) and once we halt in the next
    /// read cycle we will step the DMA unit to service any pending
    /// OAM or DMC request
    input_ready: bool,

    /// OAM DMA request to be picked up by the DMA unit when halted
    oam_dma_pending: Option<u16>,

    // INTERRUPTS:
    //
    // NB: the journey of an interrupt looks like
    // 1) _phase2 / φ2: edge/level detector sets pending_*_detected
    // 2) _phase1 / φ1: internal signal is raised after interrupt detected (*_raised = true)
    // 3) instruction poll: instruction queues interrupt handling (after current instruction) if poll finds raised interrupt signal
    // 4) step_instruction: runs handle_interrupt if queued

    /// Previous state of NMI input For NMI edge detector
    last_nmi_level: bool,
    /// Set by NMI edge detector in _phase2
    pending_nmi_detected: bool,
    /// Set by IRQ level detector in _phase2
    pending_irq_detected: bool,
    /// Has an NMI been raised in phase 1 after edge detection?
    nmi_raised: bool,
    /// Has en IRQ been raised in phase 1 after level detection?
    irq_raised: bool,
    /// Has interrupt polling queued an interrupt service routine before the
    /// the next instruction?
    pub(super) interrupt_handler_pending: Option<Interrupt>,
    /// interrupt polling is disabled while dispatching an interrupt
    /// (i.e. during `handle_interrupt`) but not set while the interrupt
    /// routine itself is running
    interrupt_polling_disabled: bool,

    /// For asserting that every instruction polls for interrupts at least
    /// once
    #[cfg(debug_assertions)]
    pub(super) instruction_polled_interrupts: bool,

    /// So we don't lose track of how many cycles the current instruction
    /// has taken we count the cycles for OAM DMAs or DMC cycle stealing
    /// separately
    #[cfg(debug_assertions)]
    pub non_instruction_cycles: u32,

    /// Accumulator
    pub a: u8,
    /// Index Register
    pub x: u8,
    /// Index Register
    pub y: u8,
    /// Program Counter
    pub pc: u16,
    /// Stack Pointer
    pub sp: u8,
    /// Processor Status Register
    pub p: Flags,

    #[cfg(feature="trace")]
    pub trace: TraceState,

    /// Set to true to continue without re-breaking on the
    /// current instruction (automatically cleared when
    /// the pc updates)
    pub breakpoints_paused: bool,
    pub breakpoints: Vec<Breakpoint>,
    pub breakpoint_hit: bool,
}

impl Default for Cpu {
    fn default() -> Self {
        Self {
            clock: 0,

            last_nmi_level: false,
            pending_nmi_detected: false,
            pending_irq_detected: false,
            nmi_raised: false,
            irq_raised: false,
            interrupt_handler_pending: None,
            interrupt_polling_disabled: false,
            #[cfg(debug_assertions)]
            instruction_polled_interrupts: false,

            #[cfg(debug_assertions)]
            non_instruction_cycles: 0,

            input_ready: true,
            oam_dma_pending: None,

            a: 0,
            x: 0,
            y: 0,
            pc: 0,
            sp: 0xfd,
            p: unsafe { Flags::from_bits_unchecked(0x34) },

            #[cfg(feature="trace")]
            trace: TraceState::default(),

            breakpoints_paused: false,
            breakpoints: vec![],
            breakpoint_hit: false,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum DmcDmaState {
    None,
    Stall,
    Read // Handles alignment if needed
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum OamDmaState {
    None,
    Read, // Handles alignment if needed
    Write
}

impl Cpu {

    /// Handles OAM and DMC DMA requests with pedantic handling of cycle stealing
    fn run_dma_unit(&mut self, system: &mut System, dummy_addr: u16) {

        debug_assert_eq!(self.input_ready, false); // Make sure we aren't recursing somehow

        //println!("Start running DMA unit on clock = {}", self.clock);

        // Ref: https://archive.nes.science/nesdev-forums/f3/t14120.xhtml

        // We handle dummy reads to 4016/7 carefully so that back-to-back reads
        // will actually be coalesced (since the controller hardware
        // isn't aware of the cpu clock then back-to-back reads just look like
        // long reads)
        let dummy_read_coalesce = if dummy_addr == 0x4016 || dummy_addr == 0x4017 { true } else { false };

        // To coalesce dummy reads we track the last read address, and this will
        // also be cleared to zero on writes
        let mut last_read_addr = dummy_addr;

        // OAM DMA requests can't arrive while the DMA unit is servicing a DMA request
        // so we can just check for a pending request once
        let (mut oam_dma_state, oam_dma_addr) = match std::mem::take(&mut self.oam_dma_pending) {
            Some(addr) => (OamDmaState::Read, addr),
            None => (OamDmaState::None, 0)
        };
        let mut oam_dma_value = 0u8;
        let mut oam_dma_offset = 0;

        let mut dmc_dma_state = DmcDmaState::None;
        let mut dmc_dma_addr = 0u16;

        // Iterate one clock cycle at a time until pending DMA[s] completed
        while oam_dma_state != OamDmaState::None || dmc_dma_state != DmcDmaState::None {
            self.start_clock_cycle_phi1(system);
            // DMC DMA requests can arrive in the middle of an OAM DMA and DMC reads have higher priority
            if let Some(request) = std::mem::take(&mut system.dmc_dma_request) {
                dmc_dma_state = DmcDmaState::Stall;
                dmc_dma_addr = request.address;
            }

            // Every case must:
            // - perform a single read or write and step the system for a single cycle
            // - make sure to update `last_read_addr` (reset to zero for writes)
            // - update the state for each DMA as needed
            //
            // Note that in the case of a parallel OAM and DMC DMA then the stall and
            // alignment cycles for the DMC DMA can be accounted for with OAM cycles
            // without needing a dummy read.
            //
            // Note: we don't have separate `Align` states and instead handle alignment
            // during the read state. This ensures that the OAM DMA will correctly
            // re-align it's read if it gets interrupted by a DMC read.
            //
            match (oam_dma_state, dmc_dma_state) {
                (OamDmaState::None, DmcDmaState::None) => unreachable!(), // We'll exit loop in this case
                (OamDmaState::None, DmcDmaState::Stall) => {
                    if dummy_read_coalesce == false || last_read_addr != dummy_addr {
                            //println!("Stepping system for dummy read during DMA, clock = {}", self.clock);
                        let _discard = system.cpu_read(dummy_addr, self.clock); // will call .step_for_cpu_cycle()
                        last_read_addr = dummy_addr;
                    } else {
                        //println!("Stepping system for coalesced dummy read during DMA, clock = {}", self.clock);
                        system.step_for_cpu_cycle(); // Coalesced dummy read
                    }
                    dmc_dma_state = DmcDmaState::Read;
                }
                (OamDmaState::None, DmcDmaState::Read) |
                (OamDmaState::Read, DmcDmaState::Read) => { // DMC DMA reads take priority over OAM DMA reads
                    if self.clock % 2 == 1 { // DMC and OAM DMA only read on even cycles
                        if dummy_read_coalesce == false || last_read_addr != dummy_addr {
                            //println!("Stepping system for dummy read during DMA, clock = {}", self.clock);
                            let _discard = system.cpu_read(dummy_addr, self.clock); // will call .step_for_cpu_cycle()
                            last_read_addr = dummy_addr;
                        } else {
                            //println!("Stepping system for coalesced dummy read during DMA, clock = {}", self.clock);
                            system.step_for_cpu_cycle(); // Coalesced dummy read
                        }
                    } else {
                        let sample = system.cpu_read(dmc_dma_addr, self.clock); // will call .step_for_cpu_cycle()
                        last_read_addr = dmc_dma_addr;
                        system.apu.dmc_channel.completed_dma(dmc_dma_addr, sample);
                        dmc_dma_state = DmcDmaState::None;
                    }
                }
                (OamDmaState::Read, DmcDmaState::None) |
                (OamDmaState::Read, DmcDmaState::Stall) => {
                    if self.clock % 2 == 1 { // OAM DMA only reads on even cycles
                        if dummy_read_coalesce == false || last_read_addr != dummy_addr {
                            //println!("Stepping system for dummy read during DMA, clock = {}", self.clock);
                            let _discard = system.cpu_read(dummy_addr, self.clock); // will call .step_for_cpu_cycle()
                            last_read_addr = dummy_addr;
                        } else {
                            //println!("Stepping system for coalesced dummy read during DMA, clock = {}", self.clock);
                            system.step_for_cpu_cycle(); // Coalesced dummy read
                        }
                    } else {
                        let dma_addr = oam_dma_addr.wrapping_add(oam_dma_offset);
                        oam_dma_value = system.cpu_read(dma_addr, self.clock);
                        //println!("OAM DMA: reading {dma_addr:04x} = {oam_dma_value:02x},  offset = {}, clock = {}", oam_dma_offset, self.clock);
                        last_read_addr = dma_addr;
                        oam_dma_state = OamDmaState::Write;
                    }
                    if dmc_dma_state == DmcDmaState::Stall {
                        dmc_dma_state = DmcDmaState::Read;
                    }
                }
                (OamDmaState::Write, _) => {
                    debug_assert_eq!(self.clock % 2, 1);

                    //println!("OAM DMA: writing {oam_dma_value:02x} to $2004, offset = {}, clock = {}", oam_dma_offset, self.clock);
                    system.cpu_write(0x2004 /* OAMDATA */, oam_dma_value, self.clock);
                    last_read_addr = 0;

                    oam_dma_offset += 1;
                    if oam_dma_offset == 256 {
                        oam_dma_state = OamDmaState::None;
                    } else {
                        oam_dma_state = OamDmaState::Read;
                    }

                    if dmc_dma_state == DmcDmaState::Stall {
                        dmc_dma_state = DmcDmaState::Read;
                    }
                }
            }

            self.end_clock_cycle_phi2(system);

            self.clock += 1;

            #[cfg(debug_assertions)]
            {
                self.non_instruction_cycles += 1;
            }
        }

        // Raise the RDY line high again to un-halt the CPU from its original read
        self.input_ready = true;
        //println!("Finished running DMA unit");
    }

    /// Halt the CPU if the RDY line is pulled low.
    ///
    /// This will run the DMA unit which uses RDY to suspend the CPU and the last read
    /// will effectively be repeated for any dummy cycle needed while servicing the DMA.
    #[inline]
    fn handle_RDY_halt(&mut self, system: &mut System, addr: u16) -> u8 {
        //println!("CPU Halt");

        let dummy_addr = addr;

        // Count the that read to the halt read as a 'dummy' read that's not associated with
        // the current instruction
        #[cfg(debug_assertions)]
        {
            self.non_instruction_cycles += 1;
        }

        self.run_dma_unit(system, dummy_addr);

        //println!("Finished DMA @ clock = {}", self.clock);

        debug_assert_eq!(self.input_ready, true); // Shouldn't be possible to queue more DMAs yet

        // Repeat the original read and continue the current instruction...
        let data = self.read_system_bus(system, addr);

        //println!("Finished original read that was halted @ clock = {}", self.clock);
        data
    }

    /// A NOP read optimization for various superfluous reads that the CPU does (such as
    /// reading the non-existent op code for implied/accumulator addressing mode instructions)
    /// or when an address crosses a page boundary.
    ///
    /// This skips doing the actual read but still steps the system for one CPU clock cycle.
    ///
    /// If the CPU is halted by the RDY line the address is given for performing any required
    /// dummy reads while the DMA unit is running
    ///
    /// "read" the system bus at the given address, but don't _actually_ do the read
    pub(in super) fn nop_read_system_bus(&mut self, system: &mut System, addr: u16) {
        self.start_clock_cycle_phi1(system);
        system.step_for_cpu_cycle();
        self.end_clock_cycle_phi2(system);

        self.clock += 1;

        if !self.input_ready {
            let _data = self.handle_RDY_halt(system, addr);
        }
    }

    pub(in super) fn read_system_bus(&mut self, system: &mut System, addr: u16) -> u8 {
        // The reads/writes by the CPU effectively correspond to clock cycles
        // so this is a convenient place to run the interrupt detection that
        // happens during phase 1/2 of each clock cycle

        self.start_clock_cycle_phi1(system);
        let mut data = system.cpu_read(addr, self.clock);
        self.end_clock_cycle_phi2(system);

        self.clock += 1;

        if !self.input_ready {
            data = self.handle_RDY_halt(system, addr);
        }

        data
    }

    /// A NOP write optimization for various superfluous writes that the CPU does
    ///
    /// This skips doing the actual write but still steps the system for one CPU clock cycle.
    pub(in super) fn nop_write_system_bus(&mut self, system: &mut System, addr: u16, data: u8) {
        self.start_clock_cycle_phi1(system);
        system.step_for_cpu_cycle();
        self.end_clock_cycle_phi2(system);

        self.clock += 1;
    }

    pub(in super) fn write_system_bus(&mut self, system: &mut System, addr: u16, data: u8) {

        // The reads/writes by the CPU effectively correspond to clock cycles
        // so this is a convenient place to run the interrupt detection that
        // happens during phase 1/2 of each clock cycle

        #[cfg(feature="trace")]
        {
            self.trace.stored_mem_value = data;
        }

        self.start_clock_cycle_phi1(system);
        system.cpu_write(addr, data, self.clock);

        // We treat the OAMDMA register as a special, internal register
        // so we can neatly control how we suspend/halt the CPU mid-instruction
        // to service DMA requests
        if addr == 0x4014 {
            // Note that we don't stop the redundant write to the system
            // bus above via system.cpu_write() in case there are debug
            // features enabled, such as for tracing memory writes.

            debug_assert!(self.oam_dma_pending.is_none());
            self.oam_dma_pending = Some((data as u16) << 8);

            // Pull the RDY line low, so the CPU will halt on the next
            // read and then the DMA unit can service the pending DMA
            self.input_ready = false;
        }

        self.end_clock_cycle_phi2(system);
        self.clock += 1;
    }

    pub(in super) fn stack_push(&mut self, system: &mut System, data: u8) {
        self.write_system_bus(system, self.sp as u16 + 0x100, data);
        self.sp = self.sp.wrapping_sub(1);
    }

    pub(in super) fn stack_pop(&mut self, system: &mut System) -> u8 {
        self.sp = self.sp.wrapping_add(1);
        self.read_system_bus(system, self.sp as u16 + 0x100)
    }

    pub fn handle_interrupt(&mut self, system: &mut System, interrupt: Interrupt) {

        // "The interrupt sequences themselves do not perform interrupt polling, meaning at least one instruction
        // from the interrupt handler will execute before another interrupt is serviced."
        self.interrupt_polling_disabled = true;

        self.interrupt_handler_pending = None;

        let vector = match interrupt {
            Interrupt::NMI => {
                //println!("Handling NMI");

                // "The internal signal goes high during φ1 of the cycle that follows the one where the edge is detected,
                // and stays high until the NMI has been handled"
                self.nmi_raised = false;
                self.pending_nmi_detected = false;

                self.stack_push(system, (self.pc >> 8) as u8);
                self.stack_push(system, (self.pc & 0xff) as u8);
                self.stack_push(system, (self.p | Flags::BREAK_HIGH).bits());
                self.set_interrupt_flag(true);

                Interrupt::NMI
            }
            Interrupt::RESET => {
                log::debug!("CPU: reset interrupt");

                self.set_interrupt_flag(true);

                Interrupt::RESET
            }
            Interrupt::IRQ => {
                //println!("Handling IRQ");
                self.stack_push(system, (self.pc >> 8) as u8);
                self.stack_push(system, (self.pc & 0xff) as u8);

                // "*** At this point, the signal status determines which interrupt vector is used ***"
                // (I.e. the interrupt may be hijacked)
                let vector = match self.interrupt_detector_status() {
                    Some(vector) => vector,
                    None => Interrupt::IRQ
                };

                self.stack_push(system, (self.p | Flags::BREAK_HIGH).bits());
                self.set_interrupt_flag(true);

                vector
            }
            Interrupt::BRK => {
                //println!("BRK2: pending NMI = {}, raised NMI = {}, handler pending = {:?}", self.pending_nmi_detected, self.nmi_raised, self.interrupt_handler_pending);
                self.stack_push(system, (self.pc >> 8) as u8);

                //println!("BRK3: pending NMI = {}, raised NMI = {}, handler pending = {:?}", self.pending_nmi_detected, self.nmi_raised, self.interrupt_handler_pending);
                self.stack_push(system, (self.pc & 0xff) as u8);

                // "*** At this point, the signal status determines which interrupt vector is used ***"
                // (I.e. the interrupt may be hijacked)
                let vector = match self.interrupt_detector_status() {
                    Some(vector) => vector,
                    None => Interrupt::BRK
                };
                //println!("BRK vector = {:?}", vector);

                //println!("BRK4: pending NMI = {}, raised NMI = {}, handler pending = {:?}", self.pending_nmi_detected, self.nmi_raised, self.interrupt_handler_pending);
                self.stack_push(system, (self.p | Flags::BREAK_HIGH | Flags::BREAK_LOW).bits());
                self.set_interrupt_flag(true);

                vector
            }
        };

        //println!("Interrupt vector = {vector:?}");
        let (lower_addr, upper_addr) = match vector {
            Interrupt::NMI => (NMI_READ_LOWER, NMI_READ_UPPER),
            Interrupt::RESET => (RESET_READ_LOWER, RESET_READ_UPPER),
            Interrupt::IRQ => (IRQ_READ_LOWER, IRQ_READ_UPPER),
            Interrupt::BRK => (BRK_READ_LOWER, BRK_READ_UPPER),
        };
        //println!("vector address lo ={lower_addr:04x}, hi = {upper_addr:04x}");
        //println!("BRK5: pending NMI = {}, raised NMI = {}, handler pending = {:?}", self.pending_nmi_detected, self.nmi_raised, self.interrupt_handler_pending);
        let lower = self.read_system_bus(system, lower_addr);
        //println!("BRK6: pending NMI = {}, raised NMI = {}, handler pending = {:?}", self.pending_nmi_detected, self.nmi_raised, self.interrupt_handler_pending);
        let upper = self.read_system_bus(system, upper_addr);
        self.pc = (lower as u16) | ((upper as u16) << 8);

        //println!("BRK interrupt handler set PC = ${:04x}", self.pc);

        self.interrupt_polling_disabled = false;
    }

    /// Poll the status of interrupt detection that happened during phase 1 of this cycle
    pub(in super) fn instruction_poll_interrupts(&mut self) {
        #[cfg(debug_assertions)]
        {
            self.instruction_polled_interrupts = true;
        }

        // "The interrupt sequences themselves do not perform interrupt polling, meaning at least one
        // instruction from the interrupt handler will execute before another interrupt is serviced."
        if self.interrupt_polling_disabled {
            //println!("Interrupt polling disabled");
            return;
        }

        // If we find that there is a pending BRK handler then we shouldn't
        // override it here (that means we're handling a BRK instruction). In
        // this case the handler will anyway get hijacked by the NMI or IRQ
        // but we also need to set the B status flag from the BRK instruction.

        match self.interrupt_detector_status() {
            Some(Interrupt::NMI) => {
                if self.interrupt_handler_pending.is_none() {
                    //println!("interrupt poll: queue NMI handler");
                    self.interrupt_handler_pending = Some(Interrupt::NMI);
                } else {
                    //println!("interrupt poll: leaving BRK handler queued");
                }
            }
            Some(Interrupt::IRQ) => {
                if self.p & Flags::INTERRUPT == Flags::INTERRUPT {
                    //println!("interrupt poll: Ignoring IRQ due to interrupt flag");
                    return;
                } else {
                    if self.interrupt_handler_pending.is_none() {
                        //println!("interrupt poll: queue IRQ handler");
                        self.interrupt_handler_pending = Some(Interrupt::IRQ);
                    } else {
                        //println!("interrupt poll: leaving BRK handler queued");
                    }
                }
            }
            Some(_) => unreachable!(),
            None => {
                //println!("interrupt poll: nothing found");
            }
        }
    }

    /// Check the status of the interrupt detector, updated during phase 1 of this cycle
    pub(in super) fn interrupt_detector_status(&self) -> Option<Interrupt> {
        if self.nmi_raised {
            Some(Interrupt::NMI)
        } else if self.irq_raised {
            Some(Interrupt::IRQ)
        } else {
            None
        }
    }

    /// Checks the status of the edge/level detector during φ1/phi1 (first half) of a cycle to determine if an
    /// interrupt has been detected.
    /// Note: this phase 1 state still needs to be polled before an interrupt will actually be handled
    fn step_interrupt_detector_phi1(&mut self) {
        if self.pending_nmi_detected {
            // Note this will then stay set until "the NMI has been handled"
            self.nmi_raised = true;
            //println!("Phase 1 raised NMI interrupt")
        }
        //println!("Phase 1 raised NMI interrupt = {}", self.nmi_detected);
        self.irq_raised = self.pending_irq_detected;
        //println!("phase 1 irq_raised = {}", self.irq_raised);
    }

    /// Handle anything specific to the first half of the clock cycle, aka φ1/phi1
    pub(in super) fn start_clock_cycle_phi1(&mut self, _system: &System) {
        self.step_interrupt_detector_phi1();
    }

    /// Checks interrupt lines during φ2/phi2 (second half) of a cycle to detect NMI edges or level IRQ inputs
    fn step_interrupt_detector_phi2(&mut self, system: &System) {
        let nmi_level = system.nmi_line();
        if nmi_level == true && self.last_nmi_level == false {
            // Note this will then stay set until "the NMI has been handled"
            self.pending_nmi_detected = true;
            //println!("Phase 2 detected NMI interrupt")
        }
        //println!("prev nmi = {}, nmi = {}, detected = {}", self.last_nmi_level, nmi_level, self.pending_nmi_detected);
        self.last_nmi_level = nmi_level;
        self.pending_irq_detected = system.irq_line();
        //println!("phase 2 pending_irq_detected = {}", self.pending_irq_detected);
    }

    /// Handle anything specific to the second half of the clock cycle, aka φ2/phi2
    pub(in super) fn end_clock_cycle_phi2(&mut self, system: &mut System) {
        self.step_interrupt_detector_phi2(system);
        system.cartridge.step_m2_phi2(self.clock);
    }

    pub fn add_break(&mut self, addr: u16, temp: bool) {
        if !temp { // Allow multiple temporaries, but only one permanent
            self.remove_break(addr, false);
        }
        self.breakpoints.push(Breakpoint {
            addr,
            temp
        })
    }

    pub fn remove_break(&mut self, addr: u16, temp: bool) {
        if temp {
            loop { // There may be multiple temporary breakpoints
                if let Some(i) = self.breakpoints.iter().position(|b| b.temp == true && b.addr == addr) {
                    self.breakpoints.swap_remove(i);
                } else {
                    break;
                }
            }
        } else {
            if let Some(i) = self.breakpoints.iter().position(|b| b.addr == addr) {
                self.breakpoints.swap_remove(i);
            }
        }
    }
}
