use super::{OrdinaryRuntime32, progress::PendingCopy32};
pub(in crate::process::windows::files) struct OrdinaryThreadReply32 {
    copies: Vec<PendingCopy32>,
    error_mode: u32,
}
impl OrdinaryThreadReply32 {
    pub(super) fn copies(&self) -> &[PendingCopy32] {
        &self.copies
    }
}
impl OrdinaryRuntime32 {
    pub(in crate::process::windows::files) fn empty_thread_reply(&self) -> OrdinaryThreadReply32 {
        OrdinaryThreadReply32 {
            copies: Vec::new(),
            error_mode: 0,
        }
    }
    pub(in crate::process::windows::files) fn take_thread_reply(
        &mut self,
    ) -> OrdinaryThreadReply32 {
        OrdinaryThreadReply32 {
            copies: std::mem::take(&mut self.copies),
            error_mode: self.thread_error_mode,
        }
    }
    pub(in crate::process::windows::files) fn restore_thread_reply(
        &mut self,
        reply: OrdinaryThreadReply32,
    ) {
        assert!(self.copies.is_empty());
        self.copies = reply.copies;
        self.thread_error_mode = reply.error_mode;
    }
}
