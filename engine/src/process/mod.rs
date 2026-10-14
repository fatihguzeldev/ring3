#![forbid(unsafe_code)]

mod calls;
mod cpu;
mod execution;
mod instance;
mod loading;
mod memory;
#[cfg(target_arch = "wasm32")]
pub(crate) mod wasm;
mod windows;

pub use calls::call::CallError;
pub use cpu::tasks::TaskReturnAdmission;
pub use execution::installation::ResidentInstallation;
pub use instance::{EngineInstance, HostError, StoreCompletion};
