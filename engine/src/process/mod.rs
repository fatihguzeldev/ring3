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
pub use windows::callbacks::{
    WINDOWS_CALLBACK_ARGUMENTS32, WindowsCallback32, WindowsCallbackProvider32,
    WindowsProviderOutcome32,
};
pub use windows::calls::{WindowsGateInfo32, windows_gate_info};
pub use windows::d3d8::{D3D8Config32, D3D8DeviceInfo32, D3D8Error32};
