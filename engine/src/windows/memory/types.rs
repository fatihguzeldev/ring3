use crate::memory::{MemoryError, Permissions};

pub const MEM_COMMIT: u32 = 0x1000;
pub const MEM_RESERVE: u32 = 0x2000;
pub const MEM_DECOMMIT: u32 = 0x4000;
pub const MEM_RELEASE: u32 = 0x8000;
pub const MEM_FREE: u32 = 0x10000;
pub const MEM_PRIVATE: u32 = 0x20000;
pub const MEM_RESET: u32 = 0x80000;
pub const MEM_TOP_DOWN: u32 = 0x100000;
pub const HEAP_NO_SERIALIZE: u32 = 1;
pub const HEAP_GENERATE_EXCEPTIONS: u32 = 4;
pub const HEAP_ZERO_MEMORY: u32 = 8;
pub const HEAP_REALLOC_IN_PLACE_ONLY: u32 = 0x10;
pub const PAGE_GUARD: u32 = 0x100;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MemoryLimits32 {
    pub regions: usize,
    pub fragments: usize,
    pub heaps: usize,
    pub blocks: usize,
    pub moveable_handles: usize,
    pub heap_reserve_bytes: u32,
}
impl Default for MemoryLimits32 {
    fn default() -> Self {
        Self {
            regions: 4096,
            fragments: 65536,
            heaps: 64,
            blocks: 65536,
            moveable_handles: 4096,
            heap_reserve_bytes: 64 * 1024 * 1024,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemoryError32 {
    InvalidLimits,
    InvalidParameter,
    InvalidAddress,
    InvalidHandle,
    AccessDenied,
    NoAccess,
    Capacity,
    Allocation,
    Unsupported,
    Busy,
    Exhausted,
    NoMoreItems,
    NotLocked,
    Discarded,
    HeapCorruption,
    InsufficientBuffer,
}
impl MemoryError32 {
    pub fn win32(self) -> u32 {
        match self {
            Self::InvalidAddress => 487,
            Self::InvalidHandle => 6,
            Self::AccessDenied => 5,
            Self::NoAccess => 998,
            Self::Capacity | Self::Allocation | Self::Exhausted => 8,
            Self::Unsupported => 50,
            Self::Busy => 170,
            Self::NoMoreItems => 259,
            Self::NotLocked => 158,
            Self::Discarded => 157,
            Self::InsufficientBuffer => 122,
            _ => 87,
        }
    }
}
impl From<MemoryError> for MemoryError32 {
    fn from(value: MemoryError) -> Self {
        match value {
            MemoryError::Capacity => Self::Capacity,
            MemoryError::Allocation => Self::Allocation,
            MemoryError::VersionExhausted => Self::Exhausted,
            MemoryError::Fault(_) => Self::NoAccess,
            MemoryError::AlreadyMapped { .. } | MemoryError::NotMapped { .. } => {
                Self::InvalidAddress
            }
            MemoryError::InvalidRange => Self::InvalidParameter,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MemoryInfo32 {
    pub base: u32,
    pub allocation_base: u32,
    pub allocation_protection: u32,
    pub size: u32,
    pub state: u32,
    pub protection: u32,
    pub kind: u32,
}
impl MemoryInfo32 {
    pub(crate) fn bytes(self) -> [u8; 28] {
        let mut bytes = [0; 28];
        for (index, word) in [
            self.base,
            self.allocation_base,
            self.allocation_protection,
            self.size,
            self.state,
            self.protection,
            self.kind,
        ]
        .into_iter()
        .enumerate()
        {
            bytes[index * 4..index * 4 + 4].copy_from_slice(&word.to_le_bytes());
        }
        bytes
    }
}
pub(crate) fn protection(value: u32) -> Result<Permissions, MemoryError32> {
    let permissions = match value & !PAGE_GUARD {
        1 => Permissions::NONE,
        2 => Permissions::READ,
        4 | 8 => Permissions::READ_WRITE,
        0x10 => Permissions::EXECUTE,
        0x20 => Permissions::READ_EXECUTE,
        0x40 | 0x80 => Permissions::ALL,
        _ => {
            return Err(MemoryError32::InvalidParameter);
        }
    };
    if value & PAGE_GUARD != 0 {
        if permissions == Permissions::NONE {
            return Err(MemoryError32::InvalidParameter);
        }
        Ok(permissions.with_guard())
    } else {
        Ok(permissions)
    }
}
pub(crate) fn protection_value(value: Permissions) -> u32 {
    let guard = if value.is_guarded() { PAGE_GUARD } else { 0 };
    guard
        | match value.without_guard() {
            Permissions::NONE => 1,
            Permissions::READ => 2,
            Permissions::READ_WRITE => 4,
            Permissions::EXECUTE => 0x10,
            Permissions::READ_EXECUTE => 0x20,
            Permissions::ALL => 0x40,
            _ => 1,
        }
}
