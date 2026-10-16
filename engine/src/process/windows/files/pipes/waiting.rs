use super::{
    super::{FileCallOutcome32, FileRuntime32, asynchronous::AsyncStatus32},
    PipeWait32,
};
use crate::files::FileError32;
impl FileRuntime32 {
    pub(super) fn wait_named_pipe(
        &mut self,
        name: Vec<u16>,
        timeout: u32,
        now: u64,
    ) -> Result<FileCallOutcome32, FileError32> {
        if self.pipes.available(&name)? {
            self.pipe_wait = None;
            return Ok(FileCallOutcome32::success(1));
        }
        let deadline = match &self.pipe_wait {
            Some(PipeWait32::Name {
                name: prior,
                deadline,
            }) if *prior == name => *deadline,
            _ => {
                let timeout = if timeout == 0 {
                    self.pipes.default_timeout(&name)?
                } else {
                    timeout
                };
                if timeout == u32::MAX {
                    None
                } else {
                    Some(now.saturating_add(u64::from(timeout) * 1_000_000))
                }
            }
        };
        if deadline.is_some_and(|deadline| now >= deadline) {
            self.pipe_wait = None;
            return Ok(FileCallOutcome32::error(0, 121));
        }
        self.pipe_wait = Some(PipeWait32::Name { name, deadline });
        Ok(FileCallOutcome32::Pending)
    }
    pub(in crate::process::windows::files) fn pipe_wait_ready(
        &self,
        wait: &PipeWait32,
        now: u64,
    ) -> bool {
        match wait {
            PipeWait32::Operation(id) => self
                .asynchronous
                .operations
                .iter()
                .position(|op| op.id == *id)
                .is_none_or(|index| {
                    self.asynchronous.operations[index].status != AsyncStatus32::Pending
                        || self.pipe_operation_ready(index)
                }),
            PipeWait32::Name { name, deadline } => {
                deadline.is_some_and(|deadline| now >= deadline)
                    || self.pipes.available(name) != Ok(false)
            }
            PipeWait32::Flush(pipe) => self.pipes.flush_ready(*pipe),
        }
    }
}
impl PipeWait32 {
    pub(in crate::process::windows::files) fn deadline(&self) -> Option<u64> {
        if let Self::Name { deadline, .. } = self {
            *deadline
        } else {
            None
        }
    }
}
