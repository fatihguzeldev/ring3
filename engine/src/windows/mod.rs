#![forbid(unsafe_code)]

mod api;
mod callback_frame;
mod calling_convention;
mod modules;
mod provider;

pub(crate) use api::ProviderProfile;
pub use api::WindowsApi32;
pub use callback_frame::CallbackFrame32;
pub use calling_convention::{CallFrame32, CallingConvention32, FrameError, MAX_STACK_WORDS};
pub(crate) use provider::{ProcessContext32, ThreadState32, WindowsOutcome32};

pub use modules::kernel32_module;
