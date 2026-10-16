mod api;
mod paths;

pub use api::{ShlwapiApi32, shlwapi_module};
pub(crate) use paths::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShlwapiError32 {
    InvalidParameter,
    FilenameTooLong,
    Allocation,
    Memory(crate::memory::MemoryError),
}
impl ShlwapiError32 {
    pub fn win32(self) -> u32 {
        match self {
            Self::InvalidParameter => 87,
            Self::FilenameTooLong => 206,
            Self::Allocation => 8,
            Self::Memory(_) => 998,
        }
    }
}

#[cfg(test)]
mod tests;
