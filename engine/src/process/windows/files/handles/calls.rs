use super::KernelFileObject32;
use crate::{
    files::FileError32,
    memory::{Access, AddressSpace, GuestAddress},
    process::windows::files::{FileCallOutcome32, FileRuntime32, ordinary},
    windows::synchronization::SynchronizationContext32,
};

impl FileRuntime32 {
    pub(in crate::process::windows::files) fn close_kernel_handle(
        &mut self,
        memory: &mut AddressSpace,
        sync: Option<&mut SynchronizationContext32>,
        handle: u32,
    ) -> Result<FileCallOutcome32, FileError32> {
        match self.handles.object(handle)? {
            KernelFileObject32::File(open) => {
                let last = self.handles.file_count(open) == 1;
                let event = if last {
                    self.events.iter().position(|(id, _)| {
                        *id == crate::windows::files::completion::CompletionTarget32::File(open)
                    })
                } else {
                    None
                };
                let prepared = if let Some(index) = event {
                    Some(
                        sync.as_deref()
                            .ok_or(FileError32::Unsupported)?
                            .prepare_duplicate(self.events[index].1, 0, false, true, true, false)
                            .map_err(|_| FileError32::Exhausted)?,
                    )
                } else {
                    None
                };
                self.vfs.close_open(open)?;
                if last {
                    self.asynchronous.cancel_open(open);
                    self.release_file_locks(open);
                }
                self.retire_completion_file(open);
                self.handles.remove(handle)?;
                if let Some(prepared) = prepared {
                    sync.unwrap().commit_duplicate(prepared);
                    self.events.remove(event.unwrap());
                }
            }
            KernelFileObject32::Pipe(pipe) => {
                let target = crate::windows::files::completion::CompletionTarget32::Pipe(pipe);
                let last = self.handles.io_count(target) == 1;
                let event = if last {
                    self.events.iter().position(|(id, _)| *id == target)
                } else {
                    None
                };
                let close = event
                    .map(|index| {
                        sync.as_deref()
                            .ok_or(FileError32::Unsupported)?
                            .prepare_close_handles(&[self.events[index].1])
                            .map_err(|_| FileError32::Exhausted)
                    })
                    .transpose()?;
                self.pipes.release(pipe)?;
                if last {
                    self.asynchronous.cancel_description(target);
                }
                self.handles.remove(handle)?;
                if let Some(close) = close {
                    sync.unwrap().commit_close_handles(close);
                    self.events.remove(event.unwrap());
                }
                if self.pipes.options(pipe).is_err() {
                    self.completion.release_target(target);
                }
            }
            KernelFileObject32::Null(id) => {
                let target = crate::windows::files::completion::CompletionTarget32::Null(id);
                let event = if self.handles.io_count(target) == 1 {
                    self.events
                        .iter()
                        .position(|(description, _)| *description == target)
                } else {
                    None
                };
                let close = event
                    .map(|index| {
                        sync.as_deref()
                            .ok_or(FileError32::Unsupported)?
                            .prepare_close_handles(&[self.events[index].1])
                            .map_err(super::super::asynchronous::synchronization_error)
                    })
                    .transpose()?;
                self.null_devices.release(id)?;
                self.handles.remove(handle)?;
                if let Some(close) = close {
                    sync.unwrap().commit_close_handles(close);
                    self.events.remove(event.unwrap());
                }
                if self.null_devices.overlapped(id).is_err() {
                    self.completion.release_target(target);
                }
            }
            KernelFileObject32::Mapping(id) => {
                let result = self.close_mapping(memory, id)?;
                if !matches!(result, FileCallOutcome32::Return { value: 1, .. }) {
                    return Ok(result);
                }
                self.handles.remove(handle)?;
            }
            KernelFileObject32::Find(_) => return Err(FileError32::InvalidHandle),
            KernelFileObject32::CompletionPort(port) => {
                self.completion.release(port)?;
                self.handles.remove(handle)?;
            }
        }
        Ok(FileCallOutcome32::success(1))
    }
    pub(in crate::process::windows::files) fn duplicate_kernel_handle(
        &mut self,
        memory: &mut AddressSpace,
        mut sync: Option<&mut SynchronizationContext32>,
        args: &[u32],
        process_error: Option<FileError32>,
    ) -> Result<FileCallOutcome32, FileError32> {
        let result = if let Some(error) = process_error {
            Err(error)
        } else {
            self.duplicate_kernel_handle_inner(memory, sync.as_deref_mut(), args)
        };
        if result.is_err()
            && args[6] & 1 != 0
            && args[0] == u32::MAX
            && let Ok(outcome) = self.close_kernel_handle(memory, sync, args[1])
            && matches!(
                outcome,
                FileCallOutcome32::Pending | FileCallOutcome32::PendingBytes(_)
            )
        {
            return Ok(outcome);
        }
        result
    }
    fn duplicate_kernel_handle_inner(
        &mut self,
        memory: &mut AddressSpace,
        mut sync: Option<&mut SynchronizationContext32>,
        args: &[u32],
    ) -> Result<FileCallOutcome32, FileError32> {
        if args[0] != u32::MAX || args[2] != u32::MAX && !(args[2] == 0 && args[6] & 1 != 0) {
            return Err(FileError32::InvalidHandle);
        }
        if args[6] & !3 != 0 {
            return Err(FileError32::InvalidParameter);
        }
        if args[2] == 0 {
            return self.close_kernel_handle(memory, sync, args[1]);
        }
        if args[3] != 0 {
            memory
                .check_span(GuestAddress(args[3]), 4, Access::Write)
                .map_err(|_| FileError32::NoAccess)?;
        }
        let object = self.handles.object(args[1])?;
        let access = if args[6] & 2 != 0 {
            self.handles.access(args[1])?
        } else if matches!(object, KernelFileObject32::CompletionPort(_)) {
            crate::windows::files::completion::completion_access(args[4])?
        } else if matches!(object, KernelFileObject32::Mapping(_)) {
            args[4]
        } else {
            ordinary::access::desired_handle_access(args[4])?
        };
        if access & !self.handles.access(args[1])? != 0 {
            return Err(FileError32::AccessDenied);
        }
        let prepared = self.handles.prepare(access, args[5] != 0)?;
        match object {
            KernelFileObject32::File(open) => self.vfs.retain_open(open)?,
            KernelFileObject32::Pipe(pipe) => self.pipes.retain(pipe)?,
            KernelFileObject32::Null(id) => self.null_devices.retain(id)?,
            KernelFileObject32::Mapping(id) => self.mappings.retain(id)?,
            KernelFileObject32::Find(_) => return Err(FileError32::InvalidHandle),
            KernelFileObject32::CompletionPort(port) => self.completion.retain(port)?,
        }
        if args[3] != 0 {
            memory
                .write(GuestAddress(args[3]), &prepared.handle().to_le_bytes())
                .map_err(|_| FileError32::NoAccess)?;
        }
        self.handles.commit(prepared, object);
        if args[6] & 1 != 0 {
            self.close_kernel_handle(memory, sync.take(), args[1])?;
        }
        Ok(FileCallOutcome32::success(1))
    }
}
