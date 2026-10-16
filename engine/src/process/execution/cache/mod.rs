mod admission;
mod configuration;
mod context;
mod directory;
mod pins;
mod promotion;
mod requests;
mod types;

pub(in crate::process) use context::ExecutionCache32;
pub use context::ExecutionConfig32;
pub use types::{CacheRequest32, CacheRequestKind32, CacheUsage32};

#[cfg(test)]
mod tests;
