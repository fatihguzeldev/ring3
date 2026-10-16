use super::{
    super::{FileRuntime32, asynchronous::AsyncStatus32},
    PipeWait32,
};
use crate::{files::FileError32, windows::synchronization::SynchronizationContext32};
impl FileRuntime32 {
    pub(in crate::process::windows::files) fn cancel_pipe_wait(
        &mut self,
        sync: Option<&mut SynchronizationContext32>,
    ) -> Result<(), FileError32> {
        let Some(PipeWait32::Operation(id)) = self.pipe_wait else {
            self.pipe_wait = None;
            return Ok(());
        };
        let index = self
            .asynchronous
            .operations
            .iter()
            .position(|op| op.id == id)
            .ok_or(FileError32::InvalidHandle)?;
        let operation = &self.asynchronous.operations[index];
        if operation.status != AsyncStatus32::Pending {
            self.pipe_wait = None;
            return Ok(());
        }
        let super::super::asynchronous::AsyncTarget32::Pipe(pipe) = operation.target else {
            return Err(FileError32::InvalidHandle);
        };
        let count = operation.pipe_release_count();
        self.pipes.check_release_many(&[(pipe, count)], 0)?;
        let completion = self.prepare_io_completion(operation, 995, 0)?;
        let signals = if operation.events.is_empty() {
            None
        } else {
            Some(
                sync.as_deref()
                    .ok_or(FileError32::Unsupported)?
                    .prepare_signals_and_release(&operation.events)
                    .map_err(super::super::asynchronous::synchronization_error)?,
            )
        };
        if let Some(signals) = signals {
            sync.unwrap().commit_signals_and_release(signals);
        }
        if let Some(completion) = completion {
            self.completion.commit_post(completion);
        }
        for _ in 0..count {
            self.pipes
                .release(pipe)
                .expect("canceled pipe source release preflighted");
        }
        let operation = &mut self.asynchronous.operations[index];
        operation.events.clear();
        if let Some(bytes) = operation.take_data() {
            self.asynchronous.retained_bytes -= bytes.len();
        }
        operation.status = AsyncStatus32::Complete {
            error: 995,
            bytes: 0,
        };
        if self.pipes.options(pipe).is_err() {
            self.completion
                .release_target(operation.target.description());
        }
        self.pipe_wait = None;
        Ok(())
    }
}
