#![forbid(unsafe_code)]

mod calls;
mod clock;
#[cfg(test)]
#[path = "wasm/core/numeric.rs"]
mod core_numeric_tests;
mod cpu;
mod execution;
mod instance;
mod loading;
mod media;
mod memory;
#[cfg(feature = "observability")]
mod observability;
#[cfg(target_arch = "wasm32")]
pub(crate) mod wasm;
mod windows;

pub use calls::call::CallError;
pub use cpu::events::{CpuEvent32, CpuInstructionSite32};
pub use execution::continuation::CompiledExecutionUsage32;
pub use execution::installation::ResidentInstallation;
pub use execution::receipt::{ExecutionFrameStart32, ExecutionReceipt32};
pub use instance::{EngineInstance, HostError, StoreCompletion};
pub use loading::modules::{
    ModuleInitializer32, ModuleInitializerKind32, ModuleOperation32, ModuleRuntimeConfig32,
    ModuleSourceWait32,
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
pub use windows::user32::{
    PendingUser32ImageCall32, User32CursorRequest32, User32DialogIcon32, User32DialogRequest32,
    User32WindowInfo32,
};

pub use windows::files::{
    AssetRef32, FileConfig32, FileId, FileInfo32, FileProgress32, FileReadRequest32, FileTimes32,
    FileWait32, OverlayEntry32, OverlayKind32, VfsLimits32, WindowsFileError32,
};

pub use windows::files::{FileEnvironment32, FileInput32, FileMountStart32};

pub use execution::cache::ExecutionConfig32;
pub use windows::ole32::{Ole32Config32, Ole32Error32};

pub use windows::process::{ProcessError32, ProcessLaunch32};

pub use crate::windows::memory::{MemoryError32, MemoryInfo32, MemoryLimits32};

pub use execution::cache::{CacheRequest32, CacheRequestKind32, CacheUsage32};

pub use crate::windows::thread::{ThreadError32, ThreadInfo32, ThreadLimits32, ThreadSchedule32};

pub use windows::registry::{
    RegistryChange32, RegistryChangeId32, RegistryConfig32, RegistryError32, RegistryInput32,
    RegistryLimits32, RegistrySeed32, RegistryView32, RegistryWait32,
};

pub use windows::exceptions::{ExceptionDispatch32, ExceptionPhase32};

pub use windows::resources::{ResourceConfig32, ResourceError32};

pub use windows::activation::{ActivationConfig32, ActivationError32, ActivationWait32};
pub use windows::nls::{NlsError32, NlsProfile32};

pub use windows::gdi::{GdiConfig32, GdiFrame32};
pub use windows::version::PendingVersionCall32;

#[cfg(test)]
#[path = "wasm/core/cache.rs"]
mod core_cache_tests;
