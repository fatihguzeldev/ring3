mod calls;
mod context;
mod enumeration;
mod formatting;
mod lifecycle;
mod lookup;
mod memory;
mod payload;
mod strings;
mod types;
pub(in crate::process) use context::ResourceRuntime32;
pub use types::{ResourceConfig32, ResourceError32};

pub(in crate::process) use types::ResourceCallOutcome32;

#[cfg(test)]
pub(in crate::process::windows) mod tests;
