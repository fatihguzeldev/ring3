#![forbid(unsafe_code)]

mod api;
pub mod context;
mod handlers;
mod record;

pub use api::{ExceptionApi32, EXCEPTION_RETURN_GATE};
pub use handlers::{ExceptionError32, HandlerId32, HandlerRegistry32};
pub use record::{ExceptionRecord32, EXCEPTION_RECORD_SIZE32};
