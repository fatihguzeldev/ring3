use crate::cpu::x86::Register32;

pub(super) const STATE_PTR: u32 = 0;
pub(super) const EXIT_PTR: u32 = 1;
pub(super) const BUDGET: u32 = 2;
pub(super) const CANCEL_PTR: u32 = 3;
pub(super) const EIP: u32 = 12;
pub(super) const FLAGS: u32 = 13;
pub(super) const RETIRED: u32 = 14;
pub(super) const REASON: u32 = 15;
pub(super) const LHS: u32 = 16;
pub(super) const RHS: u32 = 17;
pub(super) const RESULT: u32 = 18;
pub(super) const RESUME: u32 = 19;
pub(super) const MEMORY_BYTES: u32 = 20;

pub(super) fn register(register: Register32) -> u32 {
    4 + register.index() as u32
}
