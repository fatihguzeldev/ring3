use super::context::*;
use crate::{
    files::{FileError32, FileIo32},
    memory::AddressSpace,
    process::windows::files::FileRuntime32,
    windows::synchronization::SynchronizationContext32,
};

impl FileRuntime32 {
    pub(in crate::process::windows::files) fn progress_io(
        &mut self,
        memory: &mut AddressSpace,
        sync: Option<&mut SynchronizationContext32>,
    ) -> Result<AsyncProgress32, FileError32> {
        let mut lock_receipt = None;
        let mut index = None;
        let priority = self.asynchronous.blocking.and_then(|id| {
            self.asynchronous
                .operations
                .iter()
                .position(|op| op.id == id)
        });
        for candidate in priority
            .into_iter()
            .chain((0..self.asynchronous.operations.len()).filter(|index| Some(*index) != priority))
        {
            let operation = &self.asynchronous.operations[candidate];
            if operation.status != AsyncStatus32::Pending {
                continue;
            }
            if let Some(lock) = operation.lock().filter(|_| !operation.cancel) {
                match self.prepare_file_lock(
                    operation
                        .target
                        .open()
                        .ok_or(FileError32::InvalidParameter)?,
                    operation.offset,
                    lock.length,
                    lock.exclusive,
                ) {
                    Ok(prepared) => {
                        lock_receipt = Some(prepared);
                        index = Some(candidate);
                        break;
                    }
                    Err(FileError32::LockViolation) => continue,
                    Err(_) => {
                        index = Some(candidate);
                        break;
                    }
                }
            } else {
                index = Some(candidate);
                break;
            }
        }
        let Some(index) = index else {
            return Ok(AsyncProgress32::Idle);
        };
        let operation = &self.asynchronous.operations[index];
        let (open, file) = operation.target.file().ok_or(FileError32::Unsupported)?;
        let overlapped = operation.overlapped.ok_or(FileError32::InvalidParameter)?;
        for &(address, version) in &operation.mappings {
            if memory.mapping_version_at(address) != Ok(version) {
                return Err(FileError32::NoAccess);
            }
        }
        let offset = if operation.offset == u64::MAX {
            self.vfs.info(file)?.size
        } else {
            operation.offset
        };
        let mut error = if operation.cancel { 995 } else { 0 };
        let mut read = None;
        let mut write = None;
        let mut count = 0;
        if error == 0
            && lock_receipt.is_none()
            && let Some(lock) = operation.lock()
        {
            error = self
                .prepare_file_lock(open, offset, lock.length, lock.exclusive)
                .err()
                .unwrap()
                .win32();
        }
        if error == 0
            && operation.lock().is_none()
            && let Err(failure) = super::positioned::check_alignment(
                self.vfs.open_options(open)?.no_buffering,
                operation.buffer.0,
                operation.length,
                offset,
            )
        {
            error = failure.win32();
        }
        if error == 0
            && operation.lock().is_none()
            && let Err(failure) = self.check_file_lock(
                open,
                offset,
                u64::from(operation.length),
                operation.write_data().is_some(),
            )
        {
            error = failure.win32();
        }
        if error == 0 && operation.lock().is_none() {
            if let Some(data) = operation.write_data() {
                match self
                    .vfs
                    .prepare_write_at_for(operation.id, file, offset, data)
                {
                    Ok(FileIo32::Ready(prepared)) => {
                        prepared.validate(&self.vfs)?;
                        count = prepared.length() as u32;
                        write = Some(prepared);
                    }
                    Ok(FileIo32::NeedBytes(request)) => {
                        return Ok(AsyncProgress32::NeedBytes(request));
                    }
                    Err(failure) => error = failure.win32(),
                }
            } else {
                match self.vfs.prepare_read_at_for(
                    operation.id,
                    file,
                    offset,
                    operation.length as usize,
                ) {
                    Ok(FileIo32::Ready(prepared)) => {
                        prepared.validate(&self.vfs)?;
                        count = prepared.bytes().len() as u32;
                        if count == 0 && operation.length != 0 {
                            error = 38;
                        } else {
                            read = Some(prepared)
                        }
                    }
                    Ok(FileIo32::NeedBytes(request)) => {
                        return Ok(AsyncProgress32::NeedBytes(request));
                    }
                    Err(failure) => error = failure.win32(),
                }
            }
        }
        self.vfs
            .check_close_open_after_mutations(open, u64::from(write.is_some()))?;
        let sync = sync.ok_or(FileError32::Unsupported)?;
        let completion = self.prepare_io_completion(operation, error, count)?;
        let signals = sync
            .prepare_signals_and_release(&operation.events)
            .map_err(super::synchronization_error)?;
        let mut completed = [0u8; 8];
        completed[..4].copy_from_slice(&status(error).to_le_bytes());
        completed[4..].copy_from_slice(&count.to_le_bytes());
        memory
            .ensure_transaction_versions(1)
            .map_err(|_| FileError32::Exhausted)?;
        let data = read.as_ref().map_or(&[][..], |read| read.bytes());
        memory
            .write_spans(&[(overlapped, &completed), (operation.buffer, data)])
            .map_err(|_| FileError32::NoAccess)?;
        if let Some(write) = write {
            self.vfs.commit_write(write);
        }
        if let Some(read) = read {
            self.vfs.commit_read(read);
        }
        self.vfs.cancel_operation_io(operation.id);
        sync.commit_signals_and_release(signals);
        if let Some(completion) = completion {
            self.completion.commit_post(completion);
        }
        if let Some(prepared) = lock_receipt.filter(|_| error == 0) {
            self.commit_file_lock(prepared);
        }
        self.vfs
            .close_open(open)
            .expect("completion source release preflighted before RAM/VFS publication");
        if self.vfs.open_info(open).is_err() {
            self.release_file_locks(open);
        }
        self.retire_completion_file(open);

        let operation = &mut self.asynchronous.operations[index];
        operation.events.clear();
        if let Some(data) = operation.take_data() {
            self.asynchronous.retained_bytes -= data.len();
        }
        operation.status = AsyncStatus32::Complete {
            error,
            bytes: count,
        };
        Ok(AsyncProgress32::Completed {
            id: operation.id,
            target: operation.target.description(),
            error,
            bytes: count,
        })
    }
}
pub(in crate::process::windows::files) fn status(error: u32) -> u32 {
    match error {
        0 => 0,
        2 => 0xc000_0034,
        3 => 0xc000_003a,
        5 => 0xc000_0022,
        6 => 0xc000_0008,
        8 => 0xc000_0017,
        32 => 0xc000_0043,
        33 => 0xc000_0054,
        38 => 0xc000_0011,
        50 => 0xc000_00bb,
        87 => 0xc000_000d,
        109 => 0xc000_014b,
        112 => 0xc000_007f,
        230 => 0xc000_00ad,
        231 => 0xc000_00ae,
        232 => 0xc000_00b1,
        233 => 0xc000_00b0,
        234 => 0x8000_0005,
        536 => 0xc000_00b3,
        995 => 0xc000_0120,
        998 => 0xc000_0005,
        _ => 0xc000_0001,
    }
}
pub(super) fn status_error(status: u32) -> u32 {
    match status {
        0 => 0,
        0x103 => 996,
        0xc000_0034 => 2,
        0xc000_003a => 3,
        0xc000_0022 => 5,
        0xc000_0008 => 6,
        0xc000_0017 => 8,
        0xc000_0043 => 32,
        0xc000_0054 => 33,
        0xc000_0011 => 38,
        0xc000_00bb => 50,
        0xc000_000d => 87,
        0xc000_014b => 109,
        0xc000_007f => 112,
        0xc000_00ad => 230,
        0xc000_00ae => 231,
        0xc000_00b1 => 232,
        0xc000_00b0 => 233,
        0x8000_0005 => 234,
        0xc000_00b3 => 536,
        0xc000_0120 => 995,
        0xc000_0005 => 998,
        _ => 31,
    }
}
