use bitflags::bitflags;

use super::interface::*;
use super::system::System;
use super::cpu_instruction::{Instruction, Opcode, AddressingMode};

pub const CPU_FREQ: u32 = 1790000;
pub const NMI_READ_LOWER: u16 = 0xfffa;
pub const NMI_READ_UPPER: u16 = 0xfffb;
pub const RESET_READ_LOWER: u16 = 0xfffc;
pub const RESET_READ_UPPER: u16 = 0xfffd;
pub const IRQ_READ_LOWER: u16 = 0xfffe;
pub const IRQ_READ_UPPER: u16 = 0xffff;
pub const BRK_READ_LOWER: u16 = 0xfffe;
pub const BRK_READ_UPPER: u16 = 0xffff;

#[derive(PartialEq, Eq)]
pub enum Interrupt {
    NMI,
    RESET,
    IRQ,
    BRK,
}

#[derive(Clone)]
pub struct TraceState {
    pub cycle_count: u64,
    pub saved_a: u8,
    pub saved_x: u8,
    pub saved_y: u8,
    pub saved_sp: u8,
    pub saved_p: Flags,
    pub saved_cyc: u64,
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
            cycle_count: 8,
            saved_a: 0,
            saved_x: 0,
            saved_y: 0,
            saved_sp: 0,
            saved_p: Flags::NONE,
            saved_cyc: 0,
            instruction_pc: 0,
            instruction_op_code: 0,
            instruction: Instruction { op: Opcode::ASR, mode: AddressingMode::Immediate, cyc: 0},
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

#[derive(Clone)]
pub struct Cpu {
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
    pub trace: TraceState
}

impl Default for Cpu {
    fn default() -> Self {
        Self {
            a: 0,
            x: 0,
            y: 0,
            pc: 0,
            sp: 0xfd,
            p: unsafe { Flags::from_bits_unchecked(0x0) },

            #[cfg(feature="trace")]
            trace: TraceState::default()
        }
    }
}

impl EmulateControl for Cpu {
    fn poweron(&mut self) {
        self.a = 0;
        self.x = 0;
        self.y = 0;
        self.pc = 0;
        self.sp = 0xfd;
        self.p = unsafe { Flags::from_bits_unchecked(0x34) };

        #[cfg(feature="trace")]
        {
            self.trace = TraceState::default();
        }
    }
}

/// Control Functions Implementation
impl Cpu {
    pub fn increment_pc(&mut self, incr: u16) {
        self.pc = self.pc + incr;
    }
    pub fn stack_push(&mut self, system: &mut System, data: u8) {
        // data store
        system.write_u8(self.sp as u16 + 0x100, data);
        // decrement
        self.sp = self.sp.wrapping_sub(1);
    }

    pub fn stack_pop(&mut self, system: &mut System) -> u8 {
        // increment
        self.sp = self.sp.wrapping_add(1);
        // data fetch
        system.read_u8(self.sp as u16 + 0x100)
    }
    pub fn interrupt(&mut self, system: &mut System, irq_type: Interrupt) {
        match irq_type {
            Interrupt::NMI => {
                //self.write_break_flag(false);
                // Store PC Upper, Lower, Status Register in Stack
                self.stack_push(system, (self.pc >> 8) as u8);
                self.stack_push(system, (self.pc & 0xff) as u8);
                self.stack_push(system, (self.p | Flags::BREAK_HIGH).bits());
                self.write_interrupt_flag(true);
            }
            Interrupt::RESET => {
                self.write_interrupt_flag(true);
            }
            Interrupt::IRQ => {
                if self.p & Flags::INTERRUPT == Flags::INTERRUPT {
                    return;
                }
                //self.write_break_flag(false);
                // Store PC Upper, Lower, Status Register in Stack
                self.stack_push(system, (self.pc >> 8) as u8);
                self.stack_push(system, (self.pc & 0xff) as u8);

                self.stack_push(system, (self.p | Flags::BREAK_HIGH).bits());
                self.write_interrupt_flag(true);
            }
            Interrupt::BRK => {
                let ret_pc = self.pc + 1;
                // Store PC Upper, Lower, Status Register in Stack
                self.stack_push(system, (ret_pc >> 8) as u8);
                self.stack_push(system, (ret_pc & 0xff) as u8);
                self.stack_push(system, (self.p | Flags::BREAK_HIGH | Flags::BREAK_LOW).bits());
                self.write_interrupt_flag(true);
            }
        }

        // Update Program Counter
        let lower_addr = match irq_type {
            Interrupt::NMI => NMI_READ_LOWER,
            Interrupt::RESET => RESET_READ_LOWER,
            Interrupt::IRQ => IRQ_READ_LOWER,
            Interrupt::BRK => BRK_READ_LOWER,
        };
        let upper_addr = match irq_type {
            Interrupt::NMI => NMI_READ_UPPER,
            Interrupt::RESET => RESET_READ_UPPER,
            Interrupt::IRQ => IRQ_READ_UPPER,
            Interrupt::BRK => BRK_READ_UPPER,
        };

        let lower = system.read_u8(lower_addr);
        let upper = system.read_u8(upper_addr);
        self.pc = (lower as u16) | ((upper as u16) << 8);
    }
}
