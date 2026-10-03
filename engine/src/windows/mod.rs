#![forbid(unsafe_code)]

mod callback_frame;
mod calling_convention;
mod provider;

pub use callback_frame::CallbackFrame32;
pub use calling_convention::{CallFrame32, CallingConvention32, FrameError, MAX_STACK_WORDS};
pub use provider::WindowsApi32;
pub(crate) use provider::{ThreadState32, WindowsOutcome32};
