#![forbid(unsafe_code)]

mod api;
mod callback_frame;
mod calling_convention;
mod modules;
mod provider;

pub mod com;
pub mod synchronization;
pub mod user32;
pub mod window;

pub(crate) use api::ProviderProfile;
pub use api::WindowsApi32;
pub use callback_frame::CallbackFrame32;
pub(crate) use calling_convention::validate_call_context;
pub use calling_convention::{CallFrame32, CallingConvention32, FrameError, MAX_STACK_WORDS};
pub(crate) use provider::{ProcessContext32, ThreadState32, WindowsOutcome32};

pub use modules::kernel32_module;

pub mod files;

pub mod text;

pub mod process;

pub mod ole32;
pub mod winmm;

pub mod memory;

pub mod exceptions;
