use super::{completion::status_error, context::*};
use crate::{
    files::FileError32,
    memory::{AddressSpace, GuestAddress},
    process::windows::files::{
        FileRuntime32,
        ordinary::{FileCallOutcome32, memory as guest},
    },
    windows::{files::FileApi32, synchronization::SynchronizationContext32},
};

impl FileRuntime32 {
    pub(in crate::process::windows::files) fn execute_async(
        &mut self,
        memory: &mut AddressSpace,
        sync: Option<&mut SynchronizationContext32>,
        thread: u32,
        now_ns: u64,
        api: FileApi32,
        args: &[u32],
    ) -> Result<FileCallOutcome32, FileError32> {
        if matches!(api, FileApi32::LockFileEx | FileApi32::UnlockFileEx) {
            return self.execute_lock_ex(memory, sync, thread, api, args);
        }
        let target = self.handles.io_target(args[0])?;
        if api == FileApi32::GetOverlappedResultEx && self.asynchronous.interrupted {
            self.asynchronous.interrupted = false;
            self.asynchronous.wait = None;
            return Ok(FileCallOutcome32::error(0, 192));
        }
        if matches!(api, FileApi32::CancelIo | FileApi32::CancelIoEx) {
            let mut found = false;
            for operation in &mut self.asynchronous.operations {
                if operation.target.description() == target
                    && operation.status == AsyncStatus32::Pending
                    && (if api == FileApi32::CancelIo {
                        operation.thread == thread
                    } else {
                        args[1] == 0 || operation.overlapped == Some(GuestAddress(args[1]))
                    })
                {
                    operation.cancel = true;
                    found = true;
                }
            }
            return Ok(if api == FileApi32::CancelIoEx && !found {
                FileCallOutcome32::error(0, 1168)
            } else {
                FileCallOutcome32::success(1)
            });
        }
        if !matches!(
            api,
            FileApi32::GetOverlappedResult | FileApi32::GetOverlappedResultEx
        ) {
            return Err(FileError32::InvalidParameter);
        }
        guest::output(memory, args[2], 4)?;
        let overlap = GuestAddress(args[1]);
        let raw = guest::read::<8>(memory, args[1])?;
        let cached = self
            .asynchronous
            .find_description(target, overlap)
            .map(|index| &self.asynchronous.operations[index]);
        let (pending, error, count, id) = match cached {
            Some(operation) => match operation.status {
                AsyncStatus32::Pending => (true, 996, 0, operation.id),
                AsyncStatus32::Complete { error, bytes } => (false, error, bytes, operation.id),
            },
            None => {
                let status = u32::from_le_bytes(raw[..4].try_into().unwrap());
                (
                    status == 0x103,
                    status_error(status),
                    u32::from_le_bytes(raw[4..].try_into().unwrap()),
                    u64::from(args[1]),
                )
            }
        };
        if pending {
            if api == FileApi32::GetOverlappedResult {
                return Ok(if args[3] == 0 {
                    FileCallOutcome32::error(0, 996)
                } else {
                    FileCallOutcome32::Pending
                });
            }
            if args[3] == 0 {
                return Ok(FileCallOutcome32::error(0, 258));
            }
            if args[3] != u32::MAX {
                let deadline = match self.asynchronous.wait {
                    Some((operation, deadline)) if operation == id => deadline,
                    _ => now_ns.saturating_add(u64::from(args[3]) * 1_000_000),
                };
                if now_ns >= deadline {
                    self.asynchronous.wait = None;
                    return Ok(FileCallOutcome32::error(0, 258));
                }
                self.asynchronous.wait = Some((id, deadline));
            }
            return Ok(FileCallOutcome32::Pending);
        }
        self.asynchronous.wait = None;
        memory
            .write(GuestAddress(args[2]), &count.to_le_bytes())
            .map_err(|_| FileError32::NoAccess)?;
        Ok(if error == 0 {
            FileCallOutcome32::success(1)
        } else {
            FileCallOutcome32::error(0, error)
        })
    }
}
