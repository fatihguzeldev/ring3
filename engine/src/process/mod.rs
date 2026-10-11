#![forbid(unsafe_code)]

mod call;
mod callback;
mod callback_code;
mod callback_installation;
mod cpu_integer;
mod cpu_memory;
mod image;
mod image_input;
mod installation;
#[cfg(test)]
mod installed_resident_disposal_tests;
mod instance;
mod resident;
mod resident_callback;
mod segments;
mod simd;
mod stack;
mod startup;
mod virtual_memory;
#[cfg(target_arch = "wasm32")]
pub(crate) mod wasm;
mod windows;
mod x87;

pub use call::CallError;
pub use installation::ResidentInstallation;
pub use instance::{EngineInstance, HostError, StoreCompletion};
