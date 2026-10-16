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
pub use windows::dinput::{DirectInputConfig32, DirectInputError32};
pub use windows::dsound::{DSoundConfig32, DSoundError32};
pub use windows::synchronization::WindowsWait32;
pub use windows::user32::User32CursorRequest32;

pub use windows::files::{
    AssetRef32, FileConfig32, FileId, FileInfo32, FileProgress32, FileReadRequest32, FileTimes32,
    FileWait32, OverlayEntry32, OverlayKind32, VfsLimits32, WindowsFileError32,
};

pub use windows::files::{FileEnvironment32, FileInput32, FileMountStart32};

pub use execution::cache::ExecutionConfig32;
pub use windows::ole32::{Ole32Config32, Ole32Error32};

pub use windows::process::{ProcessError32, ProcessLaunch32};

pub use crate::windows::memory::{MemoryLimits32, MemoryError32, MemoryInfo32};

pub use execution::cache::{CacheRequest32, CacheRequestKind32, CacheUsage32};

pub use crate::windows::thread::{ThreadLimits32, ThreadError32, ThreadInfo32, ThreadSchedule32};

pub use windows::registry::{RegistryInput32, RegistryChange32, RegistryChangeId32, RegistryConfig32, RegistryError32, RegistryLimits32, RegistrySeed32, RegistryView32, RegistryWait32};

pub use windows::exceptions::{ExceptionDispatch32,ExceptionPhase32};

pub use windows::resources::{ResourceConfig32,ResourceError32};

pub use windows::nls::{NlsError32,NlsProfile32};
