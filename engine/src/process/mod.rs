#![forbid(unsafe_code)]

mod calls;
mod clock;
mod cpu;
mod execution;
mod instance;
mod loading;
mod media;
mod memory;
#[cfg(target_arch = "wasm32")]
pub(crate) mod wasm;
mod windows;

pub use calls::call::CallError;
pub use cpu::events::{CpuEvent32, CpuInstructionSite32};
pub use execution::installation::ResidentInstallation;
pub use instance::{EngineInstance, HostError, StoreCompletion};
pub use loading::modules::{
    ModuleInitializer32, ModuleInitializerKind32, ModuleOperation32, ModuleRuntimeConfig32,
};
pub use media::{MediaConfig, MediaError};
