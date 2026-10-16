mod api;
mod block;

pub use api::{VersionApi32, version_module};
pub(crate) use block::VersionBlock32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VersionError32 {
    InvalidParameter,
    InvalidData,
    NotFound,
    InsufficientBuffer,
    Allocation,
    Memory(crate::memory::MemoryError),
    Win32(u32),
}
impl VersionError32 {
    pub fn win32(self) -> u32 {
        match self {
            Self::InvalidParameter => 87,
            Self::InvalidData => 13,
            Self::NotFound => 1814,
            Self::InsufficientBuffer => 122,
            Self::Allocation => 8,
            Self::Memory(_) => 998,
            Self::Win32(code) => code,
        }
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
pub(crate) use tests::fixture as version_fixture;
