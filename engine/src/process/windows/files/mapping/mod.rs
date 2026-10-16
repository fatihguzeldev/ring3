mod thread;
pub(in crate::process::windows::files) use thread::MappingThreadReply32;
mod calls;
mod context;
mod lifecycle;
mod protection;
mod query;
mod termination;
mod views;
mod writeback;

pub(super) use context::{MappingLimits32, MappingRuntime32};

#[cfg(test)]
mod tests;
