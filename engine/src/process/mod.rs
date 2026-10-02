#![forbid(unsafe_code)]

mod call;
mod callback;
mod callback_code;
mod instance;
mod resident;
#[cfg(target_arch = "wasm32")]
pub(crate) mod wasm;

pub use call::CallError;
pub use instance::{EngineInstance, HostError, StoreCompletion};
