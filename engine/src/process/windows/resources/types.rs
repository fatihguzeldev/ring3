use crate::{
    loader,
    memory::{MemoryError, PageRange},
    process::windows::callbacks::WindowsCallback32,
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResourceConfig32 {
    pub scratch: PageRange,
    pub max_handles: u32,
    pub max_callbacks: u32,
    pub max_enum_bytes: u32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResourceError32 {
    InvalidConfiguration,
    NotConfigured,
    AlreadyConfigured,
    InvalidHandle,
    InvalidParameter,
    Busy,
    Capacity,
    Allocation,
    Win32(u32),
    Memory(MemoryError),
    Pe(loader::ResourceError32),
}
impl ResourceError32 {
    pub fn win32(self) -> u32 {
        match self {
            Self::InvalidHandle => 6,
            Self::Memory(_) => 998,
            Self::Pe(error) => error.win32(),
            Self::Win32(error) => error,
            Self::Capacity | Self::Allocation => 8,
            Self::NotConfigured | Self::AlreadyConfigured | Self::Busy => 170,
            _ => 87,
        }
    }
}
impl From<MemoryError> for ResourceError32 {
    fn from(error: MemoryError) -> Self {
        Self::Memory(error)
    }
}
impl From<loader::ResourceError32> for ResourceError32 {
    fn from(error: loader::ResourceError32) -> Self {
        Self::Pe(error)
    }
}
pub(in crate::process) enum ResourceCallOutcome32 {
    Return { value: u32, last_error: Option<u32> },
    Callback(WindowsCallback32),
}
impl From<u32> for ResourceCallOutcome32 {
    fn from(value: u32) -> Self {
        Self::Return {
            value,
            last_error: None,
        }
    }
}
