mod context;
mod delivery;
mod invocation;
mod lifetime;
mod requests;
#[cfg(test)]
mod tests;
pub(in crate::process::windows::ole32) use context::StreamCopy32;
pub(in crate::process::windows::ole32) use lifetime::PreparedDestroyedStreams32;
pub(in crate::process::windows::ole32) use lifetime::PreparedStreamCopyExit32;
