mod admission;
mod cohort;
mod configuration;
mod context;
mod directory;
mod edges;
mod pins;
mod promotion;
mod requests;
mod slots;
mod types;
mod victims;

pub(in crate::process) use context::ExecutionCache32;
pub use context::ExecutionConfig32;
pub use types::{CacheRequest32, CacheRequestKind32, CacheUsage32};

#[cfg(test)]
mod tests;
