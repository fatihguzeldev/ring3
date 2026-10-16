mod calls;
mod context;
mod facade;
mod lifecycle;
mod memory;
pub(in crate::process) mod modules;
mod publication;
mod query;
mod scopes;
mod sources;
mod storage;
pub(in crate::process) use scopes::{PreparedActivationCapture32, PreparedActivationEntry32};

pub use crate::windows::activation::{ActivationConfig32, ActivationError32};
pub(in crate::process) use context::{ActivationRuntime32, ActivationThreadReply32};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActivationWait32 {
    pub epoch: u64,
    pub token: u32,
    pub ready: bool,
}
#[cfg(test)]
mod tests;
