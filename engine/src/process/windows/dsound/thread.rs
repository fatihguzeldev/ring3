use super::{DSoundRuntime32, enumeration::PendingEnumeration32};
pub(in crate::process) struct DSoundThreadReply32 {
    enumerations: Vec<PendingEnumeration32>,
}
impl DSoundRuntime32 {
    pub(in crate::process) fn empty_thread_reply(&self) -> DSoundThreadReply32 {
        DSoundThreadReply32 {
            enumerations: Vec::new(),
        }
    }
    pub(in crate::process) fn take_thread_reply(&mut self) -> DSoundThreadReply32 {
        DSoundThreadReply32 {
            enumerations: std::mem::take(&mut self.enumerations),
        }
    }
    pub(in crate::process) fn restore_thread_reply(&mut self, reply: DSoundThreadReply32) {
        assert!(self.enumerations.is_empty());
        self.enumerations = reply.enumerations;
    }
}
