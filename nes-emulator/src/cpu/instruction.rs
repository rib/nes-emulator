use super::cpu::*;
use crate::system::System;

#[derive(PartialEq, Eq, Copy, Clone, Debug)]
pub enum Opcode {
    // binary op
    ADC,
    SBC,
    AND,
    EOR,
    ORA,
    // shift/rotate
    ASL,
    LSR,
    ROL,
    ROR,
    // inc/dec
    INC,
    INX,
    INY,
    DEC,
    DEX,
    DEY,
    // load/store
    LDA,
    LDX,
    LDY,
    STA,
    STX,
    STY,
    // set/clear flag
    SEC,
    SED,
    SEI,
    CLC,
    CLD,
    CLI,
    CLV,
    // compare
    CMP,
    CPX,
    CPY,
    // jump return
    JMP,
    JSR,
    RTI,
    RTS,
    // branch
    BCC,
    BCS,
    BEQ,
    BMI,
    BNE,
    BPL,
    BVC,
    BVS,
    // push/pop
    PHA,
    PHP,
    PLA,
    PLP,
    // transfer
    TAX,
    TAY,
    TSX,
    TXA,
    TXS,
    TYA,
    // other
    BRK,
    BIT,
    NOP,
    // unofficial1
    // https://wiki.nesdev.com/w/index.php/Programming_with_unofficial_opcodes
    AAC,
    AAX,
    ARR,
    ASR,
    ATX,
    AXA,
    AXS,
    DCP,
    DOP,
    ISC,
    LAR,
    LAX,
    RLA,
    RRA,
    SLO,
    SRE,
    SXA,
    SYA,
    TOP,
    XAA,
    XAS,

    HALT, // Invalid op codes
}

#[derive(PartialEq, Eq, Copy, Clone, Debug)]
pub enum AddressingMode {
    Implied,
    Accumulator,
    Immediate,
    Absolute,
    ZeroPage,
    ZeroPageX,
    ZeroPageY,
    AbsoluteX,
    AbsoluteY,
    Relative,
    AbsoluteIndirect, // Only used with JMP
    IndirectX,
    IndirectY,
}

#[derive(PartialEq, Eq, Clone, Copy, Debug)]
enum OopsHandling {
    /// For applicable addressing modes, do an extra read if the operand address crosses a page boundary
    Normal,

    /// Don't do any extra reads if the operand address crosses a page boundary
    ///
    /// For branch instructions we figure out the branch status before we
    /// fetch the operand and we don't take the oops penalty if we aren't
    /// branching
    Ignore,

    /// Always do an 'oops' read for applicable addressing modes
    ///
    /// This is typically done for write instructions that must be sure to
    /// fixup their write address before they write (because, a write can't
    /// be undone)
    ///
    /// Note: this will only force an oops read for addressing modes where
    /// this is applicable.
    Always
}

#[derive(Copy, Clone)]
struct FetchedOperand {
    /// The raw operand associated with the instruction, such as an
    /// immediate value, zero page offset or absolute address
    #[allow(dead_code)]
    pub raw_operand: u16,

    /// The effective/decoded operand, after handling any offsets and indirection
    pub operand: u16,

    /// The number of clock cycles that it took to fetch the
    /// effective operand
    pub oops_cyc: u8
}

#[derive(Copy, Clone, Debug)]
pub struct Instruction {
    pub op: Opcode,
    pub mode: AddressingMode,

    /// Base number of cycles without 'oops' cycles from fetching across page boundaries
    ///
    /// This cycle count is primary used for accuracy validation by
    /// cross-referencing the expected cycle count (calculated from this and the
    /// number of opps cycles) and the actual cycle count (calculated from the
    /// number of reads/writes associated with the instruction)
    pub cyc: u8,

    /// Does the instruction poll for interrupts after fetching the OpCode?
    ///
    /// In general the hardware differentiates two-cycle instructions and polls for
    /// interrupts after the first op code fetch but there are a few special cases to consider:
    /// - PLP is a 4 cycle instruction that also polls interrupts early
    /// - BRK is a 7 cycle instruction that is declared as a 2-cycle instruction
    ///   in the table below due to how the last 5 cycles happen as part of
    ///   self.handle_interrupt() which affects how we cross-reference / verify
    ///   that are read/write cycle count is consistent with the expected
    ///   cycle count declared here.
    pub early_intr_poll: bool,
}

impl Instruction {
    /// Convert rom code into instructions
    pub fn from(inst_code: u8) -> Instruction {
        match inst_code {
            /* *************** binary op ***************  */
            0x69 => Instruction { op: Opcode::ADC, mode: AddressingMode::Immediate, cyc: 2, early_intr_poll: true, },
            0x65 => Instruction { op: Opcode::ADC, mode: AddressingMode::ZeroPage, cyc: 3, early_intr_poll: false, },
            0x75 => Instruction { op: Opcode::ADC, mode: AddressingMode::ZeroPageX, cyc: 4, early_intr_poll: false, },
            0x6d => Instruction { op: Opcode::ADC, mode: AddressingMode::Absolute, cyc: 4, early_intr_poll: false, },
            0x7d => Instruction { op: Opcode::ADC, mode: AddressingMode::AbsoluteX, cyc: 4, early_intr_poll: false, },
            0x79 => Instruction { op: Opcode::ADC, mode: AddressingMode::AbsoluteY, cyc: 4, early_intr_poll: false, },
            0x61 => Instruction { op: Opcode::ADC, mode: AddressingMode::IndirectX, cyc: 6, early_intr_poll: false, },
            0x71 => Instruction { op: Opcode::ADC, mode: AddressingMode::IndirectY, cyc: 5, early_intr_poll: false, },

            0xe9 => Instruction { op: Opcode::SBC, mode: AddressingMode::Immediate, cyc: 2, early_intr_poll: true, },
            0xe5 => Instruction { op: Opcode::SBC, mode: AddressingMode::ZeroPage, cyc: 3, early_intr_poll: false, },
            0xf5 => Instruction { op: Opcode::SBC, mode: AddressingMode::ZeroPageX, cyc: 4, early_intr_poll: false, },
            0xed => Instruction { op: Opcode::SBC, mode: AddressingMode::Absolute, cyc: 4, early_intr_poll: false, },
            0xfd => Instruction { op: Opcode::SBC, mode: AddressingMode::AbsoluteX, cyc: 4, early_intr_poll: false, },
            0xf9 => Instruction { op: Opcode::SBC, mode: AddressingMode::AbsoluteY, cyc: 4, early_intr_poll: false, },
            0xe1 => Instruction { op: Opcode::SBC, mode: AddressingMode::IndirectX, cyc: 6, early_intr_poll: false, },
            0xf1 => Instruction { op: Opcode::SBC, mode: AddressingMode::IndirectY, cyc: 5, early_intr_poll: false, },

            0x29 => Instruction { op: Opcode::AND, mode: AddressingMode::Immediate, cyc: 2, early_intr_poll: true, },
            0x25 => Instruction { op: Opcode::AND, mode: AddressingMode::ZeroPage, cyc: 3, early_intr_poll: false, },
            0x35 => Instruction { op: Opcode::AND, mode: AddressingMode::ZeroPageX, cyc: 4, early_intr_poll: false, },
            0x2d => Instruction { op: Opcode::AND, mode: AddressingMode::Absolute, cyc: 4, early_intr_poll: false, },
            0x3d => Instruction { op: Opcode::AND, mode: AddressingMode::AbsoluteX, cyc: 4, early_intr_poll: false, },
            0x39 => Instruction { op: Opcode::AND, mode: AddressingMode::AbsoluteY, cyc: 4, early_intr_poll: false, },
            0x21 => Instruction { op: Opcode::AND, mode: AddressingMode::IndirectX, cyc: 6, early_intr_poll: false, },
            0x31 => Instruction { op: Opcode::AND, mode: AddressingMode::IndirectY, cyc: 5, early_intr_poll: false, },

            0x49 => Instruction { op: Opcode::EOR, mode: AddressingMode::Immediate, cyc: 2, early_intr_poll: true, },
            0x45 => Instruction { op: Opcode::EOR, mode: AddressingMode::ZeroPage, cyc: 3, early_intr_poll: false, },
            0x55 => Instruction { op: Opcode::EOR, mode: AddressingMode::ZeroPageX, cyc: 4, early_intr_poll: false, },
            0x4d => Instruction { op: Opcode::EOR, mode: AddressingMode::Absolute, cyc: 4, early_intr_poll: false, },
            0x5d => Instruction { op: Opcode::EOR, mode: AddressingMode::AbsoluteX, cyc: 4, early_intr_poll: false, },
            0x59 => Instruction { op: Opcode::EOR, mode: AddressingMode::AbsoluteY, cyc: 4, early_intr_poll: false, },
            0x41 => Instruction { op: Opcode::EOR, mode: AddressingMode::IndirectX, cyc: 6, early_intr_poll: false, },
            0x51 => Instruction { op: Opcode::EOR, mode: AddressingMode::IndirectY, cyc: 5, early_intr_poll: false, },

            0x09 => Instruction { op: Opcode::ORA, mode: AddressingMode::Immediate, cyc: 2, early_intr_poll: true, },
            0x05 => Instruction { op: Opcode::ORA, mode: AddressingMode::ZeroPage, cyc: 3, early_intr_poll: false, },
            0x15 => Instruction { op: Opcode::ORA, mode: AddressingMode::ZeroPageX, cyc: 4, early_intr_poll: false, },
            0x0d => Instruction { op: Opcode::ORA, mode: AddressingMode::Absolute, cyc: 4, early_intr_poll: false, },
            0x1d => Instruction { op: Opcode::ORA, mode: AddressingMode::AbsoluteX, cyc: 4, early_intr_poll: false, },
            0x19 => Instruction { op: Opcode::ORA, mode: AddressingMode::AbsoluteY, cyc: 4, early_intr_poll: false, },
            0x01 => Instruction { op: Opcode::ORA, mode: AddressingMode::IndirectX, cyc: 6, early_intr_poll: false, },
            0x11 => Instruction { op: Opcode::ORA, mode: AddressingMode::IndirectY, cyc: 5, early_intr_poll: false, },

            /* *************** shift/rotate op ***************  */
            0x0a => Instruction { op: Opcode::ASL, mode: AddressingMode::Accumulator, cyc: 2, early_intr_poll: true, },
            0x06 => Instruction { op: Opcode::ASL, mode: AddressingMode::ZeroPage, cyc: 5, early_intr_poll: false, },
            0x16 => Instruction { op: Opcode::ASL, mode: AddressingMode::ZeroPageX, cyc: 6, early_intr_poll: false, },
            0x0e => Instruction { op: Opcode::ASL, mode: AddressingMode::Absolute, cyc: 6, early_intr_poll: false, },
            0x1e => Instruction { op: Opcode::ASL, mode: AddressingMode::AbsoluteX, cyc: 7, early_intr_poll: false, },

            0x4a => Instruction { op: Opcode::LSR, mode: AddressingMode::Accumulator, cyc: 2, early_intr_poll: true, },
            0x46 => Instruction { op: Opcode::LSR, mode: AddressingMode::ZeroPage, cyc: 5, early_intr_poll: false, },
            0x56 => Instruction { op: Opcode::LSR, mode: AddressingMode::ZeroPageX, cyc: 6, early_intr_poll: false, },
            0x4e => Instruction { op: Opcode::LSR, mode: AddressingMode::Absolute, cyc: 6, early_intr_poll: false, },
            0x5e => Instruction { op: Opcode::LSR, mode: AddressingMode::AbsoluteX, cyc: 7, early_intr_poll: false, },

            0x2a => Instruction { op: Opcode::ROL, mode: AddressingMode::Accumulator, cyc: 2, early_intr_poll: true, },
            0x26 => Instruction { op: Opcode::ROL, mode: AddressingMode::ZeroPage, cyc: 5, early_intr_poll: false, },
            0x36 => Instruction { op: Opcode::ROL, mode: AddressingMode::ZeroPageX, cyc: 6, early_intr_poll: false, },
            0x2e => Instruction { op: Opcode::ROL, mode: AddressingMode::Absolute, cyc: 6, early_intr_poll: false, },
            0x3e => Instruction { op: Opcode::ROL, mode: AddressingMode::AbsoluteX, cyc: 7, early_intr_poll: false, },

            0x6a => Instruction { op: Opcode::ROR, mode: AddressingMode::Accumulator, cyc: 2, early_intr_poll: true, },
            0x66 => Instruction { op: Opcode::ROR, mode: AddressingMode::ZeroPage, cyc: 5, early_intr_poll: false, },
            0x76 => Instruction { op: Opcode::ROR, mode: AddressingMode::ZeroPageX, cyc: 6, early_intr_poll: false, },
            0x6e => Instruction { op: Opcode::ROR, mode: AddressingMode::Absolute, cyc: 6, early_intr_poll: false, },
            0x7e => Instruction { op: Opcode::ROR, mode: AddressingMode::AbsoluteX, cyc: 7, early_intr_poll: false, },

            /* *************** inc/dec op ***************  */
            0xe6 => Instruction { op: Opcode::INC, mode: AddressingMode::ZeroPage, cyc: 5, early_intr_poll: false, },
            0xf6 => Instruction { op: Opcode::INC, mode: AddressingMode::ZeroPageX, cyc: 6, early_intr_poll: false, },
            0xee => Instruction { op: Opcode::INC, mode: AddressingMode::Absolute, cyc: 6, early_intr_poll: false, },
            0xfe => Instruction { op: Opcode::INC, mode: AddressingMode::AbsoluteX, cyc: 7, early_intr_poll: false, },

            0xe8 => Instruction { op: Opcode::INX, mode: AddressingMode::Implied, cyc: 2, early_intr_poll: true, },
            0xc8 => Instruction { op: Opcode::INY, mode: AddressingMode::Implied, cyc: 2, early_intr_poll: true, },

            0xc6 => Instruction { op: Opcode::DEC, mode: AddressingMode::ZeroPage, cyc: 5, early_intr_poll: false, },
            0xd6 => Instruction { op: Opcode::DEC, mode: AddressingMode::ZeroPageX, cyc: 6, early_intr_poll: false, },
            0xce => Instruction { op: Opcode::DEC, mode: AddressingMode::Absolute, cyc: 6, early_intr_poll: false, },
            0xde => Instruction { op: Opcode::DEC, mode: AddressingMode::AbsoluteX, cyc: 7, early_intr_poll: false, },

            0xca => Instruction { op: Opcode::DEX, mode: AddressingMode::Implied, cyc: 2, early_intr_poll: true, },
            0x88 => Instruction { op: Opcode::DEY, mode: AddressingMode::Implied, cyc: 2, early_intr_poll: true, },

            /* *************** load/store op ***************  */
            0xa9 => Instruction { op: Opcode::LDA, mode: AddressingMode::Immediate, cyc: 2, early_intr_poll: true, },
            0xa5 => Instruction { op: Opcode::LDA, mode: AddressingMode::ZeroPage, cyc: 3, early_intr_poll: false, },
            0xb5 => Instruction { op: Opcode::LDA, mode: AddressingMode::ZeroPageX, cyc: 4, early_intr_poll: false, },
            0xad => Instruction { op: Opcode::LDA, mode: AddressingMode::Absolute, cyc: 4, early_intr_poll: false, },
            0xbd => Instruction { op: Opcode::LDA, mode: AddressingMode::AbsoluteX, cyc: 4, early_intr_poll: false, },
            0xb9 => Instruction { op: Opcode::LDA, mode: AddressingMode::AbsoluteY, cyc: 4, early_intr_poll: false, },
            0xa1 => Instruction { op: Opcode::LDA, mode: AddressingMode::IndirectX, cyc: 6, early_intr_poll: false, },
            0xb1 => Instruction { op: Opcode::LDA, mode: AddressingMode::IndirectY, cyc: 5, early_intr_poll: false, },

            0xa2 => Instruction { op: Opcode::LDX, mode: AddressingMode::Immediate, cyc: 2, early_intr_poll: true, },
            0xa6 => Instruction { op: Opcode::LDX, mode: AddressingMode::ZeroPage, cyc: 3, early_intr_poll: false, },
            0xb6 => Instruction { op: Opcode::LDX, mode: AddressingMode::ZeroPageY, cyc: 4, early_intr_poll: false, },
            0xae => Instruction { op: Opcode::LDX, mode: AddressingMode::Absolute, cyc: 4, early_intr_poll: false, },
            0xbe => Instruction { op: Opcode::LDX, mode: AddressingMode::AbsoluteY, cyc: 4, early_intr_poll: false, },

            0xa0 => Instruction { op: Opcode::LDY, mode: AddressingMode::Immediate, cyc: 2, early_intr_poll: true, },
            0xa4 => Instruction { op: Opcode::LDY, mode: AddressingMode::ZeroPage, cyc: 3, early_intr_poll: false, },
            0xb4 => Instruction { op: Opcode::LDY, mode: AddressingMode::ZeroPageX, cyc: 4, early_intr_poll: false, },
            0xac => Instruction { op: Opcode::LDY, mode: AddressingMode::Absolute, cyc: 4, early_intr_poll: false, },
            0xbc => Instruction { op: Opcode::LDY, mode: AddressingMode::AbsoluteX, cyc: 4, early_intr_poll: false, },

            0x85 => Instruction { op: Opcode::STA, mode: AddressingMode::ZeroPage, cyc: 3, early_intr_poll: false, },
            0x95 => Instruction { op: Opcode::STA, mode: AddressingMode::ZeroPageX, cyc: 4, early_intr_poll: false, },
            0x8d => Instruction { op: Opcode::STA, mode: AddressingMode::Absolute, cyc: 4, early_intr_poll: false, },
            0x9d => Instruction { op: Opcode::STA, mode: AddressingMode::AbsoluteX, cyc: 5, early_intr_poll: false, },
            0x99 => Instruction { op: Opcode::STA, mode: AddressingMode::AbsoluteY, cyc: 5, early_intr_poll: false, },
            0x81 => Instruction { op: Opcode::STA, mode: AddressingMode::IndirectX, cyc: 6, early_intr_poll: false, },
            0x91 => Instruction { op: Opcode::STA, mode: AddressingMode::IndirectY, cyc: 6, early_intr_poll: false, },

            0x86 => Instruction { op: Opcode::STX, mode: AddressingMode::ZeroPage, cyc: 3, early_intr_poll: false, },
            0x96 => Instruction { op: Opcode::STX, mode: AddressingMode::ZeroPageY, cyc: 4, early_intr_poll: false, },
            0x8e => Instruction { op: Opcode::STX, mode: AddressingMode::Absolute, cyc: 4, early_intr_poll: false, },

            0x84 => Instruction { op: Opcode::STY, mode: AddressingMode::ZeroPage, cyc: 3, early_intr_poll: false, },
            0x94 => Instruction { op: Opcode::STY, mode: AddressingMode::ZeroPageX, cyc: 4, early_intr_poll: false, },
            0x8c => Instruction { op: Opcode::STY, mode: AddressingMode::Absolute, cyc: 4, early_intr_poll: false, },

            /* *************** set/clear flag ***************  */
            0x38 => Instruction { op: Opcode::SEC, mode: AddressingMode::Implied, cyc: 2, early_intr_poll: true, },
            0xf8 => Instruction { op: Opcode::SED, mode: AddressingMode::Implied, cyc: 2, early_intr_poll: true, },
            0x78 => Instruction { op: Opcode::SEI, mode: AddressingMode::Implied, cyc: 2, early_intr_poll: true, },
            0x18 => Instruction { op: Opcode::CLC, mode: AddressingMode::Implied, cyc: 2, early_intr_poll: true, },
            0xd8 => Instruction { op: Opcode::CLD, mode: AddressingMode::Implied, cyc: 2, early_intr_poll: true, },
            0x58 => Instruction { op: Opcode::CLI, mode: AddressingMode::Implied, cyc: 2, early_intr_poll: true, },
            0xb8 => Instruction { op: Opcode::CLV, mode: AddressingMode::Implied, cyc: 2, early_intr_poll: true, },

            /* *************** compare ***************  */
            0xc9 => Instruction { op: Opcode::CMP, mode: AddressingMode::Immediate, cyc: 2, early_intr_poll: true, },
            0xc5 => Instruction { op: Opcode::CMP, mode: AddressingMode::ZeroPage, cyc: 3, early_intr_poll: false, },
            0xd5 => Instruction { op: Opcode::CMP, mode: AddressingMode::ZeroPageX, cyc: 4, early_intr_poll: false, },
            0xcd => Instruction { op: Opcode::CMP, mode: AddressingMode::Absolute, cyc: 4, early_intr_poll: false, },
            0xdd => Instruction { op: Opcode::CMP, mode: AddressingMode::AbsoluteX, cyc: 4, early_intr_poll: false, },
            0xd9 => Instruction { op: Opcode::CMP, mode: AddressingMode::AbsoluteY, cyc: 4, early_intr_poll: false, },
            0xc1 => Instruction { op: Opcode::CMP, mode: AddressingMode::IndirectX, cyc: 6, early_intr_poll: false, },
            0xd1 => Instruction { op: Opcode::CMP, mode: AddressingMode::IndirectY, cyc: 5, early_intr_poll: false, },

            0xe0 => Instruction { op: Opcode::CPX, mode: AddressingMode::Immediate, cyc: 2, early_intr_poll: true, },
            0xe4 => Instruction { op: Opcode::CPX, mode: AddressingMode::ZeroPage, cyc: 3, early_intr_poll: false, },
            0xec => Instruction { op: Opcode::CPX, mode: AddressingMode::Absolute, cyc: 4, early_intr_poll: false, },

            0xc0 => Instruction { op: Opcode::CPY, mode: AddressingMode::Immediate, cyc: 2, early_intr_poll: true, },
            0xc4 => Instruction { op: Opcode::CPY, mode: AddressingMode::ZeroPage, cyc: 3, early_intr_poll: false, },
            0xcc => Instruction { op: Opcode::CPY, mode: AddressingMode::Absolute, cyc: 4, early_intr_poll: false, },

            /* *************** jump/return ***************  */
            0x4c => Instruction { op: Opcode::JMP, mode: AddressingMode::Absolute, cyc: 3, early_intr_poll: false, },
            0x6c => Instruction { op: Opcode::JMP, mode: AddressingMode::AbsoluteIndirect, cyc: 5, early_intr_poll: false, },

            0x20 => Instruction { op: Opcode::JSR, mode: AddressingMode::Absolute, cyc: 6, early_intr_poll: false, },

            0x40 => Instruction { op: Opcode::RTI, mode: AddressingMode::Implied, cyc: 6, early_intr_poll: false, },
            0x60 => Instruction { op: Opcode::RTS, mode: AddressingMode::Implied, cyc: 6, early_intr_poll: false, },

            /* *************** branch ***************  */

            // XXX: technically the hardware predecoder unit does _not_ consider these
            // instructions to be "two-cycle" instructions, which may be relevant for
            // deciding when to poll for interrupts

            0x90 => Instruction { op: Opcode::BCC, mode: AddressingMode::Relative, cyc: 2, early_intr_poll: true, },
            0xb0 => Instruction { op: Opcode::BCS, mode: AddressingMode::Relative, cyc: 2, early_intr_poll: true, },
            0xf0 => Instruction { op: Opcode::BEQ, mode: AddressingMode::Relative, cyc: 2, early_intr_poll: true, },
            0xd0 => Instruction { op: Opcode::BNE, mode: AddressingMode::Relative, cyc: 2, early_intr_poll: true, },
            0x30 => Instruction { op: Opcode::BMI, mode: AddressingMode::Relative, cyc: 2, early_intr_poll: true, },
            0x10 => Instruction { op: Opcode::BPL, mode: AddressingMode::Relative, cyc: 2, early_intr_poll: true, },
            0x50 => Instruction { op: Opcode::BVC, mode: AddressingMode::Relative, cyc: 2, early_intr_poll: true, },
            0x70 => Instruction { op: Opcode::BVS, mode: AddressingMode::Relative, cyc: 2, early_intr_poll: true, },

            /* *************** push/pop *************** , early_intr_poll: false, */
            0x48 => Instruction { op: Opcode::PHA, mode: AddressingMode::Implied, cyc: 3, early_intr_poll: false, },
            0x08 => Instruction { op: Opcode::PHP, mode: AddressingMode::Implied, cyc: 3, early_intr_poll: false, },
            0x68 => Instruction { op: Opcode::PLA, mode: AddressingMode::Implied, cyc: 4, early_intr_poll: false, },
            0x28 => Instruction { op: Opcode::PLP, mode: AddressingMode::Implied, cyc: 4, early_intr_poll: false, },

            /* *************** transfer ***************  */
            0xaa => Instruction { op: Opcode::TAX, mode: AddressingMode::Implied, cyc: 2, early_intr_poll: true, },
            0xa8 => Instruction { op: Opcode::TAY, mode: AddressingMode::Implied, cyc: 2, early_intr_poll: true, },
            0xba => Instruction { op: Opcode::TSX, mode: AddressingMode::Implied, cyc: 2, early_intr_poll: true, },
            0x8a => Instruction { op: Opcode::TXA, mode: AddressingMode::Implied, cyc: 2, early_intr_poll: true, },
            0x9a => Instruction { op: Opcode::TXS, mode: AddressingMode::Implied, cyc: 2, early_intr_poll: true, },
            0x98 => Instruction { op: Opcode::TYA, mode: AddressingMode::Implied, cyc: 2, early_intr_poll: true, },

            /* *************** other ***************  */

            // Note: BRK will actually take 7 cycles but as a special case we declare it as taking 2 cycles here
            // considering that the later 5 cycles will happen within `self.handle_interrupt` which is after we
            // use the `cyc` count to validate that read/write cycle counts match out expected cycle count.
            0x00 => Instruction { op: Opcode::BRK, mode: AddressingMode::Implied, cyc: 2, early_intr_poll: false, },

            0x24 => Instruction { op: Opcode::BIT, mode: AddressingMode::ZeroPage, cyc: 3, early_intr_poll: false, },
            0x2c => Instruction { op: Opcode::BIT, mode: AddressingMode::Absolute, cyc: 4, early_intr_poll: false, },

            0xea => Instruction { op: Opcode::NOP, mode: AddressingMode::Implied, cyc: 2, early_intr_poll: true, },

            /* *************** unofficial1 ***************  */
            // https://www.nesdev.com/undocumented_opcodes.txt
            // https://www.nesdev.org/wiki/Programming_with_unofficial_opcodes
            0x0b => Instruction { op: Opcode::AAC, mode: AddressingMode::Immediate, cyc: 2, early_intr_poll: true, },
            0x2b => Instruction { op: Opcode::AAC, mode: AddressingMode::Immediate, cyc: 2, early_intr_poll: true, },

            0x87 => Instruction { op: Opcode::AAX, mode: AddressingMode::ZeroPage, cyc: 3, early_intr_poll: false, },
            0x97 => Instruction { op: Opcode::AAX, mode: AddressingMode::ZeroPageY, cyc: 4, early_intr_poll: false, },
            0x83 => Instruction { op: Opcode::AAX, mode: AddressingMode::IndirectX, cyc: 6, early_intr_poll: false, },
            0x8f => Instruction { op: Opcode::AAX, mode: AddressingMode::Absolute, cyc: 4, early_intr_poll: false, },

            0x6b => Instruction { op: Opcode::ARR, mode: AddressingMode::Immediate, cyc: 2, early_intr_poll: true, },

            0x4b => Instruction { op: Opcode::ASR, mode: AddressingMode::Immediate, cyc: 2, early_intr_poll: true, },

            0xab => Instruction { op: Opcode::ATX, mode: AddressingMode::Immediate, cyc: 2, early_intr_poll: true, },

            0x9f => Instruction { op: Opcode::AXA, mode: AddressingMode::AbsoluteY, cyc: 5, early_intr_poll: false, },
            0x93 => Instruction { op: Opcode::AXA, mode: AddressingMode::IndirectY, cyc: 6, early_intr_poll: false, },

            0xcb => Instruction { op: Opcode::AXS, mode: AddressingMode::Immediate, cyc: 2, early_intr_poll: true, },

            0xc7 => Instruction { op: Opcode::DCP, mode: AddressingMode::ZeroPage, cyc: 5, early_intr_poll: false, },
            0xd7 => Instruction { op: Opcode::DCP, mode: AddressingMode::ZeroPageX, cyc: 6, early_intr_poll: false, },
            0xcf => Instruction { op: Opcode::DCP, mode: AddressingMode::Absolute, cyc: 6, early_intr_poll: false, },
            0xdf => Instruction { op: Opcode::DCP, mode: AddressingMode::AbsoluteX, cyc: 7, early_intr_poll: false, },
            0xdb => Instruction { op: Opcode::DCP, mode: AddressingMode::AbsoluteY, cyc: 7, early_intr_poll: false, },
            0xc3 => Instruction { op: Opcode::DCP, mode: AddressingMode::IndirectX, cyc: 8, early_intr_poll: false, },
            0xd3 => Instruction { op: Opcode::DCP, mode: AddressingMode::IndirectY, cyc: 8, early_intr_poll: false, },

            0x04 => Instruction { op: Opcode::DOP, mode: AddressingMode::ZeroPage, cyc: 3, early_intr_poll: false, },
            0x14 => Instruction { op: Opcode::DOP, mode: AddressingMode::ZeroPageX, cyc: 4, early_intr_poll: false, },
            0x34 => Instruction { op: Opcode::DOP, mode: AddressingMode::ZeroPageX, cyc: 4, early_intr_poll: false, },
            0x44 => Instruction { op: Opcode::DOP, mode: AddressingMode::ZeroPage, cyc: 3, early_intr_poll: false, },
            0x54 => Instruction { op: Opcode::DOP, mode: AddressingMode::ZeroPageX, cyc: 4, early_intr_poll: false, },
            0x64 => Instruction { op: Opcode::DOP, mode: AddressingMode::ZeroPage, cyc: 3, early_intr_poll: false, },
            0x74 => Instruction { op: Opcode::DOP, mode: AddressingMode::ZeroPageX, cyc: 4, early_intr_poll: false, },
            0x80 => Instruction { op: Opcode::DOP, mode: AddressingMode::Immediate, cyc: 2, early_intr_poll: true, },
            0x82 => Instruction { op: Opcode::DOP, mode: AddressingMode::Immediate, cyc: 2, early_intr_poll: true, },
            0x89 => Instruction { op: Opcode::DOP, mode: AddressingMode::Immediate, cyc: 2, early_intr_poll: true, },
            0xc2 => Instruction { op: Opcode::DOP, mode: AddressingMode::Immediate, cyc: 2, early_intr_poll: true, },
            0xd4 => Instruction { op: Opcode::DOP, mode: AddressingMode::ZeroPageX, cyc: 4, early_intr_poll: false, },
            0xe2 => Instruction { op: Opcode::DOP, mode: AddressingMode::Immediate, cyc: 2, early_intr_poll: true, },
            0xf4 => Instruction { op: Opcode::DOP, mode: AddressingMode::ZeroPageX, cyc: 4, early_intr_poll: false, },

            0xe7 => Instruction { op: Opcode::ISC, mode: AddressingMode::ZeroPage, cyc: 5, early_intr_poll: false, },
            0xf7 => Instruction { op: Opcode::ISC, mode: AddressingMode::ZeroPageX, cyc: 6, early_intr_poll: false, },
            0xef => Instruction { op: Opcode::ISC, mode: AddressingMode::Absolute, cyc: 6, early_intr_poll: false, },
            0xff => Instruction { op: Opcode::ISC, mode: AddressingMode::AbsoluteX, cyc: 7, early_intr_poll: false, },
            0xfb => Instruction { op: Opcode::ISC, mode: AddressingMode::AbsoluteY, cyc: 7, early_intr_poll: false, },
            0xe3 => Instruction { op: Opcode::ISC, mode: AddressingMode::IndirectX, cyc: 8, early_intr_poll: false, },
            0xf3 => Instruction { op: Opcode::ISC, mode: AddressingMode::IndirectY, cyc: 8, early_intr_poll: false, },

            0xbb => Instruction { op: Opcode::LAR, mode: AddressingMode::AbsoluteY, cyc: 4, early_intr_poll: false, },

            0xa7 => Instruction { op: Opcode::LAX, mode: AddressingMode::ZeroPage, cyc: 3, early_intr_poll: false, },
            0xb7 => Instruction { op: Opcode::LAX, mode: AddressingMode::ZeroPageY, cyc: 4, early_intr_poll: false, },
            0xaf => Instruction { op: Opcode::LAX, mode: AddressingMode::Absolute, cyc: 4, early_intr_poll: false, },
            0xbf => Instruction { op: Opcode::LAX, mode: AddressingMode::AbsoluteY, cyc: 4, early_intr_poll: false, },
            0xa3 => Instruction { op: Opcode::LAX, mode: AddressingMode::IndirectX, cyc: 6, early_intr_poll: false, },
            0xb3 => Instruction { op: Opcode::LAX, mode: AddressingMode::IndirectY, cyc: 5, early_intr_poll: false, },

            0x1a => Instruction { op: Opcode::NOP, mode: AddressingMode::Implied, cyc: 2, early_intr_poll: true, },
            0x3a => Instruction { op: Opcode::NOP, mode: AddressingMode::Implied, cyc: 2, early_intr_poll: true, },
            0x5a => Instruction { op: Opcode::NOP, mode: AddressingMode::Implied, cyc: 2, early_intr_poll: true, },
            0x7a => Instruction { op: Opcode::NOP, mode: AddressingMode::Implied, cyc: 2, early_intr_poll: true, },
            0xda => Instruction { op: Opcode::NOP, mode: AddressingMode::Implied, cyc: 2, early_intr_poll: true, },
            0xfa => Instruction { op: Opcode::NOP, mode: AddressingMode::Implied, cyc: 2, early_intr_poll: true, },

            0x27 => Instruction { op: Opcode::RLA, mode: AddressingMode::ZeroPage, cyc: 5, early_intr_poll: false, },
            0x37 => Instruction { op: Opcode::RLA, mode: AddressingMode::ZeroPageX, cyc: 6, early_intr_poll: false, },
            0x2f => Instruction { op: Opcode::RLA, mode: AddressingMode::Absolute, cyc: 6, early_intr_poll: false, },
            0x3f => Instruction { op: Opcode::RLA, mode: AddressingMode::AbsoluteX, cyc: 7, early_intr_poll: false, },
            0x3b => Instruction { op: Opcode::RLA, mode: AddressingMode::AbsoluteY, cyc: 7, early_intr_poll: false, },
            0x23 => Instruction { op: Opcode::RLA, mode: AddressingMode::IndirectX, cyc: 8, early_intr_poll: false, },
            0x33 => Instruction { op: Opcode::RLA, mode: AddressingMode::IndirectY, cyc: 8, early_intr_poll: false, },

            0x67 => Instruction { op: Opcode::RRA, mode: AddressingMode::ZeroPage, cyc: 5, early_intr_poll: false, },
            0x77 => Instruction { op: Opcode::RRA, mode: AddressingMode::ZeroPageX, cyc: 6, early_intr_poll: false, },
            0x6f => Instruction { op: Opcode::RRA, mode: AddressingMode::Absolute, cyc: 6, early_intr_poll: false, },
            0x7f => Instruction { op: Opcode::RRA, mode: AddressingMode::AbsoluteX, cyc: 7, early_intr_poll: false, },
            0x7b => Instruction { op: Opcode::RRA, mode: AddressingMode::AbsoluteY, cyc: 7, early_intr_poll: false, },
            0x63 => Instruction { op: Opcode::RRA, mode: AddressingMode::IndirectX, cyc: 8, early_intr_poll: false, },
            0x73 => Instruction { op: Opcode::RRA, mode: AddressingMode::IndirectY, cyc: 8, early_intr_poll: false, },

            0xeb => Instruction { op: Opcode::SBC, mode: AddressingMode::Immediate, cyc: 2, early_intr_poll: true, },

            0x07 => Instruction { op: Opcode::SLO, mode: AddressingMode::ZeroPage, cyc: 5, early_intr_poll: false, },
            0x17 => Instruction { op: Opcode::SLO, mode: AddressingMode::ZeroPageX, cyc: 6, early_intr_poll: false, },
            0x0f => Instruction { op: Opcode::SLO, mode: AddressingMode::Absolute, cyc: 6, early_intr_poll: false, },
            0x1f => Instruction { op: Opcode::SLO, mode: AddressingMode::AbsoluteX, cyc: 7, early_intr_poll: false, },
            0x1b => Instruction { op: Opcode::SLO, mode: AddressingMode::AbsoluteY, cyc: 7, early_intr_poll: false, },
            0x03 => Instruction { op: Opcode::SLO, mode: AddressingMode::IndirectX, cyc: 8, early_intr_poll: false, },
            0x13 => Instruction { op: Opcode::SLO, mode: AddressingMode::IndirectY, cyc: 8, early_intr_poll: false, },

            0x47 => Instruction { op: Opcode::SRE, mode: AddressingMode::ZeroPage, cyc: 5, early_intr_poll: false, },
            0x57 => Instruction { op: Opcode::SRE, mode: AddressingMode::ZeroPageX, cyc: 6, early_intr_poll: false, },
            0x4f => Instruction { op: Opcode::SRE, mode: AddressingMode::Absolute, cyc: 6, early_intr_poll: false, },
            0x5f => Instruction { op: Opcode::SRE, mode: AddressingMode::AbsoluteX, cyc: 7, early_intr_poll: false, },
            0x5b => Instruction { op: Opcode::SRE, mode: AddressingMode::AbsoluteY, cyc: 7, early_intr_poll: false, },
            0x43 => Instruction { op: Opcode::SRE, mode: AddressingMode::IndirectX, cyc: 8, early_intr_poll: false, },
            0x53 => Instruction { op: Opcode::SRE, mode: AddressingMode::IndirectY, cyc: 8, early_intr_poll: false, },

            0x9e => Instruction { op: Opcode::SXA, mode: AddressingMode::AbsoluteY, cyc: 5, early_intr_poll: false, },

            0x9c => Instruction { op: Opcode::SYA, mode: AddressingMode::AbsoluteX, cyc: 5, early_intr_poll: false, },

            0x0c => Instruction { op: Opcode::TOP, mode: AddressingMode::Absolute, cyc: 4, early_intr_poll: false, },
            0x1c => Instruction { op: Opcode::TOP, mode: AddressingMode::AbsoluteX, cyc: 4, early_intr_poll: false, },
            0x3c => Instruction { op: Opcode::TOP, mode: AddressingMode::AbsoluteX, cyc: 4, early_intr_poll: false, },
            0x5c => Instruction { op: Opcode::TOP, mode: AddressingMode::AbsoluteX, cyc: 4, early_intr_poll: false, },
            0x7c => Instruction { op: Opcode::TOP, mode: AddressingMode::AbsoluteX, cyc: 4, early_intr_poll: false, },
            0xdc => Instruction { op: Opcode::TOP, mode: AddressingMode::AbsoluteX, cyc: 4, early_intr_poll: false, },
            0xfc => Instruction { op: Opcode::TOP, mode: AddressingMode::AbsoluteX, cyc: 4, early_intr_poll: false, },

            0x8b => Instruction { op: Opcode::XAA, mode: AddressingMode::Immediate, cyc: 2, early_intr_poll: true, },

            0x9b => Instruction { op: Opcode::XAS, mode: AddressingMode::AbsoluteY, cyc: 5, early_intr_poll: false, },


            // Invalid op codes
            0x02 | 0x12 | 0x22 | 0x32 | 0x42 | 0x52 | 0x62 | 0x72 | 0x92 | 0xb2 | 0xd2 |0xf2 =>
                Instruction { op: Opcode::HALT, mode: AddressingMode::Implied, cyc: 1, early_intr_poll: false, },
        }
    }

    pub fn len(&self) -> usize {
        match self.mode {
            AddressingMode::Implied => 1,
            AddressingMode::Accumulator => 1,
            AddressingMode::Immediate => 2,
            AddressingMode::Absolute => 3,
            AddressingMode::ZeroPage => 2,
            AddressingMode::ZeroPageX => 2,
            AddressingMode::ZeroPageY => 2,
            AddressingMode::AbsoluteX => 3,
            AddressingMode::AbsoluteY => 3,
            AddressingMode::AbsoluteIndirect => 3,
            AddressingMode::IndirectX => 2,
            AddressingMode::IndirectY => 2,
            AddressingMode::Relative => 2,
        }
    }

    pub fn disassemble(&self, operand: u16, effective_address: u16, _loaded: u8, _stored: u8) -> String {

        /*
        if self.stores() {
            match self.mode {
                AddressingMode::Implied => format!("{:#?} = ${stored:02X}", self.op),
                AddressingMode::Accumulator => format!("{:#?} A = ${stored:02X}", self.op),
                AddressingMode::Immediate => format!("{:#?} #${operand:02X} = ${stored:02X}", self.op),
                AddressingMode::Absolute => format!("{:#?} ${operand:04X} = ${stored:02X}", self.op),
                AddressingMode::ZeroPage => format!("{:#?} ${operand:02X} = ${stored:02X}", self.op),
                AddressingMode::ZeroPageX => format!("{:#?} ${operand:02X}, X = ${stored:02X}", self.op),
                AddressingMode::ZeroPageY => format!("{:#?} ${operand:02X}, Y = ${stored:02X}", self.op),
                AddressingMode::AbsoluteX => format!("{:#?} ${operand:04X}, X = ${stored:02X}", self.op),
                AddressingMode::AbsoluteY => format!("{:#?} ${operand:04X}, Y = ${stored:02X}", self.op),
                AddressingMode::Relative => format!("{:#?} ${operand:02X} = ${stored:02X}", self.op),
                AddressingMode::AbsoluteIndirect => format!("{:#?} (${operand:04X}) = ${stored:02X}", self.op),
                AddressingMode::IndirectX => format!("{:#?} (${operand:02X}, X) = ${stored:02X}", self.op),
                AddressingMode::IndirectY => format!("{:#?} (${operand:02X}), Y = ${stored:02X}", self.op),
            }
        } else {
            match self.mode {
                AddressingMode::Implied => format!("{:#?}", self.op),
                AddressingMode::Accumulator => format!("{:#?} A", self.op),
                AddressingMode::Immediate => format!("{:#?} #${operand:02X}", self.op),
                AddressingMode::Absolute => format!("{:#?} ${operand:04X}", self.op),
                AddressingMode::ZeroPage => format!("{:#?} ${operand:02X}", self.op),
                AddressingMode::ZeroPageX => format!("{:#?} ${operand:02X}, X", self.op),
                AddressingMode::ZeroPageY => format!("{:#?} ${operand:02X}, Y", self.op),
                AddressingMode::AbsoluteX => format!("{:#?} ${operand:04X}, X", self.op),
                AddressingMode::AbsoluteY => format!("{:#?} ${operand:04X}, Y", self.op),
                AddressingMode::Relative => format!("{:#?} ${operand:02X}", self.op),
                AddressingMode::AbsoluteIndirect => format!("{:#?} (${operand:04X})", self.op),
                AddressingMode::IndirectX => format!("{:#?} (${operand:02X}, X)", self.op),
                AddressingMode::IndirectY => format!("{:#?} (${operand:02X}), Y", self.op),
            }
        }
        */
        match self.mode {
            AddressingMode::Implied => format!("{:#?}", self.op),
            AddressingMode::Accumulator => format!("{:#?} A", self.op),
            AddressingMode::Immediate => format!("{:#?} #${operand:02X}", self.op),
            AddressingMode::Absolute => format!("{:#?} ${operand:04X}", self.op),
            AddressingMode::ZeroPage => format!("{:#?} ${operand:02X}", self.op),
            AddressingMode::ZeroPageX => format!("{:#?} ${operand:02X},X", self.op),
            AddressingMode::ZeroPageY => format!("{:#?} ${operand:02X},Y", self.op),
            AddressingMode::AbsoluteX => format!("{:#?} ${operand:04X},X", self.op),
            AddressingMode::AbsoluteY => format!("{:#?} ${operand:04X},Y", self.op),
            AddressingMode::Relative => format!("{:#?} ${effective_address:04X}", self.op),
            AddressingMode::AbsoluteIndirect => format!("{:#?} (${operand:04X})", self.op),
            AddressingMode::IndirectX => format!("{:#?} (${operand:02X},X)", self.op),
            AddressingMode::IndirectY => format!("{:#?} (${operand:02X}),Y", self.op),
        }
    }

}

impl Cpu {

    /// Fetch 1 byte from PC but _don't_ increment the PC by one
    ///
    /// This is effectively what the HW does for implied operand instructions, and
    /// we emulate this so that we can step the clock / system mid-instruction and
    /// since it also affects when DMC or OAM DMAs will start (they both only start
    /// when the CPU executes it's next read)
    fn nop_pc_fetch_u8(&mut self, system: &mut System) {
        self.dummy_read_system_bus(system, self.pc);
    }

    /// Fetch 1 byte from PC and after fetching, advance the PC by one
    fn pc_fetch_u8(&mut self, system: &mut System) -> u8 {
        //println!("calling system.read_u8({:x}", self.pc);
        let data = self.read_system_bus(system, self.pc);
        self.pc = self.pc + 1;

        data
    }

    /// Fetch 2 bytes from PC and increment PC by two
    fn pc_fetch_u16(&mut self, system: &mut System) -> u16 {
        let lower = self.pc_fetch_u8(system);
        let upper = self.pc_fetch_u8(system);
        let data = u16::from(lower) | (u16::from(upper) << 8);
        data
    }

    pub fn pc_peek_instruction(&self, system: &mut System) -> Instruction {
        let opcode = system.peek(self.pc);
        Instruction::from(opcode)
    }

    /// Fetch the operand.
    /// Depending on the Addressing mode, the PC also advances.
    /// When implementing, Cpu::fetch when reading the operand immediately after the instruction, otherwise System::read
    fn fetch_operand(&mut self, system: &mut System, mode: AddressingMode, oops_handling: OopsHandling) -> FetchedOperand {
        let operand = match mode {
            AddressingMode::Implied => {
                self.nop_pc_fetch_u8(system); // read next instruction byte but throw it away
                FetchedOperand { raw_operand: 0, operand: 0, oops_cyc: 0 }
            },
            AddressingMode::Accumulator => {
                self.nop_pc_fetch_u8(system); // read next instruction byte but throw it away
                FetchedOperand { raw_operand: 0, operand: 0, oops_cyc: 0 }
            },
            AddressingMode::Immediate => {
                let in_operand = self.pc_fetch_u8(system) as u16;
                FetchedOperand { raw_operand: in_operand, operand: u16::from(in_operand), oops_cyc: 0 }
            },
            AddressingMode::Absolute => {
                let in_operand = self.pc_fetch_u16(system);
                FetchedOperand { raw_operand: in_operand, operand: in_operand, oops_cyc: 0 }
            },
            AddressingMode::ZeroPage => {
                let in_operand = self.pc_fetch_u8(system) as u16;
                FetchedOperand { raw_operand: in_operand, operand: in_operand, oops_cyc: 0 }
            },
            AddressingMode::ZeroPageX => {
                let in_operand = self.pc_fetch_u8(system);
                self.dummy_read_system_bus(system, in_operand as u16); // read while adding X
                FetchedOperand { raw_operand: in_operand as u16, operand: u16::from(in_operand.wrapping_add(self.x)), oops_cyc: 0 }
            }
            AddressingMode::ZeroPageY => {
                let in_operand = self.pc_fetch_u8(system);
                self.dummy_read_system_bus(system, in_operand as u16); // read while adding Y
                FetchedOperand { raw_operand: in_operand as u16, operand: u16::from(in_operand.wrapping_add(self.y)), oops_cyc: 0 }
            }
            AddressingMode::AbsoluteX => {
                let in_operand = self.pc_fetch_u16(system);
                let data = in_operand.wrapping_add(u16::from(self.x));
                let oops_cyc =
                    if (in_operand & 0xff00u16) != (data & 0xff00u16) || oops_handling == OopsHandling::Always {
                        //println!("AbsoluteX oops: operand = {in_operand:x} addr = {data:x}");
                        match oops_handling {
                            OopsHandling::Always | OopsHandling::Normal => {
                                let wrong_addr = in_operand & 0xff00 | data & 0xff; // Address without carry for high byte
                                self.dummy_read_system_bus(system, wrong_addr);
                            },
                            OopsHandling::Ignore => {}
                        }
                        1
                    } else {
                        0
                    };
                FetchedOperand { raw_operand: in_operand as u16, operand: data, oops_cyc }
            }
            AddressingMode::AbsoluteY => {
                let in_operand = self.pc_fetch_u16(system);
                let data = in_operand.wrapping_add(u16::from(self.y));
                let oops_cyc =
                    if (in_operand & 0xff00u16) != (data & 0xff00u16) || oops_handling == OopsHandling::Always {
                        match oops_handling {
                            OopsHandling::Always | OopsHandling::Normal => {
                                let wrong_addr = in_operand & 0xff00 | data & 0xff; // Address without carry for high byte
                                self.dummy_read_system_bus(system, wrong_addr);
                            },
                            OopsHandling::Ignore => {}
                        }
                        1
                    } else {
                        0
                    };
                FetchedOperand { raw_operand: in_operand as u16, operand: data, oops_cyc }
            }
            AddressingMode::Relative => {
                let in_operand = self.pc_fetch_u8(system);
                let offset = in_operand as i8;

                // XXX: haven't seen any clarification on how the hardware handles overflow/underflow
                // with the signed arithmetic here...

                let data = self.pc.wrapping_add(offset as u16);
                //let signed_addr = (self.pc as i32) + (offset as i32); // Sign extension and calculation
                //debug_assert!(signed_addr >= 0);
                //debug_assert!(signed_addr < 0x10000);

                //let data = signed_addr as u16;
                let oops_cyc = if (data & 0xff00u16) != (self.pc & 0xff00u16) || oops_handling == OopsHandling::Always {
                    match oops_handling {
                        OopsHandling::Always | OopsHandling::Normal => {
                            let wrong_addr = self.pc & 0xff00 | data & 0xff; // Address without carry for high byte
                            self.dummy_read_system_bus(system, wrong_addr);
                        },
                        OopsHandling::Ignore => {}
                    }
                    1
                } else {
                    0
                };

                FetchedOperand { raw_operand: in_operand as u16, operand: data, oops_cyc }
            }
            AddressingMode::AbsoluteIndirect => {
                let src_addr_lower = self.pc_fetch_u8(system);
                let src_addr_upper = self.pc_fetch_u8(system);

                let dst_addr_lower = u16::from(src_addr_lower) | (u16::from(src_addr_upper) << 8); // operand as it is

                // NB: The original 6502 can't (correctly) read addresses that cross page boundaries as
                // it only wraps the lower indirect address byte at page boundaries
                let dst_addr_upper =
                    u16::from(src_addr_lower.wrapping_add(1)) | (u16::from(src_addr_upper) << 8); // +1 to the lower of the operand

                let dst_data_lower = u16::from(self.read_system_bus(system, dst_addr_lower));
                let dst_data_upper = u16::from(self.read_system_bus(system, dst_addr_upper));

                let indirect = dst_data_lower | (dst_data_upper << 8);
                FetchedOperand { raw_operand: dst_addr_lower, operand: indirect, oops_cyc: 0 }
            }
            AddressingMode::IndirectX => {
                let src_addr = self.pc_fetch_u8(system);

                self.dummy_read_system_bus(system, src_addr as u16); // dummy read while adding X
                let dst_addr = src_addr.wrapping_add(self.x);

                let data_lower = u16::from(self.read_system_bus(system, u16::from(dst_addr)));
                let data_upper =
                    u16::from(self.read_system_bus(system, u16::from(dst_addr.wrapping_add(1))));

                let indirect = data_lower | (data_upper << 8);
                FetchedOperand { raw_operand: src_addr as u16, operand: indirect, oops_cyc: 0 }
            }
            AddressingMode::IndirectY => {
                let src_addr = self.pc_fetch_u8(system);

                let data_lower = u16::from(self.read_system_bus(system, u16::from(src_addr)));
                let data_upper =
                    u16::from(self.read_system_bus(system, u16::from(src_addr.wrapping_add(1))));

                let base_data = data_lower | (data_upper << 8);
                let indirect = base_data.wrapping_add(u16::from(self.y));
                let oops_cyc = if (base_data & 0xff00u16) != (indirect & 0xff00u16) || oops_handling == OopsHandling::Always {
                    match oops_handling {
                        OopsHandling::Always | OopsHandling::Normal => {
                            let wrong_addr = base_data & 0xff00 | indirect & 0xff; // Address without carry for high byte
                            self.dummy_read_system_bus(system, wrong_addr);
                        },
                        OopsHandling::Ignore => {}
                    }
                    1
                } else {
                    0
                };

                FetchedOperand { raw_operand: src_addr as u16, operand: indirect, oops_cyc }
            }
        };

        #[cfg(feature="trace")]
        {
            self.trace.instruction_operand = operand.raw_operand;
            self.trace.effective_address = operand.operand;
        }

        operand
    }

    /// Fetch address operand and dereference that to read the value at that address
    /// If you want to pull not only the address but also the data in one shot
    /// returns (Operand { data: immediate value or address, number of clocks), data)
    fn fetch_operand_and_value(&mut self, system: &mut System, mode: AddressingMode, oops_handling: OopsHandling) -> (FetchedOperand, u8) {
        let (fetched, value) = match mode {
            AddressingMode::Implied => {
                // In general, instructions with implied addressing shouldn't call this API but
                // NOP is an exception which supports lots of addressing modes and we need to
                // do a dumy read of the next instruction byte in this case
                (self.fetch_operand(system, mode, oops_handling), 0)
            },
            // Use the value of the a register
            AddressingMode::Accumulator => (self.fetch_operand(system, mode, oops_handling), self.a),
            // Immediate value uses 1 byte of data immediately after opcode as it is
            AddressingMode::Immediate => {
                let fetched_operand = self.fetch_operand(system, mode, oops_handling);
                debug_assert!(fetched_operand.operand < 0x100u16);
                (fetched_operand, fetched_operand.operand as u8)
            }
            // Others pull back the data from the returned address. May not be used
            _ => {
                let fetched_operand = self.fetch_operand(system, mode, oops_handling);
                let data = self.read_system_bus(system, fetched_operand.operand);
                (fetched_operand, data)
            }
        };

        #[cfg(feature="trace")]
        {
            self.trace.loaded_mem_value = value;
        }

        (fetched, value)
    }

    // TODO: experiment with something like this for enabling the debugger
    // to step single cycles, within an instruction:
    // https://play.rust-lang.org/?version=stable&mode=debug&edition=2021&gist=3cea23f5d8935a3463dcaad97212af88
    //
    /// Execute the next instruction
    pub fn step_instruction(&mut self, system: &mut System) {
        //println!("CPU: step_instruction()");

        #[cfg(feature="debugger")]
        {
            if self.debugger.breakpoints.len() > 0 {
                let mut tmp = std::mem::take(&mut self.debugger.breakpoints);
                let mut remove = vec![];
                for bp in tmp.iter_mut() {
                    if bp.addr == self.pc {
                        self.debugger.breakpoint_hit = true;
                        if (bp.callback)(self, bp.addr) == BreakpointCallbackAction::Remove {
                            remove.push(bp.handle);
                        }
                    }
                }
                std::mem::swap(&mut tmp, &mut self.debugger.breakpoints);
                for h in remove {
                    self.remove_breakpoint(h);
                }
                if self.debugger.breakpoint_hit {
                    return;
                }
            }
        }

        // Note: we may increment self.clock mid-instruction just so long
        // as we are sure we won't push the clock into the future (to allow
        // us to step the system mid-instruction).
        //
        // At the end of the instruction we will correct the final clock relative
        // to this start_cycle
        #[cfg(debug_assertions)]
        let start_cycle = self.clock;

        #[cfg(debug_assertions)]
        {
            self.non_instruction_cycles = 0;
            self.instruction_polled_interrupts = false;
        }

        #[cfg(feature="trace")]
        {
            self.trace.saved_a = self.a;
            self.trace.saved_x = self.x;
            self.trace.saved_y = self.y;
            self.trace.saved_sp = self.sp;
            self.trace.saved_p = self.p;
            self.trace.cycle_count = self.clock;
        }

        //println!("Fetching opcode");
        // Address where the instruction is placed
        let inst_pc = self.pc;
        let inst_code = self.pc_fetch_u8(system);

        let Instruction { op: opcode, mode, cyc: expected_cyc, early_intr_poll }  = Instruction::from(inst_code);
        //println!("start instruction {:#?}, pc = {inst_pc:04x}", opcode);

        #[cfg(debug_assertions)]
        {
            //println!("start instruction {:#?}, start clock = {}, non-instruction cycles = {}, expected clock = {}, actual = {}",
            //         opcode, start_cycle, self.non_instruction_cycles, start_cycle + (self.non_instruction_cycles as u64) + 1, self.clock);
            debug_assert_eq!(self.clock - (self.non_instruction_cycles as u64) - start_cycle, 1);
        }

        if early_intr_poll {
            // nesdev:
            //  "(like all two-cycle instructions they poll the interrupt lines at the end of the first cycle),"
            //
            //  Note: in addition to two-cycle instructions; the PLP instruction also polls early as do branch instructions
            //  ref:
            //    "The branch instructions have more subtle interrupt polling behavior. Interrupts are always polled
            //     before the second CPU cycle (the operand fetch)"
            self.instruction_poll_interrupts();
        }

        let cyc = match opcode {
            /* *************** binary op ***************  */
            // The result is stored in the a register, so the address of the operand is not used
            Opcode::ADC => {
                let (FetchedOperand { operand: _addr, oops_cyc, .. }, arg) = self.fetch_operand_and_value(system, mode, OopsHandling::Normal);

                let tmp = u16::from(self.a)
                    + u16::from(arg)
                    + (if self.carry_flag() { 1 } else { 0 });
                let result = (tmp & 0xff) as u8;

                let is_carry = tmp > 0x00ffu16;
                let is_zero = result == 0;
                let is_negative = (result & 0x80) == 0x80;
                let is_overflow = ((self.a ^ result) & (arg ^ result) & 0x80) == 0x80;

                self.set_carry_flag(is_carry);
                self.set_zero_flag(is_zero);
                self.set_negative_flag(is_negative);
                self.set_overflow_flag(is_overflow);
                self.a = result;
                expected_cyc + oops_cyc
            }
            Opcode::SBC => {
                let (FetchedOperand { operand: _addr, oops_cyc, .. }, arg) = self.fetch_operand_and_value(system, mode, OopsHandling::Normal);

                let (data1, is_carry1) = self.a.overflowing_sub(arg);
                let (result, is_carry2) =
                    data1.overflowing_sub(if self.carry_flag() { 0 } else { 1 });

                let is_carry = !(is_carry1 || is_carry2);
                let is_zero = result == 0;
                let is_negative = (result & 0x80) == 0x80;
                let is_overflow =
                    (((self.a ^ arg) & 0x80) == 0x80) && (((self.a ^ result) & 0x80) == 0x80);

                self.set_carry_flag(is_carry);
                self.set_zero_flag(is_zero);
                self.set_negative_flag(is_negative);
                self.set_overflow_flag(is_overflow);
                self.a = result;
                expected_cyc + oops_cyc
            }
            Opcode::AND => {
                let (FetchedOperand { operand: _addr, oops_cyc, .. }, arg) = self.fetch_operand_and_value(system, mode, OopsHandling::Normal);

                let result = self.a & arg;
                let is_zero = result == 0;
                let is_negative = (result & 0x80) == 0x80;

                self.set_zero_flag(is_zero);
                self.set_negative_flag(is_negative);
                self.a = result;
                expected_cyc + oops_cyc
            }
            Opcode::EOR => {
                let (FetchedOperand { operand: _addr, oops_cyc, .. }, arg) = self.fetch_operand_and_value(system, mode, OopsHandling::Normal);

                let result = self.a ^ arg;

                let is_zero = result == 0;
                let is_negative = (result & 0x80) == 0x80;

                self.set_zero_flag(is_zero);
                self.set_negative_flag(is_negative);
                self.a = result;
                expected_cyc + oops_cyc
            }
            Opcode::ORA => {
                let (FetchedOperand { operand: _addr, oops_cyc, .. }, arg) = self.fetch_operand_and_value(system, mode, OopsHandling::Normal);

                let result = self.a | arg;

                let is_zero = result == 0;
                let is_negative = (result & 0x80) == 0x80;

                self.set_zero_flag(is_zero);
                self.set_negative_flag(is_negative);
                self.a = result;

                expected_cyc + oops_cyc
            }
            Opcode::ASL => {
                let (FetchedOperand { operand: addr, ..}, arg) = self.fetch_operand_and_value(system, mode, OopsHandling::Always);

                if mode != AddressingMode::Accumulator {
                    self.dummy_write_system_bus(system, addr, arg); // redundant write while modifying
                }
                let result = arg.wrapping_shl(1);

                let is_carry = (arg & 0x80) == 0x80;
                let is_zero = result == 0;
                let is_negative = (result & 0x80) == 0x80;

                self.set_carry_flag(is_carry);
                self.set_zero_flag(is_zero);
                self.set_negative_flag(is_negative);

                if mode == AddressingMode::Accumulator {
                    self.a = result;
                } else {
                    self.write_system_bus(system, addr, result);
                }
                expected_cyc// + oops_cyc
            }
            Opcode::LSR => {
                let (FetchedOperand { operand: addr, ..}, arg) = self.fetch_operand_and_value(system, mode, OopsHandling::Always);

                if mode != AddressingMode::Accumulator {
                    self.dummy_write_system_bus(system, addr, arg); // redundant write while modifying
                }
                let result = arg.wrapping_shr(1);

                let is_carry = (arg & 0x01) == 0x01;
                let is_zero = result == 0;
                let is_negative = (result & 0x80) == 0x80;

                self.set_carry_flag(is_carry);
                self.set_zero_flag(is_zero);
                self.set_negative_flag(is_negative);

                if mode == AddressingMode::Accumulator {
                    self.a = result;
                } else {
                    self.write_system_bus(system, addr, result);
                }
                expected_cyc// + oops_cyc
            }
            Opcode::ROL => {
                let (FetchedOperand { operand: addr, ..}, arg) = self.fetch_operand_and_value(system, mode, OopsHandling::Always);

                if mode != AddressingMode::Accumulator {
                    self.dummy_write_system_bus(system, addr, arg); // redundant write while modifying
                }
                let result =
                    arg.wrapping_shl(1) | (if self.carry_flag() { 0x01 } else { 0x00 });

                let is_carry = (arg & 0x80) == 0x80;
                let is_zero = result == 0;
                let is_negative = (result & 0x80) == 0x80;

                self.set_carry_flag(is_carry);
                self.set_zero_flag(is_zero);
                self.set_negative_flag(is_negative);

                if mode == AddressingMode::Accumulator {
                    self.a = result;
                } else {
                    self.write_system_bus(system, addr, result);
                }
                expected_cyc// + oops_cyc
            }
            Opcode::ROR => {
                let (FetchedOperand { operand: addr, ..}, arg) = self.fetch_operand_and_value(system, mode, OopsHandling::Always);

                if mode != AddressingMode::Accumulator {
                    self.dummy_write_system_bus(system, addr, arg); // redundant write while modifying
                }
                let result =
                    arg.wrapping_shr(1) | (if self.carry_flag() { 0x80 } else { 0x00 });

                let is_carry = (arg & 0x01) == 0x01;
                let is_zero = result == 0;
                let is_negative = (result & 0x80) == 0x80;

                self.set_carry_flag(is_carry);
                self.set_zero_flag(is_zero);
                self.set_negative_flag(is_negative);

                if mode == AddressingMode::Accumulator {
                    self.a = result;
                } else {
                    self.write_system_bus(system, addr, result);
                }
                expected_cyc// + oops_cyc
            }
            Opcode::INC => {
                let (FetchedOperand { operand: addr, ..}, arg) = self.fetch_operand_and_value(system, mode, OopsHandling::Always);

                self.dummy_write_system_bus(system, addr, arg); // redundant write while modifying
                let result = arg.wrapping_add(1);

                let is_zero = result == 0;
                let is_negative = (result & 0x80) == 0x80;

                self.set_zero_flag(is_zero);
                self.set_negative_flag(is_negative);

                self.write_system_bus(system, addr, result);
                expected_cyc// + oops_cyc
            }
            Opcode::INX => {
                self.nop_pc_fetch_u8(system); // discard operand

                let result = self.x.wrapping_add(1);

                let is_zero = result == 0;
                let is_negative = (result & 0x80) == 0x80;

                self.set_zero_flag(is_zero);
                self.set_negative_flag(is_negative);
                self.x = result;
                expected_cyc
            }
            Opcode::INY => {
                self.nop_pc_fetch_u8(system); // discard operand

                let result = self.y.wrapping_add(1);

                let is_zero = result == 0;
                let is_negative = (result & 0x80) == 0x80;

                self.set_zero_flag(is_zero);
                self.set_negative_flag(is_negative);
                self.y = result;
                expected_cyc
            }
            Opcode::DEC => {
                let (FetchedOperand { operand: addr, ..}, arg) = self.fetch_operand_and_value(system, mode, OopsHandling::Always);

                self.dummy_write_system_bus(system, addr, arg); // redundant write while modifying
                let result = arg.wrapping_sub(1);

                let is_zero = result == 0;
                let is_negative = (result & 0x80) == 0x80;

                self.set_zero_flag(is_zero);
                self.set_negative_flag(is_negative);

                self.write_system_bus(system, addr, result);
                expected_cyc// + oops_cyc
            }
            Opcode::DEX => {
                self.nop_pc_fetch_u8(system); // discard operand

                let result = self.x.wrapping_sub(1);

                let is_zero = result == 0;
                let is_negative = (result & 0x80) == 0x80;

                self.set_zero_flag(is_zero);
                self.set_negative_flag(is_negative);
                self.x = result;
                2
            }
            Opcode::DEY => {
                self.nop_pc_fetch_u8(system); // discard operand

                let result = self.y.wrapping_sub(1);

                let is_zero = result == 0;
                let is_negative = (result & 0x80) == 0x80;

                self.set_zero_flag(is_zero);
                self.set_negative_flag(is_negative);
                self.y = result;
                expected_cyc
            }

            /* *************** load/store op ***************  */
            Opcode::LDA => {
                let (FetchedOperand { operand: _addr, oops_cyc, .. }, arg) = self.fetch_operand_and_value(system, mode, OopsHandling::Normal);

                let is_zero = arg == 0;
                let is_negative = (arg & 0x80) == 0x80;

                self.set_zero_flag(is_zero);
                self.set_negative_flag(is_negative);
                self.a = arg;
                expected_cyc + oops_cyc
            }
            Opcode::LDX => {
                let (FetchedOperand { operand: _addr, oops_cyc, .. }, arg) = self.fetch_operand_and_value(system, mode, OopsHandling::Normal);

                let is_zero = arg == 0;
                let is_negative = (arg & 0x80) == 0x80;

                self.set_zero_flag(is_zero);
                self.set_negative_flag(is_negative);
                self.x = arg;
                expected_cyc + oops_cyc
            }
            Opcode::LDY => {
                let (FetchedOperand { operand: _addr, oops_cyc, .. }, arg) = self.fetch_operand_and_value(system, mode, OopsHandling::Normal);

                let is_zero = arg == 0;
                let is_negative = (arg & 0x80) == 0x80;

                self.set_zero_flag(is_zero);
                self.set_negative_flag(is_negative);
                self.y = arg;
                expected_cyc + oops_cyc
            }
            Opcode::STA => {
                let FetchedOperand { operand: addr, .. } = self.fetch_operand(system, mode, OopsHandling::Always);

                self.write_system_bus(system, addr, self.a);
                expected_cyc// + oops_cyc
            }
            Opcode::STX => {
                let FetchedOperand { operand: addr, oops_cyc, .. } = self.fetch_operand(system, mode, OopsHandling::Always);

                self.write_system_bus(system, addr, self.x);
                expected_cyc + oops_cyc
            }
            Opcode::STY => {
                let FetchedOperand { operand: addr, oops_cyc, .. } = self.fetch_operand(system, mode, OopsHandling::Always);

                self.write_system_bus(system, addr, self.y);
                expected_cyc + oops_cyc
            }

            /* *************** set/clear flag ***************  */
            Opcode::SEC => {
                self.nop_pc_fetch_u8(system); // discard operand
                self.set_carry_flag(true);
                expected_cyc
            }
            Opcode::SED => {
                self.nop_pc_fetch_u8(system); // discard operand
                self.set_decimal_flag(true);
                expected_cyc
            }
            Opcode::SEI => {
                self.nop_pc_fetch_u8(system); // discard operand
                self.set_interrupt_flag(true);
                expected_cyc
            }
            Opcode::CLC => {
                self.nop_pc_fetch_u8(system); // discard operand
                self.set_carry_flag(false);
                expected_cyc
            }
            Opcode::CLD => {
                self.nop_pc_fetch_u8(system); // discard operand
                self.set_decimal_flag(false);
                expected_cyc
            }
            Opcode::CLI => {
                self.nop_pc_fetch_u8(system); // discard operand
                self.set_interrupt_flag(false);
                expected_cyc
            }
            Opcode::CLV => {
                self.nop_pc_fetch_u8(system); // discard operand
                self.set_overflow_flag(false);
                expected_cyc
            }

            /* *************** compare ***************  */
            Opcode::CMP => {
                let (FetchedOperand { operand: _addr, oops_cyc, .. }, arg) = self.fetch_operand_and_value(system, mode, OopsHandling::Normal);

                let (result, _) = self.a.overflowing_sub(arg);

                let is_carry = self.a >= arg;
                let is_zero = result == 0;
                let is_negative = (result & 0x80) == 0x80;

                self.set_carry_flag(is_carry);
                self.set_zero_flag(is_zero);
                self.set_negative_flag(is_negative);
                expected_cyc + oops_cyc
            }
            Opcode::CPX => {
                let (FetchedOperand { operand: _addr, oops_cyc, .. }, arg) = self.fetch_operand_and_value(system, mode, OopsHandling::Normal);

                let (result, _) = self.x.overflowing_sub(arg);

                let is_carry = self.x >= arg;
                let is_zero = result == 0;
                let is_negative = (result & 0x80) == 0x80;

                self.set_carry_flag(is_carry);
                self.set_zero_flag(is_zero);
                self.set_negative_flag(is_negative);
                expected_cyc + oops_cyc
            }
            Opcode::CPY => {
                let (FetchedOperand { operand: _addr, oops_cyc, .. }, arg) = self.fetch_operand_and_value(system, mode, OopsHandling::Normal);

                let (result, _) = self.y.overflowing_sub(arg);

                let is_carry = self.y >= arg;
                let is_zero = result == 0;
                let is_negative = (result & 0x80) == 0x80;

                self.set_carry_flag(is_carry);
                self.set_zero_flag(is_zero);
                self.set_negative_flag(is_negative);
                expected_cyc + oops_cyc
            }

            /* *************** jump/return ***************  */
            // JMP: Absolute or Indirect, JSR: Absolute, RTI,RTS: Implied
            Opcode::JMP => {
                let FetchedOperand { operand: addr, oops_cyc, .. } = self.fetch_operand(system, mode, OopsHandling::Normal);
                self.pc = addr;
                expected_cyc + oops_cyc
            }
            Opcode::JSR => {
                debug_assert_eq!(mode, AddressingMode::Absolute);

                // This instruction splits the operand fetch such that the higher byte of the absolute address
                // is read after the the program counter has been saved to the stack so we don't use fetch_operand() here.

                //let FetchedOperand { operand: addr, oops_cyc: _, .. } = self.fetch_operand(system, mode, OopsHandling::Normal);
                let addr_lo = self.pc_fetch_u8(system) as u16;

                self.dummy_read_system_bus(system, self.sp as u16 + 0x100); // "internal operation (predecrement S?)"

                let opcode_addr = inst_pc;
                let ret_addr = opcode_addr + 2;
                self.stack_push(system, (ret_addr >> 8) as u8, StackByteTags::ADDR_HI);
                self.stack_push(system, (ret_addr & 0xff) as u8, StackByteTags::ADDR_LO);

                let addr_hi = (self.pc_fetch_u8(system) as u16) << 8;
                self.pc = addr_hi | addr_lo;

                expected_cyc
            }
            Opcode::RTI => {
                self.nop_pc_fetch_u8(system); // read next instruction byte but throw it away

                self.dummy_read_system_bus(system, self.sp as u16 + 0x100); // dummy read while incrementing stack pointer S
                self.p = unsafe { Flags::from_bits_unchecked(self.stack_pop(system)) & Flags::REAL };
                let pc_lower = self.stack_pop(system);
                let pc_upper = self.stack_pop(system);
                self.pc = ((pc_upper as u16) << 8) | (pc_lower as u16);
                expected_cyc
            }
            Opcode::RTS => {
                self.nop_pc_fetch_u8(system); // read next instruction byte but throw it away
                self.dummy_read_system_bus(system, self.sp as u16 + 0x100); // dummy read while incrementing stack pointer S
                let pc_lower = self.stack_pop(system);
                let pc_upper = self.stack_pop(system);

                self.pc = (((pc_upper as u16) << 8) | (pc_lower as u16)) + 1;
                self.dummy_read_system_bus(system, self.pc); // dummy read while incrementing PC

                expected_cyc
            }

            /* *************** branch ***************  */
            Opcode::BCC => {
                debug_assert!(mode == AddressingMode::Relative);
                if !self.carry_flag() {
                    let FetchedOperand { operand: addr, oops_cyc, .. } = self.fetch_operand(system, mode, OopsHandling::Normal);
                    self.pc = addr;
                    if oops_cyc != 0 { // "Additionally, for taken branches that cross a page boundary, interrupts are polled before the PCH fixup cycle"
                        self.instruction_poll_interrupts();
                    }
                    self.nop_pc_fetch_u8(system);
                    expected_cyc + oops_cyc + 1
                } else {
                    let FetchedOperand { .. } = self.fetch_operand(system, mode, OopsHandling::Ignore);
                    expected_cyc
                }
            }
            Opcode::BCS => {
                debug_assert!(mode == AddressingMode::Relative);
                if self.carry_flag() {
                    let FetchedOperand { operand: addr, oops_cyc, .. } = self.fetch_operand(system, mode, OopsHandling::Normal);
                    self.pc = addr;
                    if oops_cyc != 0 { // "Additionally, for taken branches that cross a page boundary, interrupts are polled before the PCH fixup cycle"
                        self.instruction_poll_interrupts();
                    }
                    self.nop_pc_fetch_u8(system);
                    expected_cyc + oops_cyc + 1
                } else {
                    let FetchedOperand { .. } = self.fetch_operand(system, mode, OopsHandling::Ignore);
                    expected_cyc
                }
            }
            Opcode::BEQ => {
                debug_assert!(mode == AddressingMode::Relative);
                if self.zero_flag() {
                    let FetchedOperand { operand: addr, oops_cyc, .. } = self.fetch_operand(system, mode, OopsHandling::Normal);
                    self.pc = addr;
                    if oops_cyc != 0 { // "Additionally, for taken branches that cross a page boundary, interrupts are polled before the PCH fixup cycle"
                        self.instruction_poll_interrupts();
                    }
                    self.nop_pc_fetch_u8(system);
                    expected_cyc + oops_cyc + 1
                } else {
                    let FetchedOperand { .. } = self.fetch_operand(system, mode, OopsHandling::Ignore);
                    expected_cyc
                }
            }
            Opcode::BNE => {
                debug_assert!(mode == AddressingMode::Relative);
                if !self.zero_flag() {
                    let FetchedOperand { operand: addr, oops_cyc, .. } = self.fetch_operand(system, mode, OopsHandling::Normal);
                    self.pc = addr;
                    if oops_cyc != 0 { // "Additionally, for taken branches that cross a page boundary, interrupts are polled before the PCH fixup cycle"
                        self.instruction_poll_interrupts();
                    }
                    self.nop_pc_fetch_u8(system);
                    expected_cyc + oops_cyc + 1
                } else {
                    let FetchedOperand { .. } = self.fetch_operand(system, mode, OopsHandling::Ignore);
                    expected_cyc
                }
            }
            Opcode::BMI => {
                debug_assert!(mode == AddressingMode::Relative);
                if self.negative_flag() {
                    let FetchedOperand { operand: addr, oops_cyc, .. } = self.fetch_operand(system, mode, OopsHandling::Normal);
                    self.pc = addr;
                    if oops_cyc != 0 { // "Additionally, for taken branches that cross a page boundary, interrupts are polled before the PCH fixup cycle"
                        self.instruction_poll_interrupts();
                    }
                    self.nop_pc_fetch_u8(system);
                    expected_cyc + oops_cyc + 1
                } else {
                    let FetchedOperand { .. } = self.fetch_operand(system, mode, OopsHandling::Ignore);
                    expected_cyc
                }
            }
            Opcode::BPL => {
                debug_assert!(mode == AddressingMode::Relative);
                if !self.negative_flag() {
                    let FetchedOperand { operand: addr, oops_cyc, .. } = self.fetch_operand(system, mode, OopsHandling::Normal);
                    self.pc = addr;
                    if oops_cyc != 0 { // "Additionally, for taken branches that cross a page boundary, interrupts are polled before the PCH fixup cycle"
                        self.instruction_poll_interrupts();
                    }
                    self.nop_pc_fetch_u8(system);
                    expected_cyc + oops_cyc + 1
                } else {
                    let FetchedOperand { .. } = self.fetch_operand(system, mode, OopsHandling::Ignore);
                    expected_cyc
                }
            }
            Opcode::BVC => {
                debug_assert!(mode == AddressingMode::Relative);
                if !self.overflow_flag() {
                    let FetchedOperand { operand: addr, oops_cyc, .. } = self.fetch_operand(system, mode, OopsHandling::Normal);
                    self.pc = addr;
                    if oops_cyc != 0 { // "Additionally, for taken branches that cross a page boundary, interrupts are polled before the PCH fixup cycle"
                        self.instruction_poll_interrupts();
                    }
                    self.nop_pc_fetch_u8(system);
                    expected_cyc + oops_cyc + 1
                } else {
                    let FetchedOperand { .. } = self.fetch_operand(system, mode, OopsHandling::Ignore);
                    expected_cyc
                }
            }
            Opcode::BVS => {
                debug_assert!(mode == AddressingMode::Relative);
                if self.overflow_flag() {
                    let FetchedOperand { operand: addr, oops_cyc, .. } = self.fetch_operand(system, mode, OopsHandling::Normal);
                    self.pc = addr;
                    if oops_cyc != 0 { // "Additionally, for taken branches that cross a page boundary, interrupts are polled before the PCH fixup cycle"
                        self.instruction_poll_interrupts();
                    }
                    self.nop_pc_fetch_u8(system);
                    expected_cyc + oops_cyc + 1
                } else {
                    let FetchedOperand { .. } = self.fetch_operand(system, mode, OopsHandling::Ignore);
                    expected_cyc
                }
            }

            /* *************** push/pop ***************  */
            Opcode::PHA => {
                self.nop_pc_fetch_u8(system); // discard operand

                self.stack_push(system, self.a, StackByteTags::A);
                expected_cyc
            }
            Opcode::PHP => {
                self.nop_pc_fetch_u8(system); // discard operand

                self.stack_push(system, (self.p | Flags::BREAK_HIGH | Flags::BREAK_LOW).bits(), StackByteTags::STATUS);
                expected_cyc
            }
            Opcode::PLA => {
                self.nop_pc_fetch_u8(system); // discard operand

                self.dummy_read_system_bus(system, self.sp as u16 + 0x100); // increment stack pointer S
                let result = self.stack_pop(system);

                let is_zero = result == 0;
                let is_negative = (result & 0x80) == 0x80;

                self.set_zero_flag(is_zero);
                self.set_negative_flag(is_negative);
                self.a = result;
                expected_cyc
            }
            Opcode::PLP => {
                self.nop_pc_fetch_u8(system); // discard operand

                self.dummy_read_system_bus(system, self.sp as u16 + 0x100); // increment stack pointer S
                self.p = unsafe { Flags::from_bits_unchecked(self.stack_pop(system)) & Flags::REAL };
                //println!("Status after PLP = {:?}", self.p);
                expected_cyc
            }

            /* *************** transfer ***************  */
            Opcode::TAX => {
                self.nop_pc_fetch_u8(system); // discard operand
                let is_zero = self.a == 0;
                let is_negative = (self.a & 0x80) == 0x80;

                self.set_zero_flag(is_zero);
                self.set_negative_flag(is_negative);
                self.x = self.a;
                expected_cyc
            }
            Opcode::TAY => {
                self.nop_pc_fetch_u8(system); // discard operand
                let is_zero = self.a == 0;
                let is_negative = (self.a & 0x80) == 0x80;

                self.set_zero_flag(is_zero);
                self.set_negative_flag(is_negative);
                self.y = self.a;
                expected_cyc
            }
            Opcode::TSX => {
                self.nop_pc_fetch_u8(system); // discard operand
                let result = (self.sp & 0xff) as u8;

                let is_zero = result == 0;
                let is_negative = (result & 0x80) == 0x80;

                self.set_zero_flag(is_zero);
                self.set_negative_flag(is_negative);
                self.x = result;
                expected_cyc
            }
            Opcode::TXA => {
                self.nop_pc_fetch_u8(system); // discard operand
                let is_zero = self.x == 0;
                let is_negative = (self.x & 0x80) == 0x80;

                self.set_zero_flag(is_zero);
                self.set_negative_flag(is_negative);
                self.a = self.x;
                expected_cyc
            }
            Opcode::TXS => {
                self.nop_pc_fetch_u8(system); // discard operand
                // txs does not rewrite status
                self.sp = self.x;
                expected_cyc
            }
            Opcode::TYA => {
                self.nop_pc_fetch_u8(system); // discard operand
                let is_zero = self.y == 0;
                let is_negative = (self.y & 0x80) == 0x80;

                self.set_zero_flag(is_zero);
                self.set_negative_flag(is_negative);
                self.a = self.y;
                expected_cyc
            }

            /* *************** other ***************  */
            Opcode::BRK => {
                self.nop_pc_fetch_u8(system); // discard operand, increment PC
                self.pc += 1;

                // We rely on self.handle_interrupts() being called at the end
                // of step_instruction() if there is a _handler_pending.
                //
                // If we were to call .handle_interrupts directly here we might
                // setup the program counter for our interrupt handler but then
                // poll interrupts and call handle_interrupt() _again_ before we
                // return from step_instruction()

                if self.interrupt_handler_pending.is_none() {
                    self.interrupt_handler_pending = Some(Interrupt::BRK);
                }
                expected_cyc
            }
            Opcode::BIT => {
                // ZeroPage or Absolute
                // Requires non-destructive read, so don't call fetch_and_deref...
                let FetchedOperand { operand: addr, oops_cyc, .. } = self.fetch_operand(system, mode, OopsHandling::Normal);

                let arg = self.read_system_bus(system, addr);

                #[cfg(feature="trace")]
                {
                    self.trace.loaded_mem_value = arg;
                }

                let is_negative = (arg & 0x80) == 0x80;
                let is_overflow = (arg & 0x40) == 0x40;
                let is_zero = (self.a & arg) == 0x00;

                self.set_negative_flag(is_negative);
                self.set_zero_flag(is_zero);
                self.set_overflow_flag(is_overflow);
                expected_cyc + oops_cyc
            }
            Opcode::NOP => {
                let (FetchedOperand { .. }, _arg) = self.fetch_operand_and_value(system, mode, OopsHandling::Normal);
                expected_cyc
            }
            /* *************** unofficial1 ***************  */

            Opcode::AAC => {
                // AND byte with accumulator. If result is negative then carry is set. Status flags: N,Z,C

                debug_assert!(mode == AddressingMode::Immediate);
                let (FetchedOperand { operand: _addr, oops_cyc, .. }, arg) = self.fetch_operand_and_value(system, mode, OopsHandling::Normal);

                let result = self.a & arg;
                let is_zero = result == 0;
                let is_negative = (result & 0x80) == 0x80;

                self.set_zero_flag(is_zero);
                self.set_negative_flag(is_negative);
                self.set_carry_flag(is_negative);
                self.a = result;
                expected_cyc + oops_cyc
            }
            Opcode::AAX => {
                let FetchedOperand { operand: addr, oops_cyc, .. } = self.fetch_operand(system, mode, OopsHandling::Always);

                let result = self.a & self.x;

                #[cfg(feature="trace")]
                {
                    self.trace.stored_mem_value = result;
                }
                self.write_system_bus(system, addr, result);
                expected_cyc + oops_cyc
            }
            Opcode::ARR => {
                debug_assert!(mode == AddressingMode::Immediate);
                let (_, arg) = self.fetch_operand_and_value(system, mode, OopsHandling::Normal);

                let src = self.a & arg;
                let result =
                    src.wrapping_shr(1) | (if self.carry_flag() { 0x80 } else { 0x00 });

                let is_zero = result == 0;
                let is_negative = (result & 0x80) == 0x80;
                let is_carry = (result & 0x40) == 0x40;
                let is_overflow = ((result & 0x40) ^ ((result & 0x20) << 1)) == 0x40;

                self.set_zero_flag(is_zero);
                self.set_negative_flag(is_negative);
                self.set_carry_flag(is_carry);
                self.set_overflow_flag(is_overflow);

                self.a = result;
                expected_cyc
            }
            Opcode::ASR => {
                debug_assert!(mode == AddressingMode::Immediate);
                let (_, arg) = self.fetch_operand_and_value(system, mode, OopsHandling::Normal);

                let src = self.a & arg;
                let result = src.wrapping_shr(1);

                let is_carry = (src & 0x01) == 0x01;
                let is_zero = result == 0;
                let is_negative = (result & 0x80) == 0x80;

                self.set_carry_flag(is_carry);
                self.set_zero_flag(is_zero);
                self.set_negative_flag(is_negative);

                self.a = result;
                expected_cyc
            }
            Opcode::ATX => {

                // Conflicting information:
                // From https://www.nesdev.com/undocumented_opcodes.txt it says:
                //      AND byte with accumulator, then transfer accumulator to X register. Status flags: N,Z
                // Looking at the implementation of Mesen they implement ATX as:
                //      Store the immediate in A and X and update the N + Z flags.
                // The instr_test-v5 tests pass for the later interpretation, so that's
                // what we implement here.

                debug_assert!(mode == AddressingMode::Immediate);
                let (_, arg) = self.fetch_operand_and_value(system, mode, OopsHandling::Normal);

                self.a = arg;
                self.x = arg;

                let is_zero = arg == 0;
                let is_negative = (arg & 0x80) == 0x80;

                self.set_zero_flag(is_zero);
                self.set_negative_flag(is_negative);

                expected_cyc
            },
            Opcode::AXA => {
                // http://www.ffd2.com/fridge/docs/6502-NMOS.extra.opcodes
                // This opcode stores the result of A AND X AND the high byte of the target
                // address of the operand +1 in memory.

                let FetchedOperand { operand: addr, .. } = self.fetch_operand(system, mode, OopsHandling::Always);

                let high = (addr >> 8) as u8;
                let result = self.a & self.x & high.wrapping_add(1);

                #[cfg(feature="trace")]
                {
                    self.trace.stored_mem_value = result;
                }
                self.write_system_bus(system, addr, result);

                expected_cyc// + oops_cyc
            },
            Opcode::AXS => { // Sometimes called SAX
                // From http://www.ffd2.com/fridge/docs/6502-NMOS.extra.opcodes (called SAX):
                //    SAX ANDs the contents of the A and X registers (leaving the contents of A
                //    intact), subtracts an immediate value, and then stores the result in X.
                //    ... A few points might be made about the action of subtracting an immediate
                //    value.  It actually works just like the CMP instruction, except that CMP
                //    does not store the result of the subtraction it performs in any register.
                //    This subtract operation is not affected by the state of the Carry flag,
                //    though it does affect the Carry flag.  It does not affect the Overflow
                //    flag.

                debug_assert!(mode == AddressingMode::Immediate);
                let (_, arg) = self.fetch_operand_and_value(system, mode, OopsHandling::Normal);

                let result = self.a & self.x;
                let (result, overflow) = result.overflowing_sub(arg);

                let is_zero = result == 0;
                let is_negative = (result & 0x80) == 0x80;

                self.set_carry_flag(!overflow);
                self.set_zero_flag(is_zero);
                self.set_negative_flag(is_negative);
                self.x = result;

                expected_cyc
            }
            Opcode::LAR => {
                let (FetchedOperand { oops_cyc, .. }, _arg) = self.fetch_operand_and_value(system, mode, OopsHandling::Normal);
                // TODO
                expected_cyc + oops_cyc
            },
            Opcode::LAX => {
                let (FetchedOperand { operand: _addr, oops_cyc, .. }, arg) = self.fetch_operand_and_value(system, mode, OopsHandling::Normal);

                let is_zero = arg == 0;
                let is_negative = (arg & 0x80) == 0x80;

                self.set_zero_flag(is_zero);
                self.set_negative_flag(is_negative);
                self.a = arg;
                self.x = arg;

                expected_cyc + oops_cyc
            }
            Opcode::DCP => {
                let (FetchedOperand { operand: addr, ..}, arg) = self.fetch_operand_and_value(system, mode, OopsHandling::Always);

                self.dummy_write_system_bus(system, addr, arg); // redundant write while decrementing
                let dec_result = arg.wrapping_sub(1);

                #[cfg(feature="trace")]
                {
                    self.trace.stored_mem_value = dec_result;
                }
                self.write_system_bus(system, addr, dec_result);

                // CMP
                let result = self.a.wrapping_sub(dec_result);

                let is_carry = self.a >= dec_result;
                let is_zero = result == 0;
                let is_negative = (result & 0x80) == 0x80;

                self.set_carry_flag(is_carry);
                self.set_zero_flag(is_zero);
                self.set_negative_flag(is_negative);
                expected_cyc// + oops_cyc
            }
            Opcode::DOP => {
                // Fetch but do nothing
                let (FetchedOperand { operand: _addr, oops_cyc, .. }, _arg) = self.fetch_operand_and_value(system, mode, OopsHandling::Normal);
                expected_cyc + oops_cyc
            }
            Opcode::ISC => {
                let (FetchedOperand { operand: addr, ..}, arg) = self.fetch_operand_and_value(system, mode, OopsHandling::Always);

                self.dummy_write_system_bus(system, addr, arg); // redundant write while modifying
                let inc_result = arg.wrapping_add(1);

                self.write_system_bus(system, addr, inc_result);

                let (data1, is_carry1) = self.a.overflowing_sub(inc_result);
                let (result, is_carry2) =
                    data1.overflowing_sub(if self.carry_flag() { 0 } else { 1 });

                let is_carry = !(is_carry1 || is_carry2);
                let is_zero = result == 0;
                let is_negative = (result & 0x80) == 0x80;
                let is_overflow = (((self.a ^ inc_result) & 0x80) == 0x80)
                    && (((self.a ^ result) & 0x80) == 0x80);

                self.set_carry_flag(is_carry);
                self.set_zero_flag(is_zero);
                self.set_negative_flag(is_negative);
                self.set_overflow_flag(is_overflow);
                self.a = result;
                expected_cyc// + oops_cyc
            }
            Opcode::RLA => {
                let (FetchedOperand { operand: addr, ..}, arg) = self.fetch_operand_and_value(system, mode, OopsHandling::Always);

                self.dummy_write_system_bus(system, addr, arg); // redundant write while shifting
                let result_rol =
                    arg.wrapping_shl(1) | (if self.carry_flag() { 0x01 } else { 0x00 });

                let is_carry = (arg & 0x80) == 0x80;
                self.set_carry_flag(is_carry);

                self.write_system_bus(system, addr, result_rol);

                let result_and = self.a & result_rol;

                let is_zero = result_and == 0;
                let is_negative = (result_and & 0x80) == 0x80;

                self.set_zero_flag(is_zero);
                self.set_negative_flag(is_negative);

                self.a = result_and;

                expected_cyc// + oops_cyc
            }
            Opcode::RRA => {
                let (FetchedOperand { operand: addr, ..}, arg) = self.fetch_operand_and_value(system, mode, OopsHandling::Always);

                self.dummy_write_system_bus(system, addr, arg); // redundant write while shifting
                let result_ror =
                    arg.wrapping_shr(1) | (if self.carry_flag() { 0x80 } else { 0x00 });

                let is_carry_ror = (arg & 0x01) == 0x01;
                self.set_carry_flag(is_carry_ror);

                self.write_system_bus(system, addr, result_ror);

                let tmp = u16::from(self.a)
                    + u16::from(result_ror)
                    + (if self.carry_flag() { 1 } else { 0 });
                let result_adc = (tmp & 0xff) as u8;

                let is_carry = tmp > 0x00ffu16;
                let is_zero = result_adc == 0;
                let is_negative = (result_adc & 0x80) == 0x80;
                let is_overflow =
                    ((self.a ^ result_adc) & (result_ror ^ result_adc) & 0x80) == 0x80;

                self.set_carry_flag(is_carry);
                self.set_zero_flag(is_zero);
                self.set_negative_flag(is_negative);
                self.set_overflow_flag(is_overflow);
                self.a = result_adc;

                expected_cyc// + oops_cyc
            }
            Opcode::SLO => {
                let (FetchedOperand { operand: addr, ..}, arg) = self.fetch_operand_and_value(system, mode, OopsHandling::Always);

                self.dummy_write_system_bus(system, addr, arg); // redundant write while shifting
                let result_asl = arg.wrapping_shl(1);

                let is_carry = (arg & 0x80) == 0x80;
                self.set_carry_flag(is_carry);

                self.write_system_bus(system, addr, result_asl);

                let result_ora = self.a | result_asl;

                let is_zero = result_ora == 0;
                let is_negative = (result_ora & 0x80) == 0x80;

                self.set_zero_flag(is_zero);
                self.set_negative_flag(is_negative);
                self.a = result_ora;

                expected_cyc// + oops_cyc
            }
            Opcode::SRE => {
                let (FetchedOperand { operand: addr, ..}, arg) = self.fetch_operand_and_value(system, mode, OopsHandling::Always);

                self.dummy_write_system_bus(system, addr, arg); // redundant write while shifting
                let result_lsr = arg.wrapping_shr(1);

                let is_carry = (arg & 0x01) == 0x01;
                self.set_carry_flag(is_carry);

                self.write_system_bus(system, addr, result_lsr);

                let result_eor = self.a ^ result_lsr;

                let is_zero = result_eor == 0;
                let is_negative = (result_eor & 0x80) == 0x80;

                self.set_zero_flag(is_zero);
                self.set_negative_flag(is_negative);
                self.a = result_eor;

                expected_cyc// + oops_cyc
            }
            Opcode::SXA => {
                // Conflicting information
                //      http://www.ffd2.com/fridge/docs/6502-NMOS.extra.opcodes:
                //          This opcode ANDs the contents of the X register with <ab+1> and stores the result in memory.
                //          (where 'ab' is the high byte of the address)
                //      Mesen implements this but additionally modifies the address to
                //      by using the result as a replacement high byte for the address.
                //
                // For now the implementation uses the same logic as Mesen, since that
                // passes existing tests

                let FetchedOperand { operand: addr, .. } = self.fetch_operand(system, mode, OopsHandling::Always);

                let high = (addr >> 8) as u8;
                let low = (addr & 0xff) as u8;
                let result = self.x & high.wrapping_add(1);
                let addr = ((result as u16) << 8) | low as u16;

                self.write_system_bus(system, addr, result);

                expected_cyc// + oops_cyc
            },
            Opcode::SYA => {
                // Conflicting information
                //      http://www.ffd2.com/fridge/docs/6502-NMOS.extra.opcodes:
                //          This opcode ANDs the contents of the Y register with <ab+1> and stores the result in memory.
                //          (where 'ab' is the high byte of the address)
                //      Mesen implements this but additionally modifies the address to
                //      by using the result as a replacement high byte for the address.
                //
                // For now the implementation uses the same logic as Mesen, since that
                // passes existing tests

                let FetchedOperand { operand: addr, .. } = self.fetch_operand(system, mode, OopsHandling::Always);

                let high = (addr >> 8) as u8;
                let low = (addr & 0xff) as u8;
                let result = self.y & high.wrapping_add(1);
                let addr = ((result as u16) << 8) | low as u16;

                self.write_system_bus(system, addr, result);

                expected_cyc// + oops_cyc
            },
            Opcode::TOP => {
                // Fetch but do nothing
                let (FetchedOperand { operand: _addr, oops_cyc, .. }, _arg) = self.fetch_operand_and_value(system, mode, OopsHandling::Normal);
                expected_cyc + oops_cyc
            }
            Opcode::XAA => {
                let (FetchedOperand { .. }, _arg) = self.fetch_operand_and_value(system, mode, OopsHandling::Normal);
                // TODO
                expected_cyc
            },
            Opcode::XAS => {
                let (FetchedOperand { .. }, _arg) = self.fetch_operand_and_value(system, mode, OopsHandling::Normal);
                // TODO
                expected_cyc
            },

            Opcode::HALT => {
                panic!("Invalid opcode {inst_code:02x}");
            }
        };

        // "The output from the edge detector and level detector are polled at
        // certain points to detect pending interrupts. For most instructions,
        // this polling happens during the final cycle of the instruction,
        // before the opcode fetch for the next instruction. If the polling
        // operation detects that an interrupt has been asserted, the next
        // "instruction" executed is the interrupt sequence"
        //
        // Note: two-cycle instructions are handled at the start of the instruction:
        //  "(like all two-cycle instructions they poll the interrupt lines at the end of the first cycle)"
        // Note: Relative mode instructions are handled as part of the branch instructions since:
        //  "The branch instructions have more subtle interrupt polling behavior."
        if !early_intr_poll {
            self.instruction_poll_interrupts();
        }

        #[cfg(feature="trace")]
        {
            //debug_assert!(cyc == expected_cyc);
            self.trace.instruction = Instruction { op: opcode, mode, cyc: expected_cyc, early_intr_poll };
            self.trace.instruction_pc = inst_pc;
            self.trace.instruction_op_code = inst_code;
        }

        // Cross-reference our predicted cycle count with the actual read/write cycle count
        // (ignoring cycles that elapsed while suspended for the DMA unit)
        #[cfg(debug_assertions)]
        {
            let final_clock = start_cycle + (self.non_instruction_cycles as u64) + cyc as u64;
            //println!("end instruction {:#?}, start was = {}, non-instruction was = {}, cyc = {}, expected clock = {}, actual clock = {}, n cycles = {}",
            //         opcode, start_cycle, self.non_instruction_cycles, cyc, final_clock, self.clock, final_clock - start_cycle - (self.non_instruction_cycles as u64));

            if final_clock < self.clock {
                panic!("Did too many reads/writes for instruction {:?}, op = {}, start = {}, non-instruction = {}, clock = {}, cyc = {}, final = {}",
                    Instruction { op: opcode, mode, cyc: expected_cyc, early_intr_poll },
                    inst_code,
                    start_cycle,
                    self.non_instruction_cycles,
                    self.clock,
                    cyc,
                    final_clock
                );
            }

            // Ideally we want to get to the point where this is always zero but we still have some
            // under accounting of reads/writes (which are the only things that step the clock
            // mid-instruction) and so may have to catch up on some cycles before we return
            let fixup_delta = final_clock - self.clock;
            if fixup_delta > 0 {
                log::warn!("fixup cycles ({}) needed for instruction {:?} @ {inst_pc:04x}", fixup_delta, Instruction { op: opcode, mode, cyc: expected_cyc, early_intr_poll });
                for _ in 0..fixup_delta {
                    self.start_clock_cycle_phi1(system);
                    system.step_for_cpu_cycle();
                    self.end_clock_cycle_phi2(system);
                    self.clock += 1;
                }
            }

            debug_assert_eq!(self.instruction_polled_interrupts, true);
        }

        if let Some(interrupt_type) = self.interrupt_handler_pending {
            self.handle_interrupt(system, interrupt_type);
        }
    }
}
