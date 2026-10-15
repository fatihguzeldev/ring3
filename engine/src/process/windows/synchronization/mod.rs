mod calls;
mod decoding;
mod lifecycle;

use crate::{
    process::calls::call::PendingOwner,
    windows::{
        CallFrame32,
        synchronization::{SynchronizationContext32, WaitResult32, WaitToken32},
    },
};

pub(in crate::process) struct SynchronizationRuntime32 {
    context: SynchronizationContext32,
    reply: Option<Reply32>,
}
#[derive(Clone, Copy)]
struct Reply32 {
    owner: PendingOwner,
    token: u32,
    frame: CallFrame32,
    wait: Option<WaitToken32>,
    result: Option<WaitResult32>,
    deadline: Option<u64>,
    alertable: bool,
    wake_mask: u32,
    boolean_result: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WindowsWait32 {
    pub token: u32,
    pub epoch: u64,
    pub wait_id: u64,
    pub deadline_ns: Option<u64>,
    pub revision: u64,
    pub ready: bool,
    pub wake_mask: u32,
    pub alertable: bool,
}

#[cfg(test)]
mod tests;
