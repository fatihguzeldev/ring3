use super::{acceptance::capture_mappings, context::*};
use crate::{
    files::{FileAccess32, FileError32},
    memory::{AddressSpace, GuestAddress},
    process::windows::files::{
        FileRuntime32,
        ordinary::{FileCallOutcome32, memory as guest},
    },
    windows::{files::FileApi32, synchronization::SynchronizationContext32},
};

struct PendingLock32 {
    thread: u32,
    open: crate::files::OpenFileId,
    file_event: u32,
    overlap: u32,
    offset: u64,
    length: u64,
    exclusive: bool,
    event: u32,
    skip_port: bool,
}
impl FileRuntime32 {
    pub(in crate::process::windows::files) fn execute_lock_ex(
        &mut self,
        memory: &mut AddressSpace,
        sync: Option<&mut SynchronizationContext32>,
        thread: u32,
        api: FileApi32,
        args: &[u32],
    ) -> Result<FileCallOutcome32, FileError32> {
        let lock = api == FileApi32::LockFileEx;
        if lock && let Some(id) = self.asynchronous.blocking {
            self.progress_io(memory, sync)?;
            let operation = self
                .asynchronous
                .operations
                .iter()
                .find(|op| op.id == id)
                .ok_or(FileError32::InvalidHandle)?;
            return Ok(match operation.status {
                AsyncStatus32::Pending => FileCallOutcome32::Pending,
                AsyncStatus32::Complete { error: 0, .. } => {
                    self.asynchronous.blocking = None;
                    FileCallOutcome32::success(1)
                }
                AsyncStatus32::Complete { error, .. } => {
                    self.asynchronous.blocking = None;
                    FileCallOutcome32::error(0, error)
                }
            });
        }

        let open = self.handles.file(args[0])?;
        if self.handles.access(args[0])? & (FileAccess32::READ.0 | FileAccess32::WRITE.0) == 0 {
            return Err(FileError32::AccessDenied);
        }
        if (lock && args[1] & !3 != 0) || args[if lock { 2 } else { 1 }] != 0 {
            return Err(FileError32::InvalidParameter);
        }
        let overlap = args[if lock { 5 } else { 4 }];
        let raw = guest::read::<20>(memory, overlap)?;
        guest::output(memory, overlap, 8)?;
        let offset = u64::from(u32::from_le_bytes(raw[8..12].try_into().unwrap()))
            | u64::from(u32::from_le_bytes(raw[12..16].try_into().unwrap())) << 32;
        let length = u64::from(args[if lock { 3 } else { 2 }])
            | u64::from(args[if lock { 4 } else { 3 }]) << 32;
        if length == 0 || offset.checked_add(length).is_none() {
            return Err(FileError32::InvalidParameter);
        }
        let event = u32::from_le_bytes(raw[16..20].try_into().unwrap()) & !1;
        let skip_port = u32::from_le_bytes(raw[16..20].try_into().unwrap()) & 1 != 0;
        let target = crate::windows::files::completion::CompletionTarget32::File(open);
        let modes = self.completion.modes(target);
        let sync = sync.ok_or(FileError32::Unsupported)?;
        let mut lock_receipt = None;
        if lock {
            match self.prepare_file_lock(open, offset, length, args[1] & 2 != 0) {
                Ok(prepared) => lock_receipt = Some(prepared),
                Err(FileError32::LockViolation) if args[1] & 1 == 0 => {
                    let mut pending =
                        |files: &mut Self, sync: &mut SynchronizationContext32, file_event| {
                            files.accept_pending_lock(
                                memory,
                                sync,
                                PendingLock32 {
                                    thread,
                                    open,
                                    file_event,
                                    overlap,
                                    offset,
                                    length,
                                    exclusive: args[1] & 2 != 0,
                                    event,
                                    skip_port,
                                },
                            )
                        };
                    let result = if modes & 2 == 0 {
                        self.with_file_event(open, sync, pending)?
                    } else {
                        pending(self, sync, 0)?
                    };
                    if !self.vfs.open_options(open)?.overlapped {
                        self.asynchronous.blocking = Some(self.asynchronous.next - 1);
                        return Ok(FileCallOutcome32::Pending);
                    }
                    return Ok(result);
                }
                Err(error) => return Err(error),
            }
        } else {
            self.check_unlock_file_range(open, offset, length)?;
        }
        let complete = |files: &mut Self, sync: &mut SynchronizationContext32, file_event| {
            let mut events = [0; 2];
            let mut count = 0;
            for handle in [file_event, event]
                .into_iter()
                .filter(|handle| *handle != 0)
            {
                events[count] = handle;
                count += 1;
            }
            let events = &events[..count];
            let completion =
                if lock && !skip_port && modes & 1 == 0 && files.vfs.open_options(open)?.overlapped
                {
                    files
                        .completion
                        .association(target)
                        .map(|(port, key, _)| {
                            files.completion.prepare_post(
                                port,
                                crate::windows::files::completion::CompletionPacket32 {
                                    key,
                                    overlapped: overlap,
                                    bytes: 0,
                                    status: 0,
                                    error: 0,
                                },
                            )
                        })
                        .transpose()?
                } else {
                    None
                };
            let signals = sync
                .prepare_signal_handles(events)
                .map_err(super::synchronization_error)?;
            memory
                .ensure_transaction_versions(1)
                .map_err(|_| FileError32::Exhausted)?;
            memory
                .write(GuestAddress(overlap), &[0; 8])
                .map_err(|_| FileError32::NoAccess)?;
            if let Some(prepared) = lock_receipt {
                files.commit_file_lock(prepared);
            } else {
                files
                    .unlock_file_range(open, offset, length)
                    .expect("unlock range was checked before completion publication");
            }
            sync.commit_signals(signals);
            if let Some(completion) = completion {
                files.completion.commit_post(completion);
            }
            Ok(FileCallOutcome32::success(1))
        };
        if modes & 2 == 0 {
            self.with_file_event(open, sync, complete)
        } else {
            complete(self, sync, 0)
        }
    }
    fn accept_pending_lock(
        &mut self,
        memory: &mut AddressSpace,
        sync: &mut SynchronizationContext32,
        request: PendingLock32,
    ) -> Result<FileCallOutcome32, FileError32> {
        let PendingLock32 {
            thread,
            open,
            file_event,
            overlap,
            offset,
            length,
            exclusive,
            event,
            skip_port,
        } = request;
        let overlapped = GuestAddress(overlap);
        if self
            .asynchronous
            .find(open, overlapped)
            .is_some_and(|index| {
                self.asynchronous.operations[index].status == AsyncStatus32::Pending
            })
        {
            return Err(FileError32::Busy);
        }
        let old = self.asynchronous.find(open, overlapped).or_else(|| {
            (self.asynchronous.operations.len() >= self.asynchronous.max_operations)
                .then(|| {
                    self.asynchronous.operations.iter().position(|operation| {
                        matches!(operation.status, AsyncStatus32::Complete { .. })
                    })
                })
                .flatten()
        });
        if old.is_none() && self.asynchronous.operations.len() >= self.asynchronous.max_operations {
            return Err(FileError32::Capacity);
        }
        let next = self
            .asynchronous
            .next
            .checked_add(1)
            .ok_or(FileError32::Exhausted)?;
        self.vfs.check_retain_open(open)?;
        let file = self.vfs.file_for_open(open)?;
        let mut mappings = Vec::new();
        capture_mappings(memory, overlapped, 8, &mut mappings)?;
        let mut handles = [0; 2];
        let mut count = 0;
        for handle in [file_event, event]
            .into_iter()
            .filter(|handle| *handle != 0)
        {
            handles[count] = handle;
            count += 1;
        }
        let handles = &handles[..count];
        let release = old.map_or(&[][..], |index| {
            self.asynchronous.operations[index].events.as_slice()
        });
        let events = sync
            .prepare_io_events(handles, release, handles)
            .map_err(super::synchronization_error)?;
        let mut pending = [0; 8];
        pending[..4].copy_from_slice(&0x103u32.to_le_bytes());
        memory
            .write(overlapped, &pending)
            .map_err(|_| FileError32::NoAccess)?;
        let events = sync.commit_event_bindings(events);
        self.vfs
            .retain_open(open)
            .expect("pending lock source lifetime was checked");
        if let Some(index) = old {
            self.asynchronous.operations.remove(index);
        }
        let id = self.asynchronous.next;
        self.asynchronous.next = next;
        self.asynchronous.operations.push(AsyncOperation32 {
            id,
            target: AsyncTarget32::File { open, file },
            thread,
            buffer: GuestAddress(0),
            length: 0,
            overlapped: Some(overlapped),
            count_pointer: None,
            offset,
            kind: AsyncKind32::Lock(AsyncLock32 { length, exclusive }),
            events,
            status: AsyncStatus32::Pending,
            cancel: false,
            mappings,
            skip_port,
        });
        Ok(FileCallOutcome32::error(0, 997))
    }
}
