use super::context::{MappingRuntime32, PendingView32};
pub(in crate::process::windows::files) struct MappingThreadReply32 {
    pending: Option<PendingView32>,
}
impl MappingRuntime32 {
    pub(in crate::process::windows::files) fn empty_thread_reply(&self) -> MappingThreadReply32 {
        MappingThreadReply32 { pending: None }
    }
    pub(in crate::process::windows::files) fn take_thread_reply(&mut self) -> MappingThreadReply32 {
        MappingThreadReply32 {
            pending: self.pending.take(),
        }
    }
    pub(in crate::process::windows::files) fn restore_thread_reply(
        &mut self,
        reply: MappingThreadReply32,
    ) {
        assert!(self.pending.is_none());
        self.pending = reply.pending;
    }
}
