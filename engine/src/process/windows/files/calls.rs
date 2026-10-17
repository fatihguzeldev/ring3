use super::*;
use crate::{
    memory::AddressSpace,
    process::{CallError, EngineInstance, HostError},
    windows::{
        CallingConvention32, WindowsApi32, files::FileApi32,
        synchronization::SynchronizationContext32,
    },
};

struct FileExecutionContext32<'a> {
    reserved: &'a [PageRange],
    now: u64,
    thread: u32,
    security: &'a crate::windows::security::SecurityRuntime32,
    threads: &'a crate::process::windows::threads::ThreadRuntime32,
    console_attached: bool,
}

impl EngineInstance {
    pub(in crate::process) fn check_guard_service_file_reply(
        &self,
        owner: PendingOwner,
        token: u32,
    ) -> Result<(), HostError> {
        if self.files.as_ref().is_none_or(|files| {
            files
                .reply
                .is_some_and(|reply| reply.owner != owner || reply.token != token)
        }) {
            return Err(HostError::Call(CallError::StateChanged));
        }
        Ok(())
    }
    pub(in crate::process) fn commit_guard_service_file_reply(&mut self) {
        let files = self.files.as_mut().unwrap();
        files.reply = None;
        files.pipe_wait = None;
    }
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
        self.collect_gdi_font_file_holds()?;
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
        let ansi = self.windows_ansi_code_page();
        let files = self
            .files
            .as_mut()
            .ok_or(HostError::File(FileError32::Unsupported))?;
        files.ordinary.ansi = ansi;
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
        ) && let Some(startup) = self.process_runtime.startup.as_mut()
        {
            files.set_process_current_directory(
                self.memory.as_mut().unwrap(),
                startup,
                args,
                api == Some(FileApi32::SetCurrentDirectoryW),
            )
        } else if let Some(api) = api {
            let now = if matches!(
                api,
                FileApi32::GetOverlappedResultEx
                    | FileApi32::GetQueuedCompletionStatus
                    | FileApi32::GetQueuedCompletionStatusEx
                    | FileApi32::WaitNamedPipeA
                    | FileApi32::WaitNamedPipeW
                    | FileApi32::CallNamedPipeA
                    | FileApi32::CallNamedPipeW
            ) {
                self.cpu_clock.monotonic_bounds()?.1
            } else {
                0
            };
            files.execute_file_api(
                self.memory.as_mut().unwrap(),
                sync,
                FileExecutionContext32 {
                    reserved: &reservations,
                    now,
                    thread: current_thread,
                    security: &self.windows_security,
                    threads: &self.threads,
                    console_attached: self.console.attached(),
                },
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
            Err(error)
                if matches!(
                    self.files.as_ref().unwrap().pipe_wait,
                    Some(pipes::PipeWait32::Operation(_))
                ) || self.files.as_ref().unwrap().asynchronous.blocking.is_some() =>
            {
                self.files.as_mut().unwrap().reply = Some(FileReply32 {
                    owner,
                    token,
                    frame,
                    id,
                    request: None,
                });
                return Err(HostError::File(error));
            }
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
                if !self.defer_guard_service_cleanup(
                    crate::process::windows::service::WindowsServiceCleanup32::Files,
                ) {
                    self.files.as_mut().unwrap().reply = None;
                }
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
        context: FileExecutionContext32<'_>,
        api: FileApi32,
        args: &[u32],
    ) -> Result<FileCallOutcome32, FileError32> {
        let FileExecutionContext32 {
            reserved,
            now,
            thread,
            security,
            threads,
            console_attached,
        } = context;
        if (0x000a_00a0..=0x000a_00ae).contains(&api.id()) {
            return self.execute_pipe_api(
                memory,
                sync,
                crate::process::windows::files::pipes::PipeCallContext32 {
                    thread,
                    now,
                    token: security.token(),
                },
                api,
                args,
            );
        }
        if matches!(api, FileApi32::CreateFileA | FileApi32::CreateFileW) {
            let path = ordinary::memory::path(
                memory,
                args[0],
                api == FileApi32::CreateFileW,
                self.ordinary.ansi,
            )?;
            if let Some(device) = crate::windows::files::devices::device_name(&path) {
                return self.open_device(memory, device, args, console_attached);
            }
            if crate::windows::files::pipes::pipe_name(&path)?.is_some() {
                return self.open_pipe_client(memory, &path, args, security.token());
            }
        }
        if args.first().is_some_and(|handle| {
            matches!(
                self.handles.object(*handle),
                Ok(handles::KernelFileObject32::Null(_))
            )
        }) {
            if matches!(api, FileApi32::ReadFile | FileApi32::WriteFile) {
                return self.transfer_null_device(memory, sync, api, args);
            }
            if matches!(
                api,
                FileApi32::GetFileType
                    | FileApi32::GetFileSize
                    | FileApi32::GetFileSizeEx
                    | FileApi32::SetFilePointer
                    | FileApi32::SetFilePointerEx
                    | FileApi32::SetEndOfFile
                    | FileApi32::FlushFileBuffers
            ) {
                return self.null_metadata(memory, api, args);
            }
        }
        if matches!(
            api,
            FileApi32::ReadFile
                | FileApi32::WriteFile
                | FileApi32::GetFileType
                | FileApi32::FlushFileBuffers
        ) && matches!(
            self.handles.object(args[0]),
            Ok(handles::KernelFileObject32::Pipe(_))
        ) {
            if matches!(api, FileApi32::GetFileType | FileApi32::FlushFileBuffers) {
                return self.pipe_metadata(memory, api, args);
            }
            if let Some(sync) = sync {
                let target = self.handles.io_target(args[0])?;
                return self.with_io_event(target, sync, |files, sync, _event| {
                    files.execute_pipe_api(
                        memory,
                        Some(sync),
                        crate::process::windows::files::pipes::PipeCallContext32 {
                            thread,
                            now,
                            token: security.token(),
                        },
                        api,
                        args,
                    )
                });
            }
            return self.execute_pipe_api(
                memory,
                None,
                crate::process::windows::files::pipes::PipeCallContext32 {
                    thread,
                    now,
                    token: security.token(),
                },
                api,
                args,
            );
        }
        if (0x000a_0090..=0x000a_0094).contains(&api.id()) {
            return self.execute_completion_api(memory, threads, now, thread, api, args);
        }
        if matches!(api, FileApi32::ReadFile | FileApi32::WriteFile) {
            let open = match self.handles.file(args[0]) {
                Ok(open) => open,
                Err(_) => return self.execute_ordinary_with_security(memory, security, api, args),
            };
            if self.vfs.open_options(open)?.overlapped {
                let sync = sync.ok_or(FileError32::Unsupported)?;
                if self.completion.modes(
                    crate::windows::files::completion::CompletionTarget32::File(open),
                ) & 2
                    != 0
                {
                    return self.execute_overlapped(memory, sync, thread, 0, api, args);
                }
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
                let name = ordinary::memory::path(
                    memory,
                    address,
                    api.name().ends_with('W'),
                    self.ordinary.ansi,
                )?;
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
        self.execute_ordinary_with_security(memory, security, api, args)
    }
}
