mod snapshot;
pub use snapshot::{CpuEvent32, CpuInstructionSite32};

#[cfg(any(target_arch = "wasm32", test))]
pub(in crate::process) mod detection;
