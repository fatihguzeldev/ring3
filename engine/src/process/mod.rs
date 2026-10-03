#![forbid(unsafe_code)]

mod call;
mod callback;
mod callback_code;
mod callback_installation;
mod image;
mod installation;
mod instance;
mod resident;
mod resident_callback;
#[cfg(target_arch = "wasm32")]
pub(crate) mod wasm;

pub use call::CallError;
pub use installation::ResidentInstallation;
pub use instance::{EngineInstance, HostError, StoreCompletion};
