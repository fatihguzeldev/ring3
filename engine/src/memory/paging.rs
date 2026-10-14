use crate::{
    cpu::x86::PagingAccess32,
    memory::{GuestAddress, MemoryError},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PagingFaultFacts32 {
    pub(super) identity: u64,
    pub(super) linear: GuestAddress,
    pub(super) access: PagingAccess32,
    pub(super) present: bool,
}

impl PagingFaultFacts32 {
    pub(crate) const fn identity(self) -> u64 {
        self.identity
    }
    pub(crate) const fn linear(self) -> GuestAddress {
        self.linear
    }
    pub(crate) const fn access(self) -> PagingAccess32 {
        self.access
    }
    pub(crate) const fn present(self) -> bool {
        self.present
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PagingCheckError32 {
    Memory(MemoryError),
    Fault(PagingFaultFacts32),
}

#[derive(Clone, Copy)]
pub(crate) enum PagingBacking32 {
    Read,
    Write,
    Modify,
    Execute,
}
