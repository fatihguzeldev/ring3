mod thread;
pub(in crate::process) use thread::DSoundThreadReply32;
mod activation;
mod buffers;
mod capture;
mod context;
mod device;
mod dispatch;
mod duplex;
mod enumeration;
mod lifetime;
mod locks;
mod memory;
mod notifications;
mod objects;
mod primary;
mod runtime;
mod spatial;
mod termination;
mod types;

pub(in crate::process) use context::DSoundRuntime32;
pub use types::{DSoundConfig32, DSoundError32};

#[cfg(test)]
mod tests;
