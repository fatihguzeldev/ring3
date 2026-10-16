use super::super::{
    FileRuntime32,
    asynchronous::{AsyncKind32, AsyncProgress32, AsyncStatus32, AsyncTarget32},
};
use crate::{
    files::FileError32,
    memory::AddressSpace,
    windows::{
        files::pipes::{PipeConnection32, PipeIo32},
        synchronization::SynchronizationContext32,
    },
};

impl FileRuntime32 {
    pub(in crate::process::windows::files) fn pipe_operation_ready(&self, index: usize) -> bool {
        let op = &self.asynchronous.operations[index];
        let AsyncTarget32::Pipe(pipe) = op.target else {
            return false;
        };
        if op.cancel {
            return true;
        }
        match &op.kind {
            AsyncKind32::Read => self.pipes.read_ready(pipe),
            AsyncKind32::Write(bytes) => self.pipes.write_ready(pipe, bytes.len()),
            AsyncKind32::Connect => self.pipes.connection(pipe) != Ok(PipeConnection32::Listening),
            AsyncKind32::Transact {
                input, sent: false, ..
            } => self.pipes.write_ready(pipe, input.len()),
            AsyncKind32::Transact { sent: true, .. } => self.pipes.read_ready(pipe),
            AsyncKind32::Lock(_) => true,
        }
    }
    pub(in crate::process::windows::files) fn progress_pipe_operation(
        &mut self,
        memory: &mut AddressSpace,
        sync: Option<&mut SynchronizationContext32>,
        index: usize,
    ) -> Result<AsyncProgress32, FileError32> {
        let op = &self.asynchronous.operations[index];
        let AsyncTarget32::Pipe(pipe) = op.target else {
            return Err(FileError32::InvalidHandle);
        };
        for &(address, version) in &op.mappings {
            if memory.mapping_version_at(address) != Ok(version) {
                return Err(FileError32::NoAccess);
            }
        }
        let mut error = if op.cancel { 995 } else { 0 };
        let mut read = None;
        let mut write = None;
        let mut count = 0;
        if error == 0 {
            let result = match &op.kind {
                AsyncKind32::Read | AsyncKind32::Transact { sent: true, .. } => {
                    match self.pipes.prepare_read(pipe, op.length as usize) {
                        Ok(PipeIo32::Ready(prepared)) => {
                            count = prepared.bytes().len() as u32;
                            if prepared.more() {
                                error = 234;
                            }
                            read = Some(prepared);
                            Ok(())
                        }
                        Ok(PipeIo32::Pending) => return Ok(AsyncProgress32::Idle),
                        Err(error) => Err(error),
                    }
                }
                AsyncKind32::Write(bytes)
                | AsyncKind32::Transact {
                    input: bytes,
                    sent: false,
                    ..
                } => match self.pipes.prepare_write(pipe, bytes) {
                    Ok(PipeIo32::Ready(prepared)) => {
                        count = prepared.length() as u32;
                        write = Some(prepared);
                        Ok(())
                    }
                    Ok(PipeIo32::Pending) => return Ok(AsyncProgress32::Idle),
                    Err(error) => Err(error),
                },
                AsyncKind32::Connect => match self.pipes.connection(pipe) {
                    Ok(PipeConnection32::Connected) => Ok(()),
                    Ok(PipeConnection32::Listening) => return Ok(AsyncProgress32::Idle),
                    Ok(PipeConnection32::Broken) => Err(FileError32::NoData),
                    Err(error) => Err(error),
                },
                AsyncKind32::Lock(_) => Err(FileError32::InvalidParameter),
            };
            if let Err(failure) = result {
                error = failure.win32();
            }
        }
        if let Some(prepared_write) = write.as_ref()
            && matches!(op.kind, AsyncKind32::Transact { sent: false, .. })
            && error == 0
        {
            if prepared_write.length() != op.write_data().unwrap().len() {
                return Err(FileError32::InvalidParameter);
            }
            self.pipes.commit_write(write.take().unwrap());
            let AsyncKind32::Transact { sent, .. } = &mut self.asynchronous.operations[index].kind
            else {
                unreachable!()
            };
            *sent = true;
            return Ok(AsyncProgress32::Idle);
        }
        self.pipes.check_release_many(
            &[(pipe, op.pipe_release_count())],
            u64::from(read.is_some() || write.is_some()),
        )?;
        let completion = self.prepare_io_completion(op, error, count)?;
        let signals = if op.events.is_empty() {
            None
        } else {
            Some(
                sync.as_deref()
                    .ok_or(FileError32::Unsupported)?
                    .prepare_signals_and_release(&op.events)
                    .map_err(super::super::asynchronous::synchronization_error)?,
            )
        };
        let mut completed = [0; 8];
        completed[..4].copy_from_slice(&super::super::asynchronous::status(error).to_le_bytes());
        completed[4..].copy_from_slice(&count.to_le_bytes());
        let count_bytes = count.to_le_bytes();
        let data = read.as_ref().map_or(&[][..], |read| read.bytes());
        let spans = [
            (
                op.overlapped.unwrap_or(crate::memory::GuestAddress(0)),
                if op.overlapped.is_some() {
                    completed.as_slice()
                } else {
                    &[]
                },
            ),
            (
                op.count_pointer.unwrap_or(crate::memory::GuestAddress(0)),
                if op.count_pointer.is_some() {
                    count_bytes.as_slice()
                } else {
                    &[]
                },
            ),
            (op.buffer, data),
        ];
        if spans.iter().any(|(_, bytes)| !bytes.is_empty()) {
            memory
                .ensure_transaction_versions(1)
                .map_err(|_| FileError32::Exhausted)?;
            memory
                .write_spans(&spans)
                .map_err(|_| FileError32::NoAccess)?;
        }
        if let Some(read) = read {
            self.pipes.commit_read(read);
        }
        if let Some(write) = write {
            self.pipes.commit_write(write);
        }
        if let Some(signals) = signals {
            sync.unwrap().commit_signals_and_release(signals);
        }
        if let Some(completion) = completion {
            self.completion.commit_post(completion);
        }
        for _ in 0..op.pipe_release_count() {
            self.pipes
                .release(pipe)
                .expect("completed pipe operation release preflighted");
        }
        let op = &mut self.asynchronous.operations[index];
        op.events.clear();
        if let Some(bytes) = op.take_data() {
            self.asynchronous.retained_bytes -= bytes.len();
        }
        op.status = AsyncStatus32::Complete {
            error,
            bytes: count,
        };
        let result = AsyncProgress32::Completed {
            id: op.id,
            target: op.target.description(),
            error,
            bytes: count,
        };
        if self.pipes.options(pipe).is_err() {
            self.completion.release_target(op.target.description());
        }
        Ok(result)
    }
}
