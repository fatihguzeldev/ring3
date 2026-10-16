use super::super::{FileReply32, FileRuntime32};

pub(in crate::process) struct FileThreadReply32 {
    reply: Option<FileReply32>,
    wait: Option<(u64, u64)>,
    interrupted: bool,
    blocking: Option<u64>,
    completion: Option<super::super::completion::CompletionWait32>,
    mapping: super::super::mapping::MappingThreadReply32,
    ordinary: super::super::ordinary::OrdinaryThreadReply32,
}
struct FileReadiness32 {
    wait: Option<(u64, u64)>,
    blocking: Option<u64>,
    completion: Option<super::super::completion::CompletionWait32>,
}
impl FileRuntime32 {
    pub(in crate::process) fn thread_reply_ready(
        &self,
        reply: &FileThreadReply32,
        now: u64,
        runnable: impl Fn(u32) -> bool,
    ) -> bool {
        reply.reply.is_some_and(|pending| {
            self.file_reply_ready(
                pending,
                FileReadiness32 {
                    wait: reply.wait,
                    blocking: reply.blocking,
                    completion: reply.completion,
                },
                now,
                runnable,
            )
        })
    }
    pub(in crate::process) fn active_thread_reply_ready(
        &self,
        now: u64,
        runnable: impl Fn(u32) -> bool,
    ) -> bool {
        self.reply.is_some_and(|pending| {
            self.file_reply_ready(
                pending,
                FileReadiness32 {
                    wait: self.asynchronous.wait,
                    blocking: self.asynchronous.blocking,
                    completion: self.completion_wait,
                },
                now,
                runnable,
            )
        })
    }
    fn file_reply_ready(
        &self,
        pending: FileReply32,
        state: FileReadiness32,
        now: u64,
        runnable: impl Fn(u32) -> bool,
    ) -> bool {
        let FileReadiness32 {
            wait,
            blocking,
            completion,
        } = state;
        if let Some(id) = blocking {
            return self.blocking_lock_ready(id);
        }
        if let Some(wait) = completion {
            return wait.interrupted
                || wait.deadline.is_some_and(|deadline| now >= deadline)
                || self.completion.ready(wait.port, wait.thread, runnable);
        }
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
        let Ok(target) = self.handles.io_target(args[0]) else {
            return true;
        };
        self.asynchronous
            .find_description(target, crate::memory::GuestAddress(args[1]))
            .is_none_or(|index| {
                self.asynchronous.operations[index].status != super::context::AsyncStatus32::Pending
            })
    }
    pub(in crate::process) fn empty_thread_reply(&self) -> FileThreadReply32 {
        FileThreadReply32 {
            reply: None,
            wait: None,
            interrupted: false,
            blocking: None,
            completion: None,
            mapping: self.mappings.empty_thread_reply(),
            ordinary: self.ordinary.empty_thread_reply(),
        }
    }
    pub(in crate::process) fn take_thread_reply(&mut self) -> FileThreadReply32 {
        FileThreadReply32 {
            reply: self.reply.take(),
            wait: self.asynchronous.wait.take(),
            interrupted: std::mem::take(&mut self.asynchronous.interrupted),
            blocking: self.asynchronous.blocking.take(),
            completion: self.completion_wait.take(),
            mapping: self.mappings.take_thread_reply(),
            ordinary: self.ordinary.take_thread_reply(),
        }
    }
    pub(in crate::process) fn restore_thread_reply(&mut self, reply: FileThreadReply32) {
        assert!(self.reply.is_none());
        self.reply = reply.reply;
        assert!(self.asynchronous.wait.is_none());
        self.asynchronous.wait = reply.wait;
        self.asynchronous.interrupted = reply.interrupted;
        assert!(self.asynchronous.blocking.is_none());
        self.asynchronous.blocking = reply.blocking;
        assert!(self.completion_wait.is_none());
        self.completion_wait = reply.completion;
        self.mappings.restore_thread_reply(reply.mapping);
        self.ordinary.restore_thread_reply(reply.ordinary);
    }
}

impl FileThreadReply32 {
    pub(in crate::process) fn alertable(&self) -> bool {
        self.reply.is_some_and(|reply| {
            (reply.id == crate::windows::files::FileApi32::GetOverlappedResultEx.id()
                && reply.frame.arguments()[4] != 0
                && !self.interrupted)
                || self
                    .completion
                    .is_some_and(|wait| wait.alertable && !wait.interrupted)
        })
    }
    pub(in crate::process) fn deadline(&self) -> Option<u64> {
        self.wait
            .map(|(_, deadline)| deadline)
            .into_iter()
            .chain(self.completion.and_then(|wait| wait.deadline))
            .min()
    }
    pub(in crate::process::windows::files) fn ordinary_reply(
        &self,
    ) -> &super::super::ordinary::OrdinaryThreadReply32 {
        &self.ordinary
    }
}
