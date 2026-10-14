use super::VerifiedPageFault32;
use crate::memory::MemoryError;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CpuPagingError32 {
    Memory(MemoryError),
    PageFault(VerifiedPageFault32),
}

mod port;
pub(crate) use port::{CpuMemoryPort32, CpuReadPort32};
#[cfg(test)]
pub(crate) use port::{DefaultCpuMemoryPort32, DefaultCpuReadPort32};

mod inspect;
pub(crate) use inspect::inspect_cpu_paging_access32;
