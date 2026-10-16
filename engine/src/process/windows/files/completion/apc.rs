use crate::{process::EngineInstance, windows::files::FileApi32};

impl EngineInstance {
    pub(in crate::process) fn windows_file_wait_alertable(&self) -> bool {
        self.files.as_ref().is_some_and(|files| {
            files.reply.is_some_and(|reply| {
                (reply.id == FileApi32::GetOverlappedResultEx.id()
                    && reply.frame.arguments()[4] != 0
                    && !files.asynchronous.interrupted)
                    || files
                        .completion_wait
                        .is_some_and(|wait| wait.alertable && !wait.interrupted)
            })
        })
    }
    pub(in crate::process) fn commit_windows_file_wait_apc(&mut self) {
        if !self.windows_file_wait_alertable() {
            return;
        }
        let files = self.files.as_mut().unwrap();
        if let Some(wait) = files.completion_wait.as_mut() {
            files.completion.cancel_wait_for(wait.thread, wait.port);
            wait.interrupted = true;
        }
        if files
            .reply
            .is_some_and(|reply| reply.id == FileApi32::GetOverlappedResultEx.id())
        {
            files.asynchronous.interrupted = true;
        }
    }
    pub(in crate::process) fn active_windows_file_deadline(&self) -> Option<u64> {
        let files = self.files.as_ref()?;
        files
            .asynchronous
            .wait
            .map(|(_, deadline)| deadline)
            .into_iter()
            .chain(files.completion_wait.and_then(|wait| wait.deadline))
            .min()
    }
}
