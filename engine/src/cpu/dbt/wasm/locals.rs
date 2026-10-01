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
pub(super) const DETAIL: u32 = 21;
pub(super) const FAULT_ADDRESS: u32 = 22;
pub(super) const FAULT_ACCESS: u32 = 23;
pub(super) const FAULT_LENGTH: u32 = 24;
pub(super) const HELPER_STATUS: u32 = 25;
pub(super) const ADDRESS: u32 = 26;

pub(super) fn register(register: Register32) -> u32 {
    4 + register.index() as u32
}
