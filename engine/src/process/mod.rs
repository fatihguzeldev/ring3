#![forbid(unsafe_code)]

mod instance;
#[cfg(target_arch = "wasm32")]
pub(crate) mod wasm;

pub use instance::{EngineInstance, HostError, StoreCompletion};
