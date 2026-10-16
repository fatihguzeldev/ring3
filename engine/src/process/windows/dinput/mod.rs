mod thread;
pub(in crate::process) use thread::DirectInputThreadReply32;
mod activation;
mod context;
mod devices;
mod dispatch;
mod enumeration;
mod lifetime;
mod memory;
mod objects;
mod runtime;
mod termination;
mod types;
pub(in crate::process) use context::DirectInputRuntime32;
pub use types::{DirectInputConfig32, DirectInputError32};

#[cfg(test)]
mod tests;
