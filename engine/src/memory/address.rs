pub const PAGE_SIZE: u32 = 4096;
pub(crate) const GUEST_PAGES: usize = 1 << 20;
pub(crate) const ADDRESS_LIMIT: u64 = 1 << 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GuestAddress(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BackingOffset(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    Read,
    Write,
    Execute,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Permissions(u8);

impl Permissions {
    pub const NONE: Self = Self(0);
    pub const READ: Self = Self(1);
    pub const READ_WRITE: Self = Self(3);
    pub const EXECUTE: Self = Self(4);
    pub const READ_EXECUTE: Self = Self(5);
    pub const ALL: Self = Self(7);

    pub const fn from_bits(bits: u8) -> Option<Self> {
        if bits & !7 == 0 {
            Some(Self(bits))
        } else {
            None
        }
    }

    pub(crate) fn allows(self, access: Access) -> bool {
        self.0
            & match access {
                Access::Read => 1,
                Access::Write => 2,
                Access::Execute => 4,
            }
            != 0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PageRange {
    pub(crate) first: usize,
    pub(crate) count: usize,
}

impl PageRange {
    pub fn new(address: GuestAddress, page_count: u32) -> Result<Self, MemoryError> {
        if !address.0.is_multiple_of(PAGE_SIZE)
            || page_count == 0
            || u64::from(address.0) + u64::from(page_count) * u64::from(PAGE_SIZE) > ADDRESS_LIMIT
        {
            return Err(MemoryError::InvalidRange);
        }
        Ok(Self {
            first: (address.0 / PAGE_SIZE) as usize,
            count: page_count as usize,
        })
    }

    pub(crate) fn indices(self) -> std::ops::Range<usize> {
        self.first..self.first + self.count
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FaultReason {
    Unmapped,
    Permission,
    AddressOverflow,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MemoryFault {
    pub address: GuestAddress,
    pub access: Access,
    pub reason: FaultReason,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemoryError {
    InvalidRange,
    Capacity,
    Allocation,
    AlreadyMapped { address: GuestAddress },
    NotMapped { address: GuestAddress },
    VersionExhausted,
    Fault(MemoryFault),
}
