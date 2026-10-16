use super::{
    super::{FileCallOutcome32, FileRuntime32, ordinary::memory as guest},
    CompletionWait32,
};
use crate::{
    files::FileError32,
    memory::{AddressSpace, GuestAddress},
    process::windows::threads::ThreadRuntime32,
    windows::files::FileApi32 as Api,
};

impl FileRuntime32 {
    pub(super) fn dequeue_completion(
        &mut self,
        memory: &mut AddressSpace,
        threads: &ThreadRuntime32,
        now: u64,
        thread: u32,
        api: Api,
        args: &[u32],
    ) -> Result<FileCallOutcome32, FileError32> {
        let extended = api == Api::GetQueuedCompletionStatusEx;
        let (count, timeout, alertable) = if extended {
            if args[2] == 0 {
                return Err(FileError32::InvalidParameter);
            }
            guest::output(
                memory,
                args[1],
                (args[2] as usize)
                    .checked_mul(16)
                    .ok_or(FileError32::InvalidParameter)?,
            )?;
            guest::output(memory, args[3], 4)?;
            (args[2] as usize, args[4], args[5] != 0)
        } else {
            for &pointer in &args[1..4] {
                guest::output(memory, pointer, 4)?;
            }
            (1, args[4], false)
        };
        let wait = self.completion_wait.filter(|wait| wait.thread == thread);
        let port = wait.map_or_else(|| self.completion_port(args[0]), |wait| Ok(wait.port))?;
        let deadline = wait.map_or_else(
            || {
                if timeout == u32::MAX {
                    None
                } else {
                    Some(now.saturating_add(u64::from(timeout) * 1_000_000))
                }
            },
            |wait| wait.deadline,
        );
        let interrupted = wait.is_some_and(|wait| wait.interrupted);
        if interrupted || self.completion.is_closed(port) {
            return self.finish_empty_completion(
                memory,
                thread,
                port,
                args,
                if interrupted { 192 } else { 735 },
            );
        }
        let take = self
            .completion
            .prepare_take(port, thread, count, |tid| threads.is_runnable(tid))?;
        if let Some(take) = take {
            if extended {
                let mut bytes = Vec::new();
                bytes
                    .try_reserve_exact(take.packets().len() * 16)
                    .map_err(|_| FileError32::Allocation)?;
                for packet in take.packets() {
                    for value in [packet.key, packet.overlapped, packet.status, packet.bytes] {
                        bytes.extend_from_slice(&value.to_le_bytes());
                    }
                }
                let removed = (take.packets().len() as u32).to_le_bytes();
                memory
                    .write_spans(&[
                        (GuestAddress(args[1]), &bytes),
                        (GuestAddress(args[3]), &removed),
                    ])
                    .map_err(|_| FileError32::NoAccess)?;
                self.completion.commit_take(take);
                self.completion_wait = None;
                Ok(FileCallOutcome32::success(1))
            } else {
                let packet = take.packets()[0];
                let bytes = packet.bytes.to_le_bytes();
                let key = packet.key.to_le_bytes();
                let overlap = packet.overlapped.to_le_bytes();
                memory
                    .write_spans(&[
                        (GuestAddress(args[1]), &bytes),
                        (GuestAddress(args[2]), &key),
                        (GuestAddress(args[3]), &overlap),
                    ])
                    .map_err(|_| FileError32::NoAccess)?;
                self.completion.commit_take(take);
                self.completion_wait = None;
                Ok(if packet.error == 0 {
                    FileCallOutcome32::success(1)
                } else {
                    FileCallOutcome32::error(0, packet.error)
                })
            }
        } else if deadline.is_some_and(|deadline| now >= deadline) {
            self.finish_empty_completion(memory, thread, port, args, 258)
        } else {
            if wait.is_none() {
                let prepared = self.completion.prepare_wait(port, thread)?;
                self.completion.commit_wait(prepared);
            }
            self.completion_wait = Some(CompletionWait32 {
                port,
                thread,
                deadline,
                alertable,
                interrupted: false,
            });
            Ok(FileCallOutcome32::Pending)
        }
    }
    fn finish_empty_completion(
        &mut self,
        memory: &mut AddressSpace,
        thread: u32,
        port: crate::windows::files::completion::CompletionPortId32,
        args: &[u32],
        error: u32,
    ) -> Result<FileCallOutcome32, FileError32> {
        memory
            .write(GuestAddress(args[3]), &0u32.to_le_bytes())
            .map_err(|_| FileError32::NoAccess)?;
        self.completion.cancel_wait_for(thread, port);
        self.completion_wait = None;
        Ok(FileCallOutcome32::error(0, error))
    }
}
