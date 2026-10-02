#![forbid(unsafe_code)]

mod calling_convention;

pub use calling_convention::{CallFrame32, CallingConvention32, FrameError, MAX_STACK_WORDS};
