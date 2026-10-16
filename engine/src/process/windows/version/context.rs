use crate::{
    files::FileReadRequest32,
    process::calls::call::PendingOwner,
    windows::{CallFrame32, version::VersionApi32},
};

#[derive(Default)]
pub(in crate::process) struct VersionRuntime32 {
    pub(super) reply: Option<VersionThreadReply32>,
}
pub(in crate::process) struct VersionThreadReply32 {
    pub(super) owner: PendingOwner,
    pub(super) token: u32,
    pub(super) frame: CallFrame32,
    pub(super) api: VersionApi32,
    pub(super) thread: u32,
    pub(super) request: Option<FileReadRequest32>,
    pub(super) input: super::input::VersionInput32,
    pub(super) data: Option<Vec<u8>>,
    pub(super) error: Option<u32>,
}
impl VersionRuntime32 {
    pub(in crate::process) fn terminate(&mut self) {
        self.reply = None;
    }
    pub(in crate::process) fn pending_owner(&self) -> Option<PendingOwner> {
        self.reply.as_ref().map(|r| r.owner)
    }
    pub(in crate::process) fn owns_token(&self, token: u32) -> bool {
        self.reply.as_ref().is_some_and(|r| r.token == token)
    }
    pub(in crate::process) fn take_thread_reply(&mut self) -> Option<VersionThreadReply32> {
        self.reply.take()
    }
    pub(in crate::process) fn restore_thread_reply(&mut self, reply: Option<VersionThreadReply32>) {
        self.reply = reply;
    }
}
