mod callbacks;
mod calls;

use crate::{
    process::calls::call::PendingOwner,
    windows::{
        CallFrame32,
        thread::{ThreadError32, fls::FlsContext32},
    },
};

pub(super) struct FlsReply32 {
    thread: u32,
    owner: PendingOwner,
    token: u32,
    frame: CallFrame32,
    operation: u64,
}
pub(super) struct FlsRuntime32 {
    pub(super) context: FlsContext32,
    pub(super) replies: Vec<FlsReply32>,
}
impl FlsRuntime32 {
    pub(super) fn new(indices: u32, threads: u32) -> Result<Self, ThreadError32> {
        let context = FlsContext32::new(indices, threads)?;
        let mut replies = Vec::new();
        replies
            .try_reserve_exact(32)
            .map_err(|_| ThreadError32::Allocation)?;
        Ok(Self { context, replies })
    }
    pub(super) fn retire_thread(&mut self, thread: u32) {
        self.context.retire_thread(thread);
        self.replies.retain(|reply| reply.thread != thread);
    }
}
