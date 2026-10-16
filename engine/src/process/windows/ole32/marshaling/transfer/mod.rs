mod calls;
mod context;
mod delivery;
mod lifetime;
mod ownership;
mod parents;
mod requests;
mod teardown;
#[cfg(test)]
mod tests;

pub(in crate::process::windows::ole32) use context::{
    TransferCall32, TransferParent32, TransferPhase32,
};
pub(in crate::process::windows::ole32) use teardown::PreparedTransferExit32;
