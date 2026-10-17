mod api;
mod heap;
mod regions;
mod types;

pub use api::MemoryApi32;
pub(crate) use heap::{
    GlobalMemory32, HeapManager32, PreparedGlobalMemory32, PreparedGlobalRetirement32,
    PreparedHeapAllocation32, PreparedHeapAllocations32, PreparedHeapRetirement32,
};
pub(crate) use regions::VirtualMemory32;
pub(crate) use regions::span;
pub use types::*;

#[cfg(test)]
mod tests;
