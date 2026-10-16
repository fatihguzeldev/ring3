use super::*;
use crate::{
    memory::{Access, AddressSpace, GuestAddress},
    process::{CallError, EngineInstance, HostError},
    windows::{
        CallingConvention32, WindowsApi32, files::FileApi32,
        synchronization::SynchronizationContext32,
    },
};

impl EngineInstance {
    pub(in crate::process) fn try_complete_file_call(
        &mut self,
        owner: PendingOwner,
        token: u32,
        id: u32,
        frame: CallFrame32,
    ) -> Result<bool, HostError> {
        if id == WindowsApi32::VirtualAlloc.id()
            && self.try_complete_file_mapping_allocation(owner, token, frame)?
        {
            return Ok(true);
        }
        let api = FileApi32::from_id(id);
        let kernel = WindowsApi32::from_id(id).filter(|api| {
            matches!(
                api,
                WindowsApi32::CloseHandle | WindowsApi32::DuplicateHandle
            )
        });
        if api.is_none()
            && (kernel.is_none()
                || frame
                    .arguments()
                    .get(if kernel == Some(WindowsApi32::DuplicateHandle) {
                        1
                    } else {
                        0
                    })
                    .is_none_or(|handle| {
                        *handle < 0x8000_0000 || matches!(*handle, u32::MAX | 0xffff_fffe)
                    }))
        {
            return Ok(false);
        }
        self.validate_windows_provider_owner(owner)?;
        self.checked_pending_call(owner, token)?;
        let info = crate::process::windows_gate_info(id).ok_or(HostError::InvalidRequest)?;
        if frame.convention() != CallingConvention32::Stdcall
            || frame.stack_words() != info.stack_words
        {
            return Err(HostError::Call(CallError::InvalidRequest));
        }
        if api.is_some_and(|api| {
            matches!(
                api,
                FileApi32::CopyFileExA
                    | FileApi32::CopyFileExW
                    | FileApi32::MoveFileWithProgressA
                    | FileApi32::MoveFileWithProgressW
            )
        }) && frame.arguments()[2] != 0
        {
            self.check_windows_callback_capacity()?;
            self.validate_new_windows_reply(owner, token)?;
        }
        Self::prepare_completed_call(frame.complete(0))?;
        self.check_windows_thread_write()?;
        let reservations = if api
            .is_some_and(|api| matches!(api, FileApi32::MapViewOfFile | FileApi32::MapViewOfFileEx))
        {
            self.guest_address_reservations()?
        } else {
            Vec::new()
        };
        let current_thread = self.current_windows_thread_id()?;
        let (duplicate_args, process_error) = if kernel == Some(WindowsApi32::DuplicateHandle) {
            let mut args = *<&[u32; 7]>::try_from(frame.arguments()).unwrap();
            let source = self.validate_windows_process_handle(args[0], 0x40);
            let target = if args[2] == 0 && args[6] & 1 != 0 {
                Ok(())
            } else {
                self.validate_windows_process_handle(args[2], 0x40)
            };
            if source.is_ok() {
                args[0] = u32::MAX;
            }
            if target.is_ok() && args[2] != 0 {
                args[2] = u32::MAX;
            }
            let error = source.and(target).err().map(|error| {
                if error == crate::windows::synchronization::SynchronizationError32::AccessDenied {
                    FileError32::AccessDenied
                } else {
                    FileError32::InvalidHandle
                }
            });
            (args, error)
        } else {
            ([0; 7], None)
        };
        let files = self
            .files
            .as_mut()
            .ok_or(HostError::File(FileError32::Unsupported))?;
        files.vfs.set_io_thread(current_thread);
        if files.reply.is_some_and(|reply| {
            reply.owner != owner || reply.token != token || reply.id != id || reply.frame != frame
        }) {
            return Err(HostError::Call(CallError::Busy));
        }
        self.memory
            .as_ref()
            .unwrap()
            .ensure_transaction_versions(
                if api.is_some_and(|api| (0x000a_0080..=0x000a_0087).contains(&api.id())) {
                    3
                } else {
                    2
                },
            )
            .map_err(HostError::Memory)?;
        let args = frame.arguments();
        let sync = self
            .synchronization
            .as_mut()
            .map(|runtime| runtime.context_mut());
        let outcome = if matches!(
            api,
            Some(FileApi32::SetCurrentDirectoryA | FileApi32::SetCurrentDirectoryW)
        ) && self.process_runtime.startup.is_some()
        {
            files.set_process_current_directory(
                self.memory.as_mut().unwrap(),
                self.process_runtime.startup.as_mut().unwrap(),
                args,
                api == Some(FileApi32::SetCurrentDirectoryW),
            )
        } else if let Some(api) = api {
            let now = if api == FileApi32::GetOverlappedResultEx {
                self.cpu_clock.monotonic_bounds()?.1
            } else {
                0
            };
            files.execute_file_api(
                self.memory.as_mut().unwrap(),
                sync,
                &reservations,
                now,
                current_thread,
                api,
                args,
            )
        } else if kernel == Some(WindowsApi32::CloseHandle) {
            files.close_kernel_handle(self.memory.as_mut().unwrap(), sync, args[0])
        } else {
            files.duplicate_kernel_handle(
                self.memory.as_mut().unwrap(),
                sync,
                &duplicate_args,
                process_error,
            )
        };
        let outcome = match outcome {
            Ok(outcome) => outcome,
            Err(error) => FileCallOutcome32::Return {
                value: api.map_or(0, |api| api.failure_value()),
                last_error: Some(error.win32()),
            },
        };
        match outcome {
            FileCallOutcome32::Callback(request) => {
                self.files.as_mut().unwrap().reply = None;
                self.park_file_callback(request)?;
            }
            FileCallOutcome32::Pending | FileCallOutcome32::PendingBytes(_) => {
                let request = if let FileCallOutcome32::PendingBytes(request) = outcome {
                    Some(request)
                } else {
                    None
                };
                self.files.as_mut().unwrap().reply = Some(FileReply32 {
                    owner,
                    token,
                    frame,
                    id,
                    request,
                });
            }
            FileCallOutcome32::Return { value, last_error } => {
                let prepared = Self::prepare_completed_call(frame.complete(value))?;
                if let Some(error) = last_error {
                    let thread = self.current_windows_thread()?.with_last_error(error);
                    self.publish_windows_thread(thread)?;
                }
                self.files.as_mut().unwrap().reply = None;
                self.publish_prepared_call(prepared);
            }
        }
        Ok(true)
    }
}
impl FileRuntime32 {
    fn execute_file_api(
        &mut self,
        memory: &mut AddressSpace,
        mut sync: Option<&mut SynchronizationContext32>,
        reserved: &[PageRange],
        now: u64,
        thread: u32,
        api: FileApi32,
        args: &[u32],
    ) -> Result<FileCallOutcome32, FileError32> {
        if matches!(api, FileApi32::ReadFile | FileApi32::WriteFile) {
            let open = match self.handles.file(args[0]) {
                Ok(open) => open,
                Err(_) => return self.execute_ordinary(memory, api, args),
            };
            if self.vfs.open_options(open)?.overlapped {
                let sync = sync.ok_or(FileError32::Unsupported)?;
                return self.with_file_event(open, sync, |files, sync, event| {
                    files.execute_overlapped(memory, sync, thread, event, api, args)
                });
            }
            if args[4] != 0 {
                return self.execute_positioned(memory, api, args);
            }
        }
        if matches!(
            api,
            FileApi32::CreateFileMappingA
                | FileApi32::CreateFileMappingW
                | FileApi32::OpenFileMappingA
                | FileApi32::OpenFileMappingW
        ) {
            let address = if matches!(
                api,
                FileApi32::CreateFileMappingA | FileApi32::CreateFileMappingW
            ) {
                args[5]
            } else {
                args[2]
            };
            if address != 0 {
                let name = ordinary::memory::path(memory, address, api.name().ends_with('W'))?;
                if let Some(sync) = sync.as_deref_mut() {
                    match sync.has_named_event(&name) {
                        Ok(true) => return Err(FileError32::InvalidHandle),
                        Ok(false) => {}
                        Err(
                            crate::windows::synchronization::SynchronizationError32::Unsupported,
                        ) => return Err(FileError32::Unsupported),
                        Err(_) => return Err(FileError32::InvalidName),
                    }
                }
            }
        }
        if (0x000a_0080..=0x000a_0087).contains(&api.id()) {
            return self.execute_mapping(memory, reserved, api, args);
        }
        if matches!(api, FileApi32::LockFileEx | FileApi32::UnlockFileEx)
            || (0x000a_0088..=0x000a_008b).contains(&api.id())
        {
            return self.execute_async(memory, sync, thread, now, api, args);
        }
        self.execute_ordinary(memory, api, args)
    }
    fn close_kernel_handle(
        &mut self,
        memory: &mut AddressSpace,
        sync: Option<&mut SynchronizationContext32>,
        handle: u32,
    ) -> Result<FileCallOutcome32, FileError32> {
        match self.handles.object(handle)? {
            KernelFileObject32::File(open) => {
                let last = self.handles.file_count(open) == 1;
                let event = if last {
                    self.events.iter().position(|(id, _)| *id == open)
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
                self.handles.remove(handle)?;
                if let Some(prepared) = prepared {
                    sync.unwrap().commit_duplicate(prepared);
                    self.events.remove(event.unwrap());
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
        }
        Ok(FileCallOutcome32::success(1))
    }
    fn duplicate_kernel_handle(
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
            KernelFileObject32::Mapping(id) => self.mappings.retain(id)?,
            KernelFileObject32::Find(_) => return Err(FileError32::InvalidHandle),
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
