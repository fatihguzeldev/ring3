use crate::memory::GuestAddress;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct InterfaceId(pub [u8; 16]);

pub const IID_IUNKNOWN: InterfaceId =
    InterfaceId([0, 0, 0, 0, 0, 0, 0, 0, 0xc0, 0, 0, 0, 0, 0, 0, 0x46]);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ComObjectId32(u64);

impl ComObjectId32 {
    pub fn get(self) -> u64 {
        self.0
    }

    pub(super) fn new(value: u64) -> Self {
        Self(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InterfaceBinding32 {
    pub iid: InterfaceId,
    pub address: GuestAddress,
    pub vtable: GuestAddress,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ComIdentity32 {
    pub id: ComObjectId32,
    pub identity_pointer: GuestAddress,
    pub payload: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReleaseOutcome32 {
    pub remaining: u32,
    pub destroyed: Option<ComIdentity32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReferenceState32 {
    pub references: u32,
    pub holds: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReferenceTransition32 {
    pub identity: ComIdentity32,
    pub before: ReferenceState32,
    pub after: ReferenceState32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ComError32 {
    InvalidLimits,
    InvalidBinding,
    InvalidPointer,
    NoInterface,
    PointerReuse,
    Capacity,
    Exhausted,
    ReferenceUnderflow,
    Allocation,
}
