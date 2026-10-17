#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum DialogError32 {
    StructSize = 1,
    Initialization = 2,
    NoTemplate = 3,
    NoInstance = 4,
    FindResource = 6,
    Allocation = 9,
    NoHook = 11,
    InvalidFileName = 0x3002,
    BufferTooSmall = 0x3003,
    DialogFailure = 0xffff,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ComdlgError32 {
    Memory(crate::memory::MemoryError),
    Dialog(DialogError32),
    Allocation,
    Capacity,
    InvalidRequest,
    StateChanged,
}

impl From<crate::memory::MemoryError> for ComdlgError32 {
    fn from(error: crate::memory::MemoryError) -> Self {
        Self::Memory(error)
    }
}

impl ComdlgError32 {
    pub(crate) fn win32(&self) -> u32 {
        match self {
            Self::Memory(_) => 998,
            Self::Allocation | Self::Capacity => 8,
            Self::StateChanged => 6,
            _ => 87,
        }
    }
}
