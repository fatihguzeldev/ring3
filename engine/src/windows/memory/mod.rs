mod api;
mod heap;
mod regions;
mod types;

pub use api::MemoryApi32;
pub(crate) use heap::{HeapManager32, PreparedHeapAllocation32};
pub(crate) use regions::VirtualMemory32;
pub(crate) use regions::span;
pub use types::*;

#[cfg(test)]
mod tests;
