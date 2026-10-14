use super::PreparedTaskReturn;
use crate::memory::{MemoryError, MemoryFault};

/// admission never saves, switches, or retires a task; admitted data still needs transition authority.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskReturnAdmission {
    Admitted(PreparedTaskReturn),
    InvalidTss(u32),
    NotPresent(u32),
    MissingAuthority,
    NeedsPagingAuthority { fault: MemoryFault, length: u32 },
    Infrastructure(MemoryError),
}
