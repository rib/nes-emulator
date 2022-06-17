use super::cpu::*;

/// Processor Status Flag Implementation
impl Cpu {
    pub fn write_negative_flag(&mut self, is_active: bool) {
        if is_active {
            self.p = self.p | Flags::NEGATIVE;
        } else {
            self.p = self.p & (!Flags::NEGATIVE);
        }
    }
    pub fn write_overflow_flag(&mut self, is_active: bool) {
        if is_active {
            self.p = self.p | Flags::OVERFLOW;
        } else {
            self.p = self.p & (!Flags::OVERFLOW);
        }
    }
    /*
    pub fn write_reserved_flag(&mut self, is_active: bool) {
        if is_active {
            self.p = self.p | 0x20u8;
        } else {
            self.p = self.p & (!0x20u8);
        }
    }
    pub fn write_break_flag(&mut self, is_active: bool) {
        if is_active {
            self.p = self.p | 0x10u8;
        } else {
            self.p = self.p & (!0x10u8);
        }
    }
    */
    pub fn write_decimal_flag(&mut self, is_active: bool) {
        if is_active {
            self.p = self.p | Flags::DECIMAL;
        } else {
            self.p = self.p & (!Flags::DECIMAL);
        }
    }
    pub fn write_interrupt_flag(&mut self, is_active: bool) {
        if is_active {
            self.p = self.p | Flags::INTERRUPT;
        } else {
            self.p = self.p & (!Flags::INTERRUPT);
        }
    }
    pub fn write_zero_flag(&mut self, is_active: bool) {
        if is_active {
            self.p = self.p | Flags::ZERO;
        } else {
            self.p = self.p & (!Flags::ZERO);
        }
    }
    pub fn write_carry_flag(&mut self, is_active: bool) {
        if is_active {
            self.p = self.p | Flags::CARRY;
        } else {
            self.p = self.p & (!Flags::CARRY);
        }
    }
    pub fn read_negative_flag(&self) -> bool {
        (self.p & Flags::NEGATIVE) == Flags::NEGATIVE
    }
    pub fn read_overflow_flag(&self) -> bool {
        (self.p & Flags::OVERFLOW) == Flags::OVERFLOW
    }
    /*
    pub fn read_reserved_flag(&self) -> bool {
        (self.p & 0x20u8) == 0x20u8
    }
    pub fn read_break_flag(&self) -> bool {
        (self.p & 0x10u8) == 0x10u8
    }
    */
    pub fn read_decimal_flag(&self) -> bool {
        (self.p & Flags::DECIMAL) == Flags::DECIMAL
    }
    pub fn read_interrupt_flag(&self) -> bool {
        (self.p & Flags::INTERRUPT) == Flags::INTERRUPT
    }
    pub fn read_zero_flag(&self) -> bool {
        (self.p & Flags::ZERO) == Flags::ZERO
    }
    pub fn read_carry_flag(&self) -> bool {
        (self.p & Flags::CARRY) == Flags::CARRY
    }
}
