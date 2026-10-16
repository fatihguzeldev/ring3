use super::{D3D8Runtime32, resources::PendingPrivateCall};
pub(in crate::process) struct D3D8ThreadReply32 {
    callbacks: Vec<PendingPrivateCall>,
}
impl D3D8Runtime32 {
    pub(in crate::process) fn empty_thread_reply(&self) -> D3D8ThreadReply32 {
        D3D8ThreadReply32 {
            callbacks: Vec::new(),
        }
    }
    pub(in crate::process) fn take_thread_reply(&mut self) -> D3D8ThreadReply32 {
        D3D8ThreadReply32 {
            callbacks: std::mem::take(&mut self.private_callbacks),
        }
    }
    pub(in crate::process) fn restore_thread_reply(&mut self, reply: D3D8ThreadReply32) {
        assert!(self.private_callbacks.is_empty());
        self.private_callbacks = reply.callbacks;
    }
}
