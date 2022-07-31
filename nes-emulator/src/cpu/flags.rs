use super::cpu::*;

impl Cpu {
    #[inline]
    pub(super) fn set_negative_flag(&mut self, is_negative: bool) {
        self.p.set(Flags::NEGATIVE, is_negative);
    }
    #[inline]
    pub(super) fn set_overflow_flag(&mut self, overflow: bool) {
        self.p.set(Flags::OVERFLOW, overflow);
    }
    #[inline]
    pub(super) fn set_decimal_flag(&mut self, decimal: bool) {
        self.p.set(Flags::DECIMAL, decimal);
    }
    #[inline]
    pub(super) fn set_interrupt_flag(&mut self, interrupt: bool) {
        self.p.set(Flags::INTERRUPT, interrupt);
    }
    #[inline]
    pub(super) fn set_zero_flag(&mut self, is_zero: bool) {
        self.p.set(Flags::ZERO, is_zero);
    }
    #[inline]
    pub(super) fn set_carry_flag(&mut self, carry: bool) {
        self.p.set(Flags::CARRY, carry);
    }
    #[inline]
    pub(super) fn negative_flag(&self) -> bool {
        self.p.contains(Flags::NEGATIVE)
    }
    #[inline]
    pub(super) fn overflow_flag(&self) -> bool {
        self.p.contains(Flags::OVERFLOW)
    }
    #[allow(dead_code)]
    #[inline]
    pub(super) fn decimal_flag(&self) -> bool {
        self.p.contains(Flags::DECIMAL)
    }
    #[allow(dead_code)]
    #[inline]
    pub(super) fn interrupt_flag(&self) -> bool {
        self.p.contains(Flags::INTERRUPT)
    }
    #[inline]
    pub(super) fn zero_flag(&self) -> bool {
        self.p.contains(Flags::ZERO)
    }
    #[inline]
    pub(super) fn carry_flag(&self) -> bool {
        self.p.contains(Flags::CARRY)
    }
}
