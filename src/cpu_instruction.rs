use super::cpu::*;
use super::interface::SystemBus;
use super::system::System;

#[derive(PartialEq, Eq, Copy, Clone, Debug)]
enum Opcode {
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
    ALR,
    ANC,
    ARR,
    AXS,
    LAX,
    SAX,
    DCP,
    ISC,
    RLA,
    RRA,
    SLO,
    SRE,
    SKB,
    IGN,
    // unofficial2
    //ADC, SBC, NOP,
}

#[derive(PartialEq, Eq, Copy, Clone, Debug)]
enum AddressingMode {
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
    Indirect,
    IndirectX,
    IndirectY,
}
#[derive(Copy, Clone)]
struct Operand { data: u16, cyc: u8 }

#[derive(Copy, Clone, Debug)]
struct Instruction { op: Opcode, mode: AddressingMode }

impl Instruction {
    /// romのコードを命令に変換します
    pub fn from(inst_code: u8) -> Instruction {
        match inst_code {
            /* *************** binary op ***************  */
            0x69 => Instruction { op: Opcode::ADC, mode: AddressingMode::Immediate },
            0x65 => Instruction { op: Opcode::ADC, mode: AddressingMode::ZeroPage },
            0x75 => Instruction { op: Opcode::ADC, mode: AddressingMode::ZeroPageX },
            0x6d => Instruction { op: Opcode::ADC, mode: AddressingMode::Absolute },
            0x7d => Instruction { op: Opcode::ADC, mode: AddressingMode::AbsoluteX },
            0x79 => Instruction { op: Opcode::ADC, mode: AddressingMode::AbsoluteY },
            0x61 => Instruction { op: Opcode::ADC, mode: AddressingMode::IndirectX },
            0x71 => Instruction { op: Opcode::ADC, mode: AddressingMode::IndirectY },

            0xe9 => Instruction { op: Opcode::SBC, mode: AddressingMode::Immediate },
            0xe5 => Instruction { op: Opcode::SBC, mode: AddressingMode::ZeroPage },
            0xf5 => Instruction { op: Opcode::SBC, mode: AddressingMode::ZeroPageX },
            0xed => Instruction { op: Opcode::SBC, mode: AddressingMode::Absolute },
            0xfd => Instruction { op: Opcode::SBC, mode: AddressingMode::AbsoluteX },
            0xf9 => Instruction { op: Opcode::SBC, mode: AddressingMode::AbsoluteY },
            0xe1 => Instruction { op: Opcode::SBC, mode: AddressingMode::IndirectX },
            0xf1 => Instruction { op: Opcode::SBC, mode: AddressingMode::IndirectY },

            0x29 => Instruction { op: Opcode::AND, mode: AddressingMode::Immediate },
            0x25 => Instruction { op: Opcode::AND, mode: AddressingMode::ZeroPage },
            0x35 => Instruction { op: Opcode::AND, mode: AddressingMode::ZeroPageX },
            0x2d => Instruction { op: Opcode::AND, mode: AddressingMode::Absolute },
            0x3d => Instruction { op: Opcode::AND, mode: AddressingMode::AbsoluteX },
            0x39 => Instruction { op: Opcode::AND, mode: AddressingMode::AbsoluteY },
            0x21 => Instruction { op: Opcode::AND, mode: AddressingMode::IndirectX },
            0x31 => Instruction { op: Opcode::AND, mode: AddressingMode::IndirectY },

            0x49 => Instruction { op: Opcode::EOR, mode: AddressingMode::Immediate },
            0x45 => Instruction { op: Opcode::EOR, mode: AddressingMode::ZeroPage },
            0x55 => Instruction { op: Opcode::EOR, mode: AddressingMode::ZeroPageX },
            0x4d => Instruction { op: Opcode::EOR, mode: AddressingMode::Absolute },
            0x5d => Instruction { op: Opcode::EOR, mode: AddressingMode::AbsoluteX },
            0x59 => Instruction { op: Opcode::EOR, mode: AddressingMode::AbsoluteY },
            0x41 => Instruction { op: Opcode::EOR, mode: AddressingMode::IndirectX },
            0x51 => Instruction { op: Opcode::EOR, mode: AddressingMode::IndirectY },

            0x09 => Instruction { op: Opcode::ORA, mode: AddressingMode::Immediate },
            0x05 => Instruction { op: Opcode::ORA, mode: AddressingMode::ZeroPage },
            0x15 => Instruction { op: Opcode::ORA, mode: AddressingMode::ZeroPageX },
            0x0d => Instruction { op: Opcode::ORA, mode: AddressingMode::Absolute },
            0x1d => Instruction { op: Opcode::ORA, mode: AddressingMode::AbsoluteX },
            0x19 => Instruction { op: Opcode::ORA, mode: AddressingMode::AbsoluteY },
            0x01 => Instruction { op: Opcode::ORA, mode: AddressingMode::IndirectX },
            0x11 => Instruction { op: Opcode::ORA, mode: AddressingMode::IndirectY },

            /* *************** shift/rotate op ***************  */
            0x0a => Instruction { op: Opcode::ASL, mode: AddressingMode::Accumulator },
            0x06 => Instruction { op: Opcode::ASL, mode: AddressingMode::ZeroPage },
            0x16 => Instruction { op: Opcode::ASL, mode: AddressingMode::ZeroPageX },
            0x0e => Instruction { op: Opcode::ASL, mode: AddressingMode::Absolute },
            0x1e => Instruction { op: Opcode::ASL, mode: AddressingMode::AbsoluteX },

            0x4a => Instruction { op: Opcode::LSR, mode: AddressingMode::Accumulator },
            0x46 => Instruction { op: Opcode::LSR, mode: AddressingMode::ZeroPage },
            0x56 => Instruction { op: Opcode::LSR, mode: AddressingMode::ZeroPageX },
            0x4e => Instruction { op: Opcode::LSR, mode: AddressingMode::Absolute },
            0x5e => Instruction { op: Opcode::LSR, mode: AddressingMode::AbsoluteX },

            0x2a => Instruction { op: Opcode::ROL, mode: AddressingMode::Accumulator },
            0x26 => Instruction { op: Opcode::ROL, mode: AddressingMode::ZeroPage },
            0x36 => Instruction { op: Opcode::ROL, mode: AddressingMode::ZeroPageX },
            0x2e => Instruction { op: Opcode::ROL, mode: AddressingMode::Absolute },
            0x3e => Instruction { op: Opcode::ROL, mode: AddressingMode::AbsoluteX },

            0x6a => Instruction { op: Opcode::ROR, mode: AddressingMode::Accumulator },
            0x66 => Instruction { op: Opcode::ROR, mode: AddressingMode::ZeroPage },
            0x76 => Instruction { op: Opcode::ROR, mode: AddressingMode::ZeroPageX },
            0x6e => Instruction { op: Opcode::ROR, mode: AddressingMode::Absolute },
            0x7e => Instruction { op: Opcode::ROR, mode: AddressingMode::AbsoluteX },

            /* *************** inc/dec op ***************  */
            0xe6 => Instruction { op: Opcode::INC, mode: AddressingMode::ZeroPage },
            0xf6 => Instruction { op: Opcode::INC, mode: AddressingMode::ZeroPageX },
            0xee => Instruction { op: Opcode::INC, mode: AddressingMode::Absolute },
            0xfe => Instruction { op: Opcode::INC, mode: AddressingMode::AbsoluteX },

            0xe8 => Instruction { op: Opcode::INX, mode: AddressingMode::Implied },
            0xc8 => Instruction { op: Opcode::INY, mode: AddressingMode::Implied },

            0xc6 => Instruction { op: Opcode::DEC, mode: AddressingMode::ZeroPage },
            0xd6 => Instruction { op: Opcode::DEC, mode: AddressingMode::ZeroPageX },
            0xce => Instruction { op: Opcode::DEC, mode: AddressingMode::Absolute },
            0xde => Instruction { op: Opcode::DEC, mode: AddressingMode::AbsoluteX },

            0xca => Instruction { op: Opcode::DEX, mode: AddressingMode::Implied },
            0x88 => Instruction { op: Opcode::DEY, mode: AddressingMode::Implied },

            /* *************** load/store op ***************  */
            0xa9 => Instruction { op: Opcode::LDA, mode: AddressingMode::Immediate },
            0xa5 => Instruction { op: Opcode::LDA, mode: AddressingMode::ZeroPage },
            0xb5 => Instruction { op: Opcode::LDA, mode: AddressingMode::ZeroPageX },
            0xad => Instruction { op: Opcode::LDA, mode: AddressingMode::Absolute },
            0xbd => Instruction { op: Opcode::LDA, mode: AddressingMode::AbsoluteX },
            0xb9 => Instruction { op: Opcode::LDA, mode: AddressingMode::AbsoluteY },
            0xa1 => Instruction { op: Opcode::LDA, mode: AddressingMode::IndirectX },
            0xb1 => Instruction { op: Opcode::LDA, mode: AddressingMode::IndirectY },

            0xa2 => Instruction { op: Opcode::LDX, mode: AddressingMode::Immediate },
            0xa6 => Instruction { op: Opcode::LDX, mode: AddressingMode::ZeroPage },
            0xb6 => Instruction { op: Opcode::LDX, mode: AddressingMode::ZeroPageY },
            0xae => Instruction { op: Opcode::LDX, mode: AddressingMode::Absolute },
            0xbe => Instruction { op: Opcode::LDX, mode: AddressingMode::AbsoluteY },

            0xa0 => Instruction { op: Opcode::LDY, mode: AddressingMode::Immediate },
            0xa4 => Instruction { op: Opcode::LDY, mode: AddressingMode::ZeroPage },
            0xb4 => Instruction { op: Opcode::LDY, mode: AddressingMode::ZeroPageX },
            0xac => Instruction { op: Opcode::LDY, mode: AddressingMode::Absolute },
            0xbc => Instruction { op: Opcode::LDY, mode: AddressingMode::AbsoluteX },

            0x85 => Instruction { op: Opcode::STA, mode: AddressingMode::ZeroPage },
            0x95 => Instruction { op: Opcode::STA, mode: AddressingMode::ZeroPageX },
            0x8d => Instruction { op: Opcode::STA, mode: AddressingMode::Absolute },
            0x9d => Instruction { op: Opcode::STA, mode: AddressingMode::AbsoluteX },
            0x99 => Instruction { op: Opcode::STA, mode: AddressingMode::AbsoluteY },
            0x81 => Instruction { op: Opcode::STA, mode: AddressingMode::IndirectX },
            0x91 => Instruction { op: Opcode::STA, mode: AddressingMode::IndirectY },

            0x86 => Instruction { op: Opcode::STX, mode: AddressingMode::ZeroPage },
            0x96 => Instruction { op: Opcode::STX, mode: AddressingMode::ZeroPageY },
            0x8e => Instruction { op: Opcode::STX, mode: AddressingMode::Absolute },

            0x84 => Instruction { op: Opcode::STY, mode: AddressingMode::ZeroPage },
            0x94 => Instruction { op: Opcode::STY, mode: AddressingMode::ZeroPageX },
            0x8c => Instruction { op: Opcode::STY, mode: AddressingMode::Absolute },

            /* *************** set/clear flag ***************  */
            0x38 => Instruction { op: Opcode::SEC, mode: AddressingMode::Implied },
            0xf8 => Instruction { op: Opcode::SED, mode: AddressingMode::Implied },
            0x78 => Instruction { op: Opcode::SEI, mode: AddressingMode::Implied },
            0x18 => Instruction { op: Opcode::CLC, mode: AddressingMode::Implied },
            0xd8 => Instruction { op: Opcode::CLD, mode: AddressingMode::Implied },
            0x58 => Instruction { op: Opcode::CLI, mode: AddressingMode::Implied },
            0xb8 => Instruction { op: Opcode::CLV, mode: AddressingMode::Implied },

            /* *************** compare ***************  */
            0xc9 => Instruction { op: Opcode::CMP, mode: AddressingMode::Immediate },
            0xc5 => Instruction { op: Opcode::CMP, mode: AddressingMode::ZeroPage },
            0xd5 => Instruction { op: Opcode::CMP, mode: AddressingMode::ZeroPageX },
            0xcd => Instruction { op: Opcode::CMP, mode: AddressingMode::Absolute },
            0xdd => Instruction { op: Opcode::CMP, mode: AddressingMode::AbsoluteX },
            0xd9 => Instruction { op: Opcode::CMP, mode: AddressingMode::AbsoluteY },
            0xc1 => Instruction { op: Opcode::CMP, mode: AddressingMode::IndirectX },
            0xd1 => Instruction { op: Opcode::CMP, mode: AddressingMode::IndirectY },

            0xe0 => Instruction { op: Opcode::CPX, mode: AddressingMode::Immediate },
            0xe4 => Instruction { op: Opcode::CPX, mode: AddressingMode::ZeroPage },
            0xec => Instruction { op: Opcode::CPX, mode: AddressingMode::Absolute },

            0xc0 => Instruction { op: Opcode::CPY, mode: AddressingMode::Immediate },
            0xc4 => Instruction { op: Opcode::CPY, mode: AddressingMode::ZeroPage },
            0xcc => Instruction { op: Opcode::CPY, mode: AddressingMode::Absolute },

            /* *************** jump/return ***************  */
            0x4c => Instruction { op: Opcode::JMP, mode: AddressingMode::Absolute },
            0x6c => Instruction { op: Opcode::JMP, mode: AddressingMode::Indirect },

            0x20 => Instruction { op: Opcode::JSR, mode: AddressingMode::Absolute },

            0x40 => Instruction { op: Opcode::RTI, mode: AddressingMode::Implied },
            0x60 => Instruction { op: Opcode::RTS, mode: AddressingMode::Implied },

            /* *************** branch ***************  */
            0x90 => Instruction { op: Opcode::BCC, mode: AddressingMode::Relative },
            0xb0 => Instruction { op: Opcode::BCS, mode: AddressingMode::Relative },
            0xf0 => Instruction { op: Opcode::BEQ, mode: AddressingMode::Relative },
            0xd0 => Instruction { op: Opcode::BNE, mode: AddressingMode::Relative },
            0x30 => Instruction { op: Opcode::BMI, mode: AddressingMode::Relative },
            0x10 => Instruction { op: Opcode::BPL, mode: AddressingMode::Relative },
            0x50 => Instruction { op: Opcode::BVC, mode: AddressingMode::Relative },
            0x70 => Instruction { op: Opcode::BVS, mode: AddressingMode::Relative },

            /* *************** push/pop ***************  */
            0x48 => Instruction { op: Opcode::PHA, mode: AddressingMode::Implied },
            0x08 => Instruction { op: Opcode::PHP, mode: AddressingMode::Implied },
            0x68 => Instruction { op: Opcode::PLA, mode: AddressingMode::Implied },
            0x28 => Instruction { op: Opcode::PLP, mode: AddressingMode::Implied },

            /* *************** transfer ***************  */
            0xaa => Instruction { op: Opcode::TAX, mode: AddressingMode::Implied },
            0xa8 => Instruction { op: Opcode::TAY, mode: AddressingMode::Implied },
            0xba => Instruction { op: Opcode::TSX, mode: AddressingMode::Implied },
            0x8a => Instruction { op: Opcode::TXA, mode: AddressingMode::Implied },
            0x9a => Instruction { op: Opcode::TXS, mode: AddressingMode::Implied },
            0x98 => Instruction { op: Opcode::TYA, mode: AddressingMode::Implied },

            /* *************** other ***************  */
            0x00 => Instruction { op: Opcode::BRK, mode: AddressingMode::Implied },

            0x24 => Instruction { op: Opcode::BIT, mode: AddressingMode::ZeroPage },
            0x2c => Instruction { op: Opcode::BIT, mode: AddressingMode::Absolute },

            0xea => Instruction { op: Opcode::NOP, mode: AddressingMode::Implied },

            /* *************** unofficial1 ***************  */
            0x4b => Instruction { op: Opcode::ALR, mode: AddressingMode::Immediate },
            0x0b => Instruction { op: Opcode::ANC, mode: AddressingMode::Immediate },
            0x6b => Instruction { op: Opcode::ARR, mode: AddressingMode::Immediate },
            0xcb => Instruction { op: Opcode::AXS, mode: AddressingMode::Immediate },

            0xa3 => Instruction { op: Opcode::LAX, mode: AddressingMode::IndirectX },
            0xa7 => Instruction { op: Opcode::LAX, mode: AddressingMode::ZeroPage },
            0xaf => Instruction { op: Opcode::LAX, mode: AddressingMode::Absolute },
            0xb3 => Instruction { op: Opcode::LAX, mode: AddressingMode::IndirectY },
            0xb7 => Instruction { op: Opcode::LAX, mode: AddressingMode::ZeroPageY },
            0xbf => Instruction { op: Opcode::LAX, mode: AddressingMode::AbsoluteY },

            0x83 => Instruction { op: Opcode::SAX, mode: AddressingMode::IndirectX },
            0x87 => Instruction { op: Opcode::SAX, mode: AddressingMode::ZeroPage },
            0x8f => Instruction { op: Opcode::SAX, mode: AddressingMode::Absolute },
            0x97 => Instruction { op: Opcode::SAX, mode: AddressingMode::ZeroPageY },

            0xc3 => Instruction { op: Opcode::DCP, mode: AddressingMode::IndirectX },
            0xc7 => Instruction { op: Opcode::DCP, mode: AddressingMode::ZeroPage },
            0xcf => Instruction { op: Opcode::DCP, mode: AddressingMode::Absolute },
            0xd3 => Instruction { op: Opcode::DCP, mode: AddressingMode::IndirectY },
            0xd7 => Instruction { op: Opcode::DCP, mode: AddressingMode::ZeroPageX },
            0xdb => Instruction { op: Opcode::DCP, mode: AddressingMode::AbsoluteY },
            0xdf => Instruction { op: Opcode::DCP, mode: AddressingMode::AbsoluteX },

            0xe3 => Instruction { op: Opcode::ISC, mode: AddressingMode::IndirectX },
            0xe7 => Instruction { op: Opcode::ISC, mode: AddressingMode::ZeroPage },
            0xef => Instruction { op: Opcode::ISC, mode: AddressingMode::Absolute },
            0xf3 => Instruction { op: Opcode::ISC, mode: AddressingMode::IndirectY },
            0xf7 => Instruction { op: Opcode::ISC, mode: AddressingMode::ZeroPageX },
            0xfb => Instruction { op: Opcode::ISC, mode: AddressingMode::AbsoluteY },
            0xff => Instruction { op: Opcode::ISC, mode: AddressingMode::AbsoluteX },

            0x23 => Instruction { op: Opcode::RLA, mode: AddressingMode::IndirectX },
            0x27 => Instruction { op: Opcode::RLA, mode: AddressingMode::ZeroPage },
            0x2f => Instruction { op: Opcode::RLA, mode: AddressingMode::Absolute },
            0x33 => Instruction { op: Opcode::RLA, mode: AddressingMode::IndirectY },
            0x37 => Instruction { op: Opcode::RLA, mode: AddressingMode::ZeroPageX },
            0x3b => Instruction { op: Opcode::RLA, mode: AddressingMode::AbsoluteY },
            0x3f => Instruction { op: Opcode::RLA, mode: AddressingMode::AbsoluteX },

            0x63 => Instruction { op: Opcode::RRA, mode: AddressingMode::IndirectX },
            0x67 => Instruction { op: Opcode::RRA, mode: AddressingMode::ZeroPage },
            0x6f => Instruction { op: Opcode::RRA, mode: AddressingMode::Absolute },
            0x73 => Instruction { op: Opcode::RRA, mode: AddressingMode::IndirectY },
            0x77 => Instruction { op: Opcode::RRA, mode: AddressingMode::ZeroPageX },
            0x7b => Instruction { op: Opcode::RRA, mode: AddressingMode::AbsoluteY },
            0x7f => Instruction { op: Opcode::RRA, mode: AddressingMode::AbsoluteX },

            0x03 => Instruction { op: Opcode::SLO, mode: AddressingMode::IndirectX },
            0x07 => Instruction { op: Opcode::SLO, mode: AddressingMode::ZeroPage },
            0x0f => Instruction { op: Opcode::SLO, mode: AddressingMode::Absolute },
            0x13 => Instruction { op: Opcode::SLO, mode: AddressingMode::IndirectY },
            0x17 => Instruction { op: Opcode::SLO, mode: AddressingMode::ZeroPageX },
            0x1b => Instruction { op: Opcode::SLO, mode: AddressingMode::AbsoluteY },
            0x1f => Instruction { op: Opcode::SLO, mode: AddressingMode::AbsoluteX },

            0x43 => Instruction { op: Opcode::SRE, mode: AddressingMode::IndirectX },
            0x47 => Instruction { op: Opcode::SRE, mode: AddressingMode::ZeroPage },
            0x4f => Instruction { op: Opcode::SRE, mode: AddressingMode::Absolute },
            0x53 => Instruction { op: Opcode::SRE, mode: AddressingMode::IndirectY },
            0x57 => Instruction { op: Opcode::SRE, mode: AddressingMode::ZeroPageX },
            0x5b => Instruction { op: Opcode::SRE, mode: AddressingMode::AbsoluteY },
            0x5f => Instruction { op: Opcode::SRE, mode: AddressingMode::AbsoluteX },

            0x80 => Instruction { op: Opcode::SKB, mode: AddressingMode::Immediate },
            0x82 => Instruction { op: Opcode::SKB, mode: AddressingMode::Immediate },
            0x89 => Instruction { op: Opcode::SKB, mode: AddressingMode::Immediate },
            0xc2 => Instruction { op: Opcode::SKB, mode: AddressingMode::Immediate },
            0xe2 => Instruction { op: Opcode::SKB, mode: AddressingMode::Immediate },

            0x0c => Instruction { op: Opcode::IGN, mode: AddressingMode::Absolute },

            0x1c => Instruction { op: Opcode::IGN, mode: AddressingMode::AbsoluteX },
            0x3c => Instruction { op: Opcode::IGN, mode: AddressingMode::AbsoluteX },
            0x5c => Instruction { op: Opcode::IGN, mode: AddressingMode::AbsoluteX },
            0x7c => Instruction { op: Opcode::IGN, mode: AddressingMode::AbsoluteX },
            0xdc => Instruction { op: Opcode::IGN, mode: AddressingMode::AbsoluteX },
            0xfc => Instruction { op: Opcode::IGN, mode: AddressingMode::AbsoluteX },

            0x04 => Instruction { op: Opcode::IGN, mode: AddressingMode::ZeroPage },
            0x44 => Instruction { op: Opcode::IGN, mode: AddressingMode::ZeroPage },
            0x64 => Instruction { op: Opcode::IGN, mode: AddressingMode::ZeroPage },

            0x14 => Instruction { op: Opcode::IGN, mode: AddressingMode::ZeroPageX },
            0x34 => Instruction { op: Opcode::IGN, mode: AddressingMode::ZeroPageX },
            0x54 => Instruction { op: Opcode::IGN, mode: AddressingMode::ZeroPageX },
            0x74 => Instruction { op: Opcode::IGN, mode: AddressingMode::ZeroPageX },
            0xd4 => Instruction { op: Opcode::IGN, mode: AddressingMode::ZeroPageX },
            0xf4 => Instruction { op: Opcode::IGN, mode: AddressingMode::ZeroPageX },

            /* *************** unofficial2(既存の命令) ***************  */
            0xeb => Instruction { op: Opcode::SBC, mode: AddressingMode::Immediate },

            0x1a => Instruction { op: Opcode::NOP, mode: AddressingMode::Implied },
            0x3a => Instruction { op: Opcode::NOP, mode: AddressingMode::Implied },
            0x5a => Instruction { op: Opcode::NOP, mode: AddressingMode::Implied },
            0x7a => Instruction { op: Opcode::NOP, mode: AddressingMode::Implied },
            0xda => Instruction { op: Opcode::NOP, mode: AddressingMode::Implied },
            0xfa => Instruction { op: Opcode::NOP, mode: AddressingMode::Implied },

            _ => panic!("Invalid inst_code:{:08x}", inst_code),
        }
    }
}

impl Cpu {
    /// Fetch 1 byte from PC and after fetching, advance the PC by one
    fn fetch_u8(&mut self, system: &mut System) -> u8 {
        let data = system.read_u8(self.pc, false);
        self.pc = self.pc + 1;
        data
    }

    /// Fetch 2 bytes from PC and increment PC by two
    fn fetch_u16(&mut self, system: &mut System) -> u16 {
        let lower = self.fetch_u8(system);
        let upper = self.fetch_u8(system);
        let data = u16::from(lower) | (u16::from(upper) << 8);
        data
    }

    /// Fetch the operand.
    /// Depending on the Addressing mode, the PC also advances.
    /// When implementing, Cpu::fetch when reading the operand immediately after the instruction, otherwise System::read
    fn fetch_operand(&mut self, system: &mut System, mode: AddressingMode) -> Operand {
        match mode {
            AddressingMode::Implied => Operand { data: 0, cyc: 0 },
            AddressingMode::Accumulator => Operand { data: 0, cyc: 1 },
            AddressingMode::Immediate => Operand { data: u16::from(self.fetch_u8(system)), cyc: 1 },
            AddressingMode::Absolute => Operand { data: self.fetch_u16(system), cyc: 3 },
            AddressingMode::ZeroPage => Operand { data: u16::from(self.fetch_u8(system)), cyc: 2 },
            AddressingMode::ZeroPageX => {
                Operand { data: u16::from(self.fetch_u8(system).wrapping_add(self.x)), cyc: 3 }
            }
            AddressingMode::ZeroPageY => {
                Operand { data: u16::from(self.fetch_u8(system).wrapping_add(self.y)), cyc: 3 }
            }
            AddressingMode::AbsoluteX => {
                let data = self.fetch_u16(system).wrapping_add(u16::from(self.x));
                let additional_cyc =
                    if (data & 0xff00u16) != (data.wrapping_add(u16::from(self.x)) & 0xff00u16) {
                        1
                    } else {
                        0
                    };
                Operand { data: data, cyc: 3 + additional_cyc }
            }
            AddressingMode::AbsoluteY => {
                let data = self.fetch_u16(system).wrapping_add(u16::from(self.y));
                let additional_cyc =
                    if (data & 0xff00u16) != (data.wrapping_add(u16::from(self.y)) & 0xff00u16) {
                        1
                    } else {
                        0
                    };
                Operand { data: data, cyc: 3 + additional_cyc }
            }
            AddressingMode::Relative => {
                let src_addr = self.fetch_u8(system);
                let signed_data = ((src_addr as i8) as i32) + (self.pc as i32); // 符号拡張して計算する
                debug_assert!(signed_data >= 0);
                debug_assert!(signed_data < 0x10000);

                let data = signed_data as u16;
                let additional_cyc = if (data & 0xff00u16) != (self.pc & 0xff00u16) {
                    1
                } else {
                    0
                };

                Operand { data: data, cyc: 1 + additional_cyc }
            }
            AddressingMode::Indirect => {
                let src_addr_lower = self.fetch_u8(system);
                let src_addr_upper = self.fetch_u8(system);

                let dst_addr_lower = u16::from(src_addr_lower) | (u16::from(src_addr_upper) << 8); // operandそのまま
                let dst_addr_upper =
                    u16::from(src_addr_lower.wrapping_add(1)) | (u16::from(src_addr_upper) << 8); // operandのlowerに+1したもの

                let dst_data_lower = u16::from(system.read_u8(dst_addr_lower, false));
                let dst_data_upper = u16::from(system.read_u8(dst_addr_upper, false));

                let data = dst_data_lower | (dst_data_upper << 8);

                Operand { data: data, cyc: 5 }
            }
            AddressingMode::IndirectX => {
                let src_addr = self.fetch_u8(system);
                let dst_addr = src_addr.wrapping_add(self.x);

                let data_lower = u16::from(system.read_u8(u16::from(dst_addr), false));
                let data_upper =
                    u16::from(system.read_u8(u16::from(dst_addr.wrapping_add(1)), false));

                let data = data_lower | (data_upper << 8);
                Operand { data: data, cyc: 5 }
            }
            AddressingMode::IndirectY => {
                let src_addr = self.fetch_u8(system);

                let data_lower = u16::from(system.read_u8(u16::from(src_addr), false));
                let data_upper =
                    u16::from(system.read_u8(u16::from(src_addr.wrapping_add(1)), false));

                let base_data = data_lower | (data_upper << 8);
                let data = base_data.wrapping_add(u16::from(self.y));
                let additional_cyc = if (base_data & 0xff00u16) != (data & 0xff00u16) {
                    1
                } else {
                    0
                };

                Operand { data: data, cyc: 4 + additional_cyc }
            }
        }
    }

    /// If you want to pull not only the address but also the data in one shot
    /// returns (Operand { data: subtracted immediate value or address, number of clocks), data)
    fn fetch_args(&mut self, system: &mut System, mode: AddressingMode) -> (Operand, u8) {
        match mode {
            // (Dummy data should not be used)
            AddressingMode::Implied => (self.fetch_operand(system, mode), 0),
            // Use the value of the a register
            AddressingMode::Accumulator => (self.fetch_operand(system, mode), self.a),
            // Immediate value uses 1 byte of data immediately after opcode as it is
            AddressingMode::Immediate => {
                let Operand { data, cyc } = self.fetch_operand(system, mode);
                debug_assert!(data < 0x100u16);
                (Operand { data, cyc }, data as u8)
            }
            // Others pull back the data from the returned address. May not be used
            _ => {
                let Operand { data: addr, cyc } = self.fetch_operand(system, mode);
                let data = system.read_u8(addr, false);
                (Operand { data: addr, cyc }, data)
            }
        }
    }

    /// Execute the instruction
    /// returns: number of cycles
    /// https://www.nesdev.org/obelisk-6502-guide/reference.html
    pub fn step(&mut self, system: &mut System) -> u8 {
        // Address where the instruction is placed
        let inst_pc = self.pc;
        let inst_code = self.fetch_u8(system);

        let Instruction { op: opcode, mode }  = Instruction::from(inst_code);

        match opcode {
            /* *************** binary op ***************  */
            // The result is stored in the a register, so the address of the operand is not used
            Opcode::ADC => {
                let (Operand { data: _, cyc }, arg) = self.fetch_args(system, mode);

                let tmp = u16::from(self.a)
                    + u16::from(arg)
                    + (if self.read_carry_flag() { 1 } else { 0 });
                let result = (tmp & 0xff) as u8;

                let is_carry = tmp > 0x00ffu16;
                let is_zero = result == 0;
                let is_negative = (result & 0x80) == 0x80;
                let is_overflow = ((self.a ^ result) & (arg ^ result) & 0x80) == 0x80;

                self.write_carry_flag(is_carry);
                self.write_zero_flag(is_zero);
                self.write_negative_flag(is_negative);
                self.write_overflow_flag(is_overflow);
                self.a = result;
                1 + cyc
            }
            Opcode::SBC => {
                let (Operand { data: _, cyc }, arg) = self.fetch_args(system, mode);

                let (data1, is_carry1) = self.a.overflowing_sub(arg);
                let (result, is_carry2) =
                    data1.overflowing_sub(if self.read_carry_flag() { 0 } else { 1 });

                let is_carry = !(is_carry1 || is_carry2); // アンダーフローが発生したら0
                let is_zero = result == 0;
                let is_negative = (result & 0x80) == 0x80;
                let is_overflow =
                    (((self.a ^ arg) & 0x80) == 0x80) && (((self.a ^ result) & 0x80) == 0x80);

                self.write_carry_flag(is_carry);
                self.write_zero_flag(is_zero);
                self.write_negative_flag(is_negative);
                self.write_overflow_flag(is_overflow);
                self.a = result;
                1 + cyc
            }
            Opcode::AND => {
                let (Operand { data: _, cyc }, arg) = self.fetch_args(system, mode);

                let result = self.a & arg;
                let is_zero = result == 0;
                let is_negative = (result & 0x80) == 0x80;

                self.write_zero_flag(is_zero);
                self.write_negative_flag(is_negative);
                self.a = result;
                1 + cyc
            }
            Opcode::EOR => {
                let (Operand { data: _, cyc }, arg) = self.fetch_args(system, mode);

                let result = self.a ^ arg;

                let is_zero = result == 0;
                let is_negative = (result & 0x80) == 0x80;

                self.write_zero_flag(is_zero);
                self.write_negative_flag(is_negative);
                self.a = result;
                1 + cyc
            }
            Opcode::ORA => {
                let (Operand { data: _, cyc }, arg) = self.fetch_args(system, mode);

                let result = self.a | arg;

                let is_zero = result == 0;
                let is_negative = (result & 0x80) == 0x80;

                self.write_zero_flag(is_zero);
                self.write_negative_flag(is_negative);
                self.a = result;
                1 + cyc
            }
            /* *************** shift/rotate op ***************  */
            // aレジスタを操作する場合があるので注意
            Opcode::ASL => {
                let (Operand { data: addr, cyc }, arg) = self.fetch_args(system, mode);

                let result = arg.wrapping_shl(1);

                let is_carry = (arg & 0x80) == 0x80; // shift前データでわかるよね
                let is_zero = result == 0;
                let is_negative = (result & 0x80) == 0x80;

                self.write_carry_flag(is_carry);
                self.write_zero_flag(is_zero);
                self.write_negative_flag(is_negative);

                if mode == AddressingMode::Accumulator {
                    self.a = result;
                    1 + cyc
                } else {
                    // 計算結果を元いたアドレスに書き戻す
                    system.write_u8(addr, result, false);
                    3 + cyc
                }
            }
            Opcode::LSR => {
                let (Operand { data: addr, cyc }, arg) = self.fetch_args(system, mode);

                let result = arg.wrapping_shr(1);

                let is_carry = (arg & 0x01) == 0x01;
                let is_zero = result == 0;
                let is_negative = (result & 0x80) == 0x80;

                self.write_carry_flag(is_carry);
                self.write_zero_flag(is_zero);
                self.write_negative_flag(is_negative);

                if mode == AddressingMode::Accumulator {
                    self.a = result;
                    1 + cyc
                } else {
                    // 計算結果を元いたアドレスに書き戻す
                    system.write_u8(addr, result, false);
                    3 + cyc
                }
            }
            Opcode::ROL => {
                let (Operand { data: addr, cyc }, arg) = self.fetch_args(system, mode);

                let result =
                    arg.wrapping_shl(1) | (if self.read_carry_flag() { 0x01 } else { 0x00 });

                let is_carry = (arg & 0x80) == 0x80;
                let is_zero = result == 0;
                let is_negative = (result & 0x80) == 0x80;

                self.write_carry_flag(is_carry);
                self.write_zero_flag(is_zero);
                self.write_negative_flag(is_negative);

                if mode == AddressingMode::Accumulator {
                    self.a = result;
                    1 + cyc
                } else {
                    // 計算結果を元いたアドレスに書き戻す
                    system.write_u8(addr, result, false);
                    3 + cyc
                }
            }
            Opcode::ROR => {
                let (Operand { data: addr, cyc }, arg) = self.fetch_args(system, mode);

                let result =
                    arg.wrapping_shr(1) | (if self.read_carry_flag() { 0x80 } else { 0x00 });

                let is_carry = (arg & 0x01) == 0x01;
                let is_zero = result == 0;
                let is_negative = (result & 0x80) == 0x80;

                self.write_carry_flag(is_carry);
                self.write_zero_flag(is_zero);
                self.write_negative_flag(is_negative);

                if mode == AddressingMode::Accumulator {
                    self.a = result;
                    1 + cyc
                } else {
                    // 計算結果を元いたアドレスに書き戻す
                    system.write_u8(addr, result, false);
                    3 + cyc
                }
            }
            /* *************** inc/dec op ***************  */
            // accumulatorは使わない, x,yレジスタを使うバージョンはImplied
            Opcode::INC => {
                let (Operand { data: addr, cyc }, arg) = self.fetch_args(system, mode);

                let result = arg.wrapping_add(1);

                let is_zero = result == 0;
                let is_negative = (result & 0x80) == 0x80;

                self.write_zero_flag(is_zero);
                self.write_negative_flag(is_negative);
                system.write_u8(addr, result, false);
                3 + cyc
            }
            Opcode::INX => {
                let result = self.x.wrapping_add(1);

                let is_zero = result == 0;
                let is_negative = (result & 0x80) == 0x80;

                self.write_zero_flag(is_zero);
                self.write_negative_flag(is_negative);
                self.x = result;
                2
            }
            Opcode::INY => {
                let result = self.y.wrapping_add(1);

                let is_zero = result == 0;
                let is_negative = (result & 0x80) == 0x80;

                self.write_zero_flag(is_zero);
                self.write_negative_flag(is_negative);
                self.y = result;
                2
            }
            Opcode::DEC => {
                let (Operand { data: addr, cyc }, arg) = self.fetch_args(system, mode);

                let result = arg.wrapping_sub(1);

                let is_zero = result == 0;
                let is_negative = (result & 0x80) == 0x80;

                self.write_zero_flag(is_zero);
                self.write_negative_flag(is_negative);
                system.write_u8(addr, result, false);
                3 + cyc
            }
            Opcode::DEX => {
                let result = self.x.wrapping_sub(1);

                let is_zero = result == 0;
                let is_negative = (result & 0x80) == 0x80;

                self.write_zero_flag(is_zero);
                self.write_negative_flag(is_negative);
                self.x = result;
                2
            }
            Opcode::DEY => {
                let result = self.y.wrapping_sub(1);

                let is_zero = result == 0;
                let is_negative = (result & 0x80) == 0x80;

                self.write_zero_flag(is_zero);
                self.write_negative_flag(is_negative);
                self.y = result;
                2
            }

            /* *************** load/store op ***************  */
            // Accumualtorはなし
            // store系はargはいらない, Immediateなし
            Opcode::LDA => {
                let (Operand { data: _, cyc }, arg) = self.fetch_args(system, mode);

                let is_zero = arg == 0;
                let is_negative = (arg & 0x80) == 0x80;

                self.write_zero_flag(is_zero);
                self.write_negative_flag(is_negative);
                self.a = arg;
                1 + cyc
            }
            Opcode::LDX => {
                let (Operand { data: _, cyc }, arg) = self.fetch_args(system, mode);

                let is_zero = arg == 0;
                let is_negative = (arg & 0x80) == 0x80;

                self.write_zero_flag(is_zero);
                self.write_negative_flag(is_negative);
                self.x = arg;
                1 + cyc
            }
            Opcode::LDY => {
                let (Operand { data: _, cyc }, arg) = self.fetch_args(system, mode);

                let is_zero = arg == 0;
                let is_negative = (arg & 0x80) == 0x80;

                self.write_zero_flag(is_zero);
                self.write_negative_flag(is_negative);
                self.y = arg;
                1 + cyc
            }
            Opcode::STA => {
                let Operand { data: addr, cyc } = self.fetch_operand(system, mode);

                system.write_u8(addr, self.a, false);
                1 + cyc
            }
            Opcode::STX => {
                let Operand { data: addr, cyc } = self.fetch_operand(system, mode);

                system.write_u8(addr, self.x, false);
                1 + cyc
            }
            Opcode::STY => {
                let Operand { data: addr, cyc } = self.fetch_operand(system, mode);

                system.write_u8(addr, self.y, false);
                1 + cyc
            }

            /* *************** set/clear flag ***************  */
            // すべてImplied
            Opcode::SEC => {
                self.write_carry_flag(true);
                2
            }
            Opcode::SED => {
                self.write_decimal_flag(true);
                2
            }
            Opcode::SEI => {
                self.write_interrupt_flag(true);
                2
            }
            Opcode::CLC => {
                self.write_carry_flag(false);
                2
            }
            Opcode::CLD => {
                self.write_decimal_flag(false);
                2
            }
            Opcode::CLI => {
                self.write_interrupt_flag(false);
                2
            }
            Opcode::CLV => {
                self.write_overflow_flag(false);
                2
            }

            /* *************** compare ***************  */
            // Accumulatorなし
            Opcode::CMP => {
                let (Operand { data: _, cyc }, arg) = self.fetch_args(system, mode);

                let (result, _) = self.a.overflowing_sub(arg);

                let is_carry = self.a >= arg;
                let is_zero = result == 0;
                let is_negative = (result & 0x80) == 0x80;

                self.write_carry_flag(is_carry);
                self.write_zero_flag(is_zero);
                self.write_negative_flag(is_negative);
                1 + cyc
            }
            Opcode::CPX => {
                let (Operand { data: _, cyc }, arg) = self.fetch_args(system, mode);

                let (result, _) = self.x.overflowing_sub(arg);

                let is_carry = self.x >= arg;
                let is_zero = result == 0;
                let is_negative = (result & 0x80) == 0x80;

                self.write_carry_flag(is_carry);
                self.write_zero_flag(is_zero);
                self.write_negative_flag(is_negative);
                1 + cyc
            }
            Opcode::CPY => {
                let (Operand { data: _, cyc }, arg) = self.fetch_args(system, mode);

                let (result, _) = self.y.overflowing_sub(arg);

                let is_carry = self.y >= arg;
                let is_zero = result == 0;
                let is_negative = (result & 0x80) == 0x80;

                self.write_carry_flag(is_carry);
                self.write_zero_flag(is_zero);
                self.write_negative_flag(is_negative);
                1 + cyc
            }

            /* *************** jump/return ***************  */
            // JMP: Absolute or Indirect, JSR: Absolute, RTI,RTS: Implied
            Opcode::JMP => {
                let Operand { data: addr, cyc } = self.fetch_operand(system, mode);
                self.pc = addr;
                cyc
            }
            Opcode::JSR => {
                let Operand { data: addr, cyc: _ } = self.fetch_operand(system, mode);
                // opcodeがあったアドレスを取得する(opcode, operand fetchで3進んでる)
                let opcode_addr = inst_pc;

                // pushはUpper, Lower
                let ret_addr = opcode_addr + 2;
                self.stack_push(system, (ret_addr >> 8) as u8);
                self.stack_push(system, (ret_addr & 0xff) as u8);
                self.pc = addr;
                6
            }
            Opcode::RTI => {
                self.p = self.stack_pop(system);
                let pc_lower = self.stack_pop(system);
                let pc_upper = self.stack_pop(system);
                self.pc = ((pc_upper as u16) << 8) | (pc_lower as u16);
                6
            }
            Opcode::RTS => {
                let pc_lower = self.stack_pop(system);
                let pc_upper = self.stack_pop(system);
                self.pc = (((pc_upper as u16) << 8) | (pc_lower as u16)) + 1;
                6
            }

            /* *************** branch ***************  */
            // Relativeのみ
            Opcode::BCC => {
                debug_assert!(mode == AddressingMode::Relative);
                let Operand { data: addr, cyc } = self.fetch_operand(system, mode);
                if !self.read_carry_flag() {
                    self.pc = addr;
                    1 + cyc + 1
                } else {
                    1 + cyc
                }
            }
            Opcode::BCS => {
                debug_assert!(mode == AddressingMode::Relative);
                let Operand { data: addr, cyc } = self.fetch_operand(system, mode);
                if self.read_carry_flag() {
                    self.pc = addr;
                    1 + cyc + 1
                } else {
                    1 + cyc
                }
            }
            Opcode::BEQ => {
                debug_assert!(mode == AddressingMode::Relative);
                let Operand { data: addr, cyc } = self.fetch_operand(system, mode);
                if self.read_zero_flag() {
                    self.pc = addr;
                    1 + cyc + 1
                } else {
                    1 + cyc
                }
            }
            Opcode::BNE => {
                debug_assert!(mode == AddressingMode::Relative);
                let Operand { data: addr, cyc } = self.fetch_operand(system, mode);
                if !self.read_zero_flag() {
                    self.pc = addr;
                    1 + cyc + 1
                } else {
                    1 + cyc
                }
            }
            Opcode::BMI => {
                debug_assert!(mode == AddressingMode::Relative);
                let Operand { data: addr, cyc } = self.fetch_operand(system, mode);
                if self.read_negative_flag() {
                    self.pc = addr;
                    1 + cyc + 1
                } else {
                    1 + cyc
                }
            }
            Opcode::BPL => {
                debug_assert!(mode == AddressingMode::Relative);
                let Operand { data: addr, cyc } = self.fetch_operand(system, mode);
                if !self.read_negative_flag() {
                    self.pc = addr;
                    1 + cyc + 1
                } else {
                    1 + cyc
                }
            }
            Opcode::BVC => {
                debug_assert!(mode == AddressingMode::Relative);
                let Operand { data: addr, cyc } = self.fetch_operand(system, mode);
                if !self.read_overflow_flag() {
                    self.pc = addr;
                    1 + cyc + 1
                } else {
                    1 + cyc
                }
            }
            Opcode::BVS => {
                debug_assert!(mode == AddressingMode::Relative);
                let Operand { data: addr, cyc } = self.fetch_operand(system, mode);
                if self.read_overflow_flag() {
                    self.pc = addr;
                    1 + cyc + 1
                } else {
                    1 + cyc
                }
            }

            /* *************** push/pop ***************  */
            // Impliedのみ
            Opcode::PHA => {
                self.stack_push(system, self.a);
                3
            }
            Opcode::PHP => {
                self.stack_push(system, self.p);
                3
            }
            Opcode::PLA => {
                let result = self.stack_pop(system);

                let is_zero = result == 0;
                let is_negative = (result & 0x80) == 0x80;

                self.write_zero_flag(is_zero);
                self.write_negative_flag(is_negative);
                self.a = result;
                4
            }
            Opcode::PLP => {
                self.p = self.stack_pop(system);
                4
            }

            /* *************** transfer ***************  */
            // Impliedのみ
            Opcode::TAX => {
                let is_zero = self.a == 0;
                let is_negative = (self.a & 0x80) == 0x80;

                self.write_zero_flag(is_zero);
                self.write_negative_flag(is_negative);
                self.x = self.a;
                2
            }
            Opcode::TAY => {
                let is_zero = self.a == 0;
                let is_negative = (self.a & 0x80) == 0x80;

                self.write_zero_flag(is_zero);
                self.write_negative_flag(is_negative);
                self.y = self.a;
                2
            }
            Opcode::TSX => {
                let result = (self.sp & 0xff) as u8;

                let is_zero = result == 0;
                let is_negative = (result & 0x80) == 0x80;

                self.write_zero_flag(is_zero);
                self.write_negative_flag(is_negative);
                self.x = result;
                2
            }
            Opcode::TXA => {
                let is_zero = self.x == 0;
                let is_negative = (self.x & 0x80) == 0x80;

                self.write_zero_flag(is_zero);
                self.write_negative_flag(is_negative);
                self.a = self.x;
                2
            }
            Opcode::TXS => {
                // spの上位バイトは0x01固定
                // txsはstatus書き換えなし
                self.sp = (self.x as u16) | 0x0100u16;
                2
            }
            Opcode::TYA => {
                let is_zero = self.y == 0;
                let is_negative = (self.y & 0x80) == 0x80;

                self.write_zero_flag(is_zero);
                self.write_negative_flag(is_negative);
                self.a = self.y;
                2
            }

            /* *************** other ***************  */
            Opcode::BRK => {
                // Implied
                self.write_break_flag(true);
                self.interrupt(system, Interrupt::BRK);
                7
            }
            Opcode::BIT => {
                // ZeroPage or Absolute
                // 非破壊読み出しが必要, fetch_args使わずに自分で読むか...
                let Operand { data: addr, cyc } = self.fetch_operand(system, mode);
                let arg = system.read_u8(addr, true); // 非破壊読み出し

                let is_negative = (arg & 0x80) == 0x80;
                let is_overflow = (arg & 0x40) == 0x40;
                let is_zero = (self.a & arg) == 0x00;

                self.write_negative_flag(is_negative);
                self.write_zero_flag(is_zero);
                self.write_overflow_flag(is_overflow);
                2 + cyc
            }
            Opcode::NOP => {
                //なにもしない、Implied
                2
            }
            /* *************** unofficial1 ***************  */
            Opcode::ALR => {
                // Immediateのみ、(A & #Imm) >> 1
                debug_assert!(mode == AddressingMode::Immediate);
                let (Operand { data: _, cyc }, arg) = self.fetch_args(system, mode);

                let src = self.a & arg;
                let result = src.wrapping_shr(1);

                let is_carry = (src & 0x01) == 0x01;
                let is_zero = result == 0;
                let is_negative = (result & 0x80) == 0x80;

                self.write_carry_flag(is_carry);
                self.write_zero_flag(is_zero);
                self.write_negative_flag(is_negative);

                self.a = result;
                1 + cyc
            }
            Opcode::ANC => {
                // Immediateのみ、A=A & #IMM, Carryは前回状態のNegativeをコピー
                debug_assert!(mode == AddressingMode::Immediate);
                let (Operand { data: _, cyc }, arg) = self.fetch_args(system, mode);

                let result = self.a & arg;
                let is_zero = result == 0;
                let is_negative = (result & 0x80) == 0x80;
                let is_carry = self.read_negative_flag();

                self.write_zero_flag(is_zero);
                self.write_negative_flag(is_negative);
                self.write_carry_flag(is_carry);
                self.a = result;
                1 + cyc
            }
            Opcode::ARR => {
                // Immediateのみ、Carry=bit6, V=bit6 xor bit5
                debug_assert!(mode == AddressingMode::Immediate);
                let (Operand { data: _, cyc }, arg) = self.fetch_args(system, mode);

                let src = self.a & arg;
                let result =
                    src.wrapping_shr(1) | (if self.read_carry_flag() { 0x80 } else { 0x00 });

                let is_zero = result == 0;
                let is_negative = (result & 0x80) == 0x80;
                let is_carry = (result & 0x40) == 0x40;
                let is_overflow = ((result & 0x40) ^ ((result & 0x20) << 1)) == 0x40;

                self.write_zero_flag(is_zero);
                self.write_negative_flag(is_negative);
                self.write_carry_flag(is_carry);
                self.write_overflow_flag(is_overflow);

                self.a = result;
                1 + cyc
            }
            Opcode::AXS => {
                // Immediateのみ、X = (A & X) - #IMM, NZCを更新
                // without borrowとのことなので、減算時cフラグも無視
                debug_assert!(mode == AddressingMode::Immediate);
                let (Operand { data: _, cyc }, arg) = self.fetch_args(system, mode);

                let src = self.a & arg;

                let (result, is_carry) = self.a.overflowing_sub(src);

                let is_zero = result == 0;
                let is_negative = (result & 0x80) == 0x80;

                self.write_carry_flag(is_carry);
                self.write_zero_flag(is_zero);
                self.write_negative_flag(is_negative);
                self.x = result;
                1 + cyc
            }
            Opcode::LAX => {
                // A = X = argsっぽい
                let (Operand { data: _, cyc }, arg) = self.fetch_args(system, mode);

                let is_zero = arg == 0;
                let is_negative = (arg & 0x80) == 0x80;

                self.write_zero_flag(is_zero);
                self.write_negative_flag(is_negative);
                self.a = arg;
                self.x = arg;
                1 + cyc
            }
            Opcode::SAX => {
                // memory = A & X, flag操作はなし
                let (Operand { data: addr, cyc }, _arg) = self.fetch_args(system, mode);

                let result = self.a & self.x;

                system.write_u8(addr, result, false);
                1 + cyc
            }
            Opcode::DCP => {
                // DEC->CMPっぽい
                let (Operand { data: addr, cyc }, arg) = self.fetch_args(system, mode);

                // DEC
                let dec_result = arg.wrapping_sub(1);
                system.write_u8(addr, dec_result, false);

                // CMP
                let result = self.a.wrapping_sub(dec_result);

                let is_carry = self.a >= dec_result;
                let is_zero = result == 0;
                let is_negative = (result & 0x80) == 0x80;

                self.write_carry_flag(is_carry);
                self.write_zero_flag(is_zero);
                self.write_negative_flag(is_negative);
                3 + cyc
            }
            Opcode::ISC => {
                // INC->SBC
                let (Operand { data: addr, cyc }, arg) = self.fetch_args(system, mode);

                // INC
                let inc_result = arg.wrapping_add(1);
                system.write_u8(addr, inc_result, false);

                // SBC
                let (data1, is_carry1) = self.a.overflowing_sub(inc_result);
                let (result, is_carry2) =
                    data1.overflowing_sub(if self.read_carry_flag() { 0 } else { 1 });

                let is_carry = !(is_carry1 || is_carry2); // アンダーフローが発生したら0
                let is_zero = result == 0;
                let is_negative = (result & 0x80) == 0x80;
                let is_overflow = (((self.a ^ inc_result) & 0x80) == 0x80)
                    && (((self.a ^ result) & 0x80) == 0x80);

                self.write_carry_flag(is_carry);
                self.write_zero_flag(is_zero);
                self.write_negative_flag(is_negative);
                self.write_overflow_flag(is_overflow);
                self.a = result;
                1 + cyc
            }
            Opcode::RLA => {
                // ROL -> AND
                let (Operand { data: addr, cyc }, arg) = self.fetch_args(system, mode);

                // ROL
                let result_rol =
                    arg.wrapping_shl(1) | (if self.read_carry_flag() { 0x01 } else { 0x00 });

                let is_carry = (arg & 0x80) == 0x80;
                self.write_carry_flag(is_carry);

                system.write_u8(addr, result_rol, false);

                // AND
                let result_and = self.a & result_rol;

                let is_zero = result_and == 0;
                let is_negative = (result_and & 0x80) == 0x80;

                self.write_zero_flag(is_zero);
                self.write_negative_flag(is_negative);

                self.a = result_and;

                3 + cyc
            }
            Opcode::RRA => {
                // ROR -> ADC
                let (Operand { data: addr, cyc }, arg) = self.fetch_args(system, mode);

                // ROR
                let result_ror =
                    arg.wrapping_shr(1) | (if self.read_carry_flag() { 0x80 } else { 0x00 });

                let is_carry_ror = (arg & 0x01) == 0x01;
                self.write_carry_flag(is_carry_ror);

                system.write_u8(addr, result_ror, false);

                // ADC
                let tmp = u16::from(self.a)
                    + u16::from(result_ror)
                    + (if self.read_carry_flag() { 1 } else { 0 });
                let result_adc = (tmp & 0xff) as u8;

                let is_carry = tmp > 0x00ffu16;
                let is_zero = result_adc == 0;
                let is_negative = (result_adc & 0x80) == 0x80;
                let is_overflow =
                    ((self.a ^ result_adc) & (result_ror ^ result_adc) & 0x80) == 0x80;

                self.write_carry_flag(is_carry);
                self.write_zero_flag(is_zero);
                self.write_negative_flag(is_negative);
                self.write_overflow_flag(is_overflow);
                self.a = result_adc;

                3 + cyc
            }
            Opcode::SLO => {
                // ASL -> ORA
                let (Operand { data: addr, cyc }, arg) = self.fetch_args(system, mode);

                // ASL
                let result_asl = arg.wrapping_shl(1);

                let is_carry = (arg & 0x80) == 0x80; // shift前データでわかるよね
                self.write_carry_flag(is_carry);

                system.write_u8(addr, result_asl, false);

                // ORA
                let result_ora = self.a | result_asl;

                let is_zero = result_ora == 0;
                let is_negative = (result_ora & 0x80) == 0x80;

                self.write_zero_flag(is_zero);
                self.write_negative_flag(is_negative);
                self.a = result_ora;

                3 + cyc
            }
            Opcode::SRE => {
                // LSR -> EOR
                let (Operand { data: addr, cyc }, arg) = self.fetch_args(system, mode);

                // LSR
                let result_lsr = arg.wrapping_shr(1);

                let is_carry = (arg & 0x01) == 0x01;
                self.write_carry_flag(is_carry);

                system.write_u8(addr, result_lsr, false);

                // EOR
                let result_eor = self.a ^ result_lsr;

                let is_zero = result_eor == 0;
                let is_negative = (result_eor & 0x80) == 0x80;

                self.write_zero_flag(is_zero);
                self.write_negative_flag(is_negative);
                self.a = result_eor;

                3 + cyc
            }
            Opcode::SKB => {
                // Fetch Immediate but do nothing
                debug_assert!(mode == AddressingMode::Immediate);
                let (Operand { data: _, cyc }, _arg) = self.fetch_args(system, mode);

                1 + cyc
            }
            Opcode::IGN => {
                // Fetch but do nothing
                let (Operand { data: _, cyc }, _arg) = self.fetch_args(system, mode);

                1 + cyc
            }
        }
    }
}
