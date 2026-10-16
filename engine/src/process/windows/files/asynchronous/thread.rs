use super::super::{FileReply32, FileRuntime32};

pub(in crate::process) struct FileThreadReply32 {
    reply: Option<FileReply32>,
    wait: Option<(u64, u64)>,
    mapping: super::super::mapping::MappingThreadReply32,
    ordinary: super::super::ordinary::OrdinaryThreadReply32,
}
impl FileRuntime32 {
    pub(in crate::process) fn thread_reply_ready(
        &self,
        reply: &FileThreadReply32,
        now: u64,
    ) -> bool {
        reply
            .reply
            .is_some_and(|pending| self.file_reply_ready(pending, reply.wait, now))
    }
    pub(in crate::process) fn active_thread_reply_ready(&self, now: u64) -> bool {
        self.reply
            .is_some_and(|pending| self.file_reply_ready(pending, self.asynchronous.wait, now))
    }
    fn file_reply_ready(&self, pending: FileReply32, wait: Option<(u64, u64)>, now: u64) -> bool {
        if let Some(request) = pending.request {
            return self.vfs.request_ready(request);
        }
        if wait.is_some_and(|(_, deadline)| now >= deadline) {
            return true;
        }
        let Some(api) = crate::windows::files::FileApi32::from_id(pending.id) else {
            return false;
        };
        if !matches!(
            api,
            crate::windows::files::FileApi32::GetOverlappedResult
                | crate::windows::files::FileApi32::GetOverlappedResultEx
        ) {
            return false;
        }
        let args = pending.frame.arguments();
        let Ok(open) = self.handles.file(args[0]) else {
            return true;
        };
        self.asynchronous
            .find(open, crate::memory::GuestAddress(args[1]))
            .is_none_or(|index| {
                self.asynchronous.operations[index].status != super::context::AsyncStatus32::Pending
            })
    }
    pub(in crate::process) fn empty_thread_reply(&self) -> FileThreadReply32 {
        FileThreadReply32 {
            reply: None,
            wait: None,
            mapping: self.mappings.empty_thread_reply(),
            ordinary: self.ordinary.empty_thread_reply(),
        }
    }
    pub(in crate::process) fn take_thread_reply(&mut self) -> FileThreadReply32 {
        FileThreadReply32 {
            reply: self.reply.take(),
            wait: self.asynchronous.wait.take(),
            mapping: self.mappings.take_thread_reply(),
            ordinary: self.ordinary.take_thread_reply(),
        }
    }
    pub(in crate::process) fn restore_thread_reply(&mut self, reply: FileThreadReply32) {
        assert!(self.reply.is_none());
        self.reply = reply.reply;
        assert!(self.asynchronous.wait.is_none());
        self.asynchronous.wait = reply.wait;
        self.mappings.restore_thread_reply(reply.mapping);
        self.ordinary.restore_thread_reply(reply.ordinary);
    }
}

impl FileThreadReply32 {
    pub(in crate::process) fn deadline(&self) -> Option<u64> {
        self.wait.map(|(_, deadline)| deadline)
    }
    pub(in crate::process::windows::files) fn ordinary_reply(
        &self,
    ) -> &super::super::ordinary::OrdinaryThreadReply32 {
        &self.ordinary
    }
}
