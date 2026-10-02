#![forbid(unsafe_code)]

mod callback_frame;
mod calling_convention;

pub use callback_frame::CallbackFrame32;
pub use calling_convention::{CallFrame32, CallingConvention32, FrameError, MAX_STACK_WORDS};
