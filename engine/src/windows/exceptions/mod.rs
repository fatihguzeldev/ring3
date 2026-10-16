#![forbid(unsafe_code)]

mod api;
pub mod context;
mod handlers;
mod record;

pub use api::{EXCEPTION_RETURN_GATE, ExceptionApi32, ntdll_module};
pub use handlers::{ExceptionError32, HandlerId32, HandlerRegistry32};
pub use record::{EXCEPTION_RECORD_SIZE32, ExceptionRecord32};
