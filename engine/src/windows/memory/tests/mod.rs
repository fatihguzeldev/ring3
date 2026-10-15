mod regions;

use super::*;
use crate::memory::{AddressSpace, GuestAddress, PageRange, Permissions};

fn memory() -> AddressSpace {
    AddressSpace::new(64).unwrap()
}
fn virtual_memory() -> VirtualMemory32 {
    VirtualMemory32::new(MemoryLimits32::default()).unwrap()
}
