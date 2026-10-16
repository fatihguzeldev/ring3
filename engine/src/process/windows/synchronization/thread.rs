use super::{Reply32, SynchronizationRuntime32};
#[derive(Clone, Copy)]
pub(in crate::process) struct SynchronizationThreadReply32 {
    reply: Option<Reply32>,
}
impl SynchronizationRuntime32 {
    pub(in crate::process) fn empty_thread_reply(&self) -> SynchronizationThreadReply32 {
        SynchronizationThreadReply32 { reply: None }
    }
    pub(in crate::process) fn take_thread_reply(&mut self) -> SynchronizationThreadReply32 {
        SynchronizationThreadReply32 {
            reply: self.reply.take(),
        }
    }
    pub(in crate::process) fn restore_thread_reply(&mut self, reply: SynchronizationThreadReply32) {
        assert!(self.reply.is_none());
        self.reply = reply.reply;
    }
}

impl SynchronizationThreadReply32 {
    pub(in crate::process) fn ready(
        &self,
        context: &crate::windows::synchronization::SynchronizationContext32,
    ) -> bool {
        self.reply.is_some_and(|reply| {
            reply.result.is_some()
                || reply
                    .wait
                    .is_some_and(|wait| context.wait_status(wait).ok().flatten().is_some())
        })
    }
    pub(in crate::process) fn deadline(&self) -> Option<u64> {
        self.reply.and_then(|reply| reply.deadline)
    }
}

impl SynchronizationRuntime32 {
    pub(in crate::process) fn active_ready(&self) -> bool {
        self.reply.is_some_and(|reply| {
            reply.result.is_some()
                || reply
                    .wait
                    .is_some_and(|wait| self.context.wait_status(wait).ok().flatten().is_some())
        })
    }
    pub(in crate::process) fn active_alertable(&self) -> bool {
        self.reply.is_some_and(|reply| reply.alertable)
    }
}
