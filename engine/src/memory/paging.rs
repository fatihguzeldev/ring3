use crate::{
    cpu::x86::{LegacyPageViolation32, PagingAccess32, PagingDomain32},
    memory::{GuestAddress, MemoryError},
};

/// caller-certified effective legacy32 paging rights, independent of software backing permissions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PagingCertificate32 {
    Unknown,
    NotPresent,
    Present { user: bool, writable: bool },
}
impl PagingCertificate32 {
    pub(super) const fn bits(self) -> u8 {
        match self {
            Self::Unknown => 0,
            Self::NotPresent => 1,
            Self::Present { user, writable } => 2 | ((user as u8) << 2) | ((writable as u8) << 3),
        }
    }
    pub(super) const fn from_bits(bits: u8) -> Self {
        match bits {
            0 => Self::Unknown,
            1 => Self::NotPresent,
            _ => Self::Present {
                user: bits & 4 != 0,
                writable: bits & 8 != 0,
            },
        }
    }
}

/// created only by the actual address-space checker; process code binds the declared profile.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PagingFaultFacts32 {
    pub(super) identity: u64,
    pub(super) linear: GuestAddress,
    pub(super) access: PagingAccess32,
    pub(super) domain: PagingDomain32,
    pub(super) violation: LegacyPageViolation32,
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
    pub(crate) const fn domain(self) -> PagingDomain32 {
        self.domain
    }
    pub(crate) const fn violation(self) -> LegacyPageViolation32 {
        self.violation
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PagingCheckError32 {
    Memory(MemoryError),
    Fault(PagingFaultFacts32),
    Uncertified(GuestAddress),
    UnknownWriteProtect(GuestAddress),
}

#[derive(Clone, Copy)]
pub(crate) enum PagingBacking32 {
    Read,
    Write,
    Modify,
    Execute,
}
