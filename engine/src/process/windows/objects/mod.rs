mod allocation;
mod lifetime;
mod prepared;
mod publication;
pub(crate) use prepared::PreparedGuestLifetime32;

use crate::{
    memory::{GuestAddress, MemoryError, PageRange},
    windows::com::{ComError32, ComObjectId32, ComRegistry32, InterfaceId},
};

#[derive(Clone, Copy)]
pub(crate) struct InterfaceLayout32<'a> {
    pub iid: InterfaceId,
    pub methods: &'a [GuestAddress],
}

pub(crate) struct GuestComCreation32<'a> {
    pub output: Option<(GuestAddress, InterfaceId)>,
    pub writes: &'a [crate::memory::WordWrite32],
    pub initial: crate::windows::com::ReferenceState32,
    pub retain: &'a [GuestAddress],
}

impl Default for GuestComCreation32<'_> {
    fn default() -> Self {
        Self {
            output: None,
            writes: &[],
            initial: crate::windows::com::ReferenceState32 {
                references: 1,
                holds: 0,
            },
            retain: &[],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GuestComError32 {
    InvalidLimits,
    InvalidLayout,
    Capacity,
    Allocation,
    StaleMapping,
    Com(ComError32),
    Memory(MemoryError),
}

struct Vtable32 {
    iid: InterfaceId,
    methods: Box<[GuestAddress]>,
    address: GuestAddress,
    mapping_version: u64,
}

struct ObjectAllocation32 {
    id: ComObjectId32,
    range: PageRange,
    mapping_version: u64,
}

pub(crate) struct GuestComObjects32 {
    registry: ComRegistry32,
    bounds: PageRange,
    next_page: usize,
    memory_identity: Option<u64>,
    vtable_limit: usize,
    vtables: Vec<Vtable32>,
    allocations: Vec<ObjectAllocation32>,
}

impl From<ComError32> for GuestComError32 {
    fn from(error: ComError32) -> Self {
        Self::Com(error)
    }
}

impl From<MemoryError> for GuestComError32 {
    fn from(error: MemoryError) -> Self {
        Self::Memory(error)
    }
}

#[cfg(test)]
mod tests;
