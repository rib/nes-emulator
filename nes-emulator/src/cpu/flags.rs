use super::cpu::*;

impl Cpu {
    pub fn set_negative_flag(&mut self, is_negative: bool) {
        self.p.set(Flags::NEGATIVE, is_negative);
    }
    pub fn set_overflow_flag(&mut self, overflow: bool) {
        self.p.set(Flags::OVERFLOW, overflow);
    }
    pub fn set_decimal_flag(&mut self, decimal: bool) {
        self.p.set(Flags::DECIMAL, decimal);
    }
    pub fn set_interrupt_flag(&mut self, interrupt: bool) {
        self.p.set(Flags::INTERRUPT, interrupt);
    }
    pub fn set_zero_flag(&mut self, is_zero: bool) {
        self.p.set(Flags::ZERO, is_zero);
    }
    pub fn set_carry_flag(&mut self, carry: bool) {
        self.p.set(Flags::CARRY, carry);
    }
    pub fn negative_flag(&self) -> bool {
        self.p.contains(Flags::NEGATIVE)
    }
    pub fn overflow_flag(&self) -> bool {
        self.p.contains(Flags::OVERFLOW)
    }
    pub fn decimal_flag(&self) -> bool {
        self.p.contains(Flags::DECIMAL)
    }
    pub fn interrupt_flag(&self) -> bool {
        self.p.contains(Flags::INTERRUPT)
    }
    pub fn zero_flag(&self) -> bool {
        self.p.contains(Flags::ZERO)
    }
    pub fn carry_flag(&self) -> bool {
        self.p.contains(Flags::CARRY)
    }
}
