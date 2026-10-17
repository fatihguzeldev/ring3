use crate::{
    abi::{
        arena::EXIT_OFFSET,
        x86::{EXIT_SIZE, decode_exit, encode_exit_v4},
    },
    cpu::{ExecutionExit, ExitReason},
    loader::{ImportSymbol32, ModuleError32},
    process::{CallError, EngineInstance, HostError, ModuleOperation32},
    windows::{CallFrame32, WindowsApi32},
};

use crate::process::calls::call::PendingOwner;
mod directory;

#[derive(Clone, Copy)]
pub(in crate::process) struct ModuleWindowsReply {
    owner: PendingOwner,
    token: u32,
    initializer_token: Option<u64>,
    operation: Option<ModuleOperation32>,
    sink: ModuleReplySink32,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum ModuleReplySink32 {
    Windows,
    Ole32,
}
impl ModuleWindowsReply {
    pub(in crate::process) fn waiting_loader(&self) -> bool {
        self.operation.is_none()
    }
    pub(in crate::process) fn owner(&self) -> PendingOwner {
        self.owner
    }
}

impl EngineInstance {
    pub fn register_kernel32_module(&mut self, preferred_base: u32) -> Result<(), HostError> {
        let kernel = crate::windows::kernel32_module(preferred_base);
        if self.has_registered_module("ntdll.dll") {
            self.register_system_module(kernel)?;
        } else {
            self.register_system_module_group(vec![
                kernel,
                crate::windows::exceptions::ntdll_module(0x77f00000),
            ])?;
        }
        self.enable_windows_runtime_profile();
        Ok(())
    }

    pub(super) fn complete_module_api(
        &mut self,
        api: WindowsApi32,
        frame: CallFrame32,
    ) -> Result<bool, HostError> {
        use WindowsApi32::*;
        let general = self.module_teb_address().is_some();
        if !matches!(
            api,
            LoadLibraryA
                | LoadLibraryW
                | LoadLibraryExA
                | LoadLibraryExW
                | GetProcAddress
                | FreeLibrary
                | GetModuleHandleW
                | SetDllDirectoryA
                | SetDllDirectoryW
                | GetDllDirectoryA
                | GetDllDirectoryW
        ) && !(general && matches!(api, GetModuleHandleA | ExitProcess))
        {
            return Ok(false);
        }
        if frame.convention() != api.convention() || frame.stack_words() != api.stack_words() {
            return Err(HostError::Call(CallError::InvalidRequest));
        }
        if !matches!(api, GetModuleHandleA | GetModuleHandleW | ExitProcess) {
            self.check_windows_thread_write()?;
        }
        let args = frame.arguments();
        if matches!(
            api,
            SetDllDirectoryA | SetDllDirectoryW | GetDllDirectoryA | GetDllDirectoryW
        ) {
            self.complete_module_directory_api(api, frame)?;
            return Ok(true);
        }
        if api == ExitProcess && !self.advance_windows_fls_process_exit()? {
            return Ok(true);
        }
        if matches!(
            api,
            LoadLibraryA
                | LoadLibraryW
                | LoadLibraryExA
                | LoadLibraryExW
                | GetProcAddress
                | FreeLibrary
                | ExitProcess
        ) && self
            .loader_initialization_owner()
            .is_some_and(|owner| Some(owner) != self.current_windows_thread_id().ok())
        {
            let request = match self.prepare_windows_module_request(api, frame) {
                Ok(request) => request,
                Err(error) => {
                    let code = module_api_error(error)?;
                    self.complete_module_api_return(frame, 0, Some(code))?;
                    return Ok(true);
                }
            };
            let pending = self
                .pending_call
                .as_ref()
                .ok_or(HostError::Infrastructure)?;
            let reply = ModuleWindowsReply {
                owner: pending.owner,
                token: pending.token,
                initializer_token: None,
                operation: None,
                sink: ModuleReplySink32::Windows,
            };
            self.park_module_request(request)?;
            self.module_windows_reply = Some(reply);
            return Ok(true);
        }
        let result = (|| match api {
            LoadLibraryExA | LoadLibraryExW if args[1] != 0 => {
                Err(HostError::Module(ModuleError32::InvalidRequest))
            }
            LoadLibraryA | LoadLibraryW | LoadLibraryExA | LoadLibraryExW => {
                let name = self.windows_string(
                    args[0],
                    matches!(api, LoadLibraryW | LoadLibraryExW),
                    32767,
                )?;
                if matches!(api, LoadLibraryExA | LoadLibraryExW) && args[2] != 0 {
                    self.load_library_module_flags(&name, args[2])
                } else {
                    self.load_library_module(&name, None)
                }
            }
            GetModuleHandleA | GetModuleHandleW => {
                let name = if args[0] == 0 {
                    None
                } else {
                    Some(self.windows_string(args[0], api == GetModuleHandleW, 32767)?)
                };
                let handle = self
                    .module_handle(name.as_deref())?
                    .ok_or(HostError::Module(ModuleError32::MissingModule))?;
                Ok(ModuleOperation32::Loaded { handle })
            }
            GetProcAddress => {
                let symbol = if args[1] <= u16::MAX as u32 {
                    ImportSymbol32::Ordinal(args[1] as u16)
                } else {
                    ImportSymbol32::Name(self.windows_export_name(args[1], 4096)?)
                };
                self.resolve_module_export(args[0], &symbol)
            }
            FreeLibrary => {
                if args[0] & 3 != 0 {
                    self.free_resource_only_module(args[0])
                } else {
                    self.free_library_module(args[0])
                }
            }
            ExitProcess => self.detach_modules_for_exit(),
            _ => unreachable!(),
        })();
        match result {
            Ok(operation) => {
                if api == WindowsApi32::ExitProcess {
                    self.process_exit_owner = Some(self.threads.current_id());
                }
                self.complete_module_windows_operation(operation)?
            }
            Err(error) => {
                if api == WindowsApi32::ExitProcess {
                    return Err(error);
                }
                let code = module_api_error(error)?;
                self.complete_module_api_return(frame, 0, Some(code))?;
            }
        }
        Ok(true)
    }

    pub(in crate::process) fn complete_module_windows_operation(
        &mut self,
        operation: ModuleOperation32,
    ) -> Result<(), HostError> {
        self.complete_module_windows_result(operation, None)
    }

    pub(in crate::process) fn complete_module_windows_initializer(
        &mut self,
        operation: ModuleOperation32,
        token: u64,
    ) -> Result<(), HostError> {
        if token == 0 {
            return Ok(());
        }
        self.complete_module_windows_result(operation, Some(token))
    }

    pub(in crate::process) fn retry_module_windows_initializer(
        &mut self,
        token: u64,
    ) -> Result<Option<ModuleOperation32>, HostError> {
        let Some(reply) = self.module_windows_reply else {
            return Ok(None);
        };
        if token == 0 || reply.initializer_token != Some(token) {
            return Err(HostError::Module(ModuleError32::InvalidToken));
        }
        self.guard_detection_material_change()?;
        self.publish_module_windows_reply(reply)?;
        Ok(Some(reply.operation.ok_or(HostError::Infrastructure)?))
    }

    pub(super) fn retry_module_windows_call(
        &mut self,
        owner: PendingOwner,
        token: u32,
    ) -> Result<bool, HostError> {
        let Some(reply) = self.module_windows_reply else {
            return Ok(false);
        };
        if reply.owner != owner || reply.token != token {
            return Err(HostError::Call(CallError::InvalidToken));
        }
        if reply.operation.is_none() {
            if self
                .loader_initialization_owner()
                .is_some_and(|owner| Some(owner) != self.current_windows_thread_id().ok())
            {
                return Ok(true);
            }
            let request = self
                .take_module_request()
                .ok_or(HostError::Infrastructure)?;
            self.module_windows_reply = None;
            let frame = self.checked_pending_call(owner, token)?.frame;
            match self.execute_module_request(&request) {
                Ok(operation) => {
                    if matches!(
                        request,
                        crate::process::loading::modules::ModuleRequest32::Shutdown
                    ) {
                        self.process_exit_owner = Some(self.threads.current_id());
                    }
                    self.complete_module_windows_operation(operation)?
                }
                Err(error) => match module_api_error(error) {
                    Ok(code) => self.complete_module_api_return(frame, 0, Some(code))?,
                    Err(error) => {
                        self.park_module_request(request)?;
                        self.module_windows_reply = Some(reply);
                        return Err(error);
                    }
                },
            }
        } else {
            self.publish_module_windows_reply(reply)?;
        }
        Ok(true)
    }

    fn complete_module_windows_result(
        &mut self,
        operation: ModuleOperation32,
        initializer_token: Option<u64>,
    ) -> Result<(), HostError> {
        if matches!(
            operation,
            ModuleOperation32::Pending(_) | ModuleOperation32::NeedBytes { .. }
        ) {
            return Ok(());
        }
        let Some(pending) = self.pending_call.as_ref() else {
            return Ok(());
        };
        if self.registered_com_owns_call(pending.owner, pending.token) {
            if self.module_windows_reply.is_some() {
                return Err(HostError::Call(CallError::Busy));
            }
            let reply = ModuleWindowsReply {
                owner: pending.owner,
                token: pending.token,
                initializer_token,
                operation: Some(operation),
                sink: ModuleReplySink32::Ole32,
            };
            self.module_windows_reply = Some(reply);
            return self.publish_module_windows_reply(reply);
        }
        let exit = decode_exit(&pending.exit).map_err(|_| HostError::Infrastructure)?;
        let ExitReason::Gate { id } = exit.reason else {
            return Ok(());
        };
        let Some(api) = WindowsApi32::from_id(id) else {
            return Ok(());
        };
        if !matches!(
            api,
            WindowsApi32::LoadLibraryA
                | WindowsApi32::LoadLibraryW
                | WindowsApi32::LoadLibraryExA
                | WindowsApi32::LoadLibraryExW
                | WindowsApi32::GetProcAddress
                | WindowsApi32::FreeLibrary
                | WindowsApi32::GetModuleHandleA
                | WindowsApi32::GetModuleHandleW
                | WindowsApi32::ExitProcess
        ) {
            return Ok(());
        }
        if self.module_windows_reply.is_some() {
            return Err(HostError::Call(CallError::Busy));
        }
        let reply = ModuleWindowsReply {
            owner: pending.owner,
            token: pending.token,
            initializer_token,
            operation: Some(operation),
            sink: ModuleReplySink32::Windows,
        };
        self.module_windows_reply = Some(reply);
        self.publish_module_windows_reply(reply)
    }

    fn publish_module_windows_reply(&mut self, reply: ModuleWindowsReply) -> Result<(), HostError> {
        self.execute_pending_guarded_service(reply.owner, reply.token, |engine| {
            engine.advance_module_windows_reply(reply)
        })
    }

    fn advance_module_windows_reply(&mut self, reply: ModuleWindowsReply) -> Result<(), HostError> {
        let pending = self.checked_pending_call(reply.owner, reply.token)?;
        if reply.sink == ModuleReplySink32::Ole32 {
            let operation = reply.operation.ok_or(HostError::Call(CallError::Busy))?;
            if !self.complete_registered_com_module_operation(operation)? {
                return Err(HostError::Infrastructure);
            }
            if self.module_windows_reply.is_some_and(|current| {
                current.owner == reply.owner
                    && current.token == reply.token
                    && current.initializer_token == reply.initializer_token
                    && current.sink == ModuleReplySink32::Ole32
            }) {
                self.module_windows_reply = None;
            }
            return Ok(());
        }
        let exit = decode_exit(&pending.exit).map_err(|_| HostError::Infrastructure)?;
        let ExitReason::Gate { id } = exit.reason else {
            return Err(HostError::Infrastructure);
        };
        let api = WindowsApi32::from_id(id).ok_or(HostError::Infrastructure)?;
        let operation = reply.operation.ok_or(HostError::Call(CallError::Busy))?;
        let frame = pending.frame;
        if api == WindowsApi32::ExitProcess && operation == ModuleOperation32::Released {
            return self.publish_process_exit(frame.arguments()[0]);
        }
        let (result, error) = match operation {
            ModuleOperation32::Loaded { handle } => (handle, None),
            ModuleOperation32::ExportResolved { address } => (address, None),
            ModuleOperation32::Released => (1, None),
            ModuleOperation32::InitializationFailed => (0, Some(1114)),
            ModuleOperation32::Pending(_) | ModuleOperation32::NeedBytes { .. } => unreachable!(),
        };
        self.complete_module_api_return(frame, result, error)
    }

    fn complete_module_api_return(
        &mut self,
        frame: CallFrame32,
        result: u32,
        error: Option<u32>,
    ) -> Result<(), HostError> {
        let prepared = Self::prepare_completed_call(frame.complete(result))?;
        let thread = self.current_windows_thread()?;
        self.publish_windows_thread(error.map_or(thread, |error| thread.with_last_error(error)))?;
        self.publish_prepared_call(prepared);
        Ok(())
    }

    pub(in crate::process) fn publish_process_exit(&mut self, code: u32) -> Result<(), HostError> {
        if self
            .pending_process_exit
            .is_some_and(|pending| pending != code)
        {
            return Err(HostError::Call(CallError::Busy));
        }
        let mut bytes = [0; EXIT_SIZE];
        encode_exit_v4(
            &ExecutionExit {
                retired: 0,
                reason: ExitReason::ProcessExited { code },
            },
            &mut bytes,
        )
        .map_err(|_| HostError::Infrastructure)?;
        let mut extra_ranges = if let Some(gdi) = self.gdi.as_ref() {
            gdi.check_process_exit(self.memory()?)
                .map_err(HostError::Gdi)?;
            let mut ranges = Vec::new();
            ranges
                .try_reserve_exact(gdi.process_exit_ranges().count())
                .map_err(|_| HostError::Infrastructure)?;
            ranges.extend(gdi.process_exit_ranges());
            ranges.sort_unstable_by_key(|range| range.first);
            ranges
        } else {
            Vec::new()
        };
        self.check_automation_process_scratch()?;
        extra_ranges
            .try_reserve_exact(self.automation_process_scratch_ranges().count())
            .map_err(|_| HostError::Infrastructure)?;
        extra_ranges.extend(self.automation_process_scratch_ranges());
        extra_ranges.sort_unstable_by_key(|range| range.first);
        let media = self.prepare_media_process_exit()?;
        let automation_arrays: crate::process::windows::automation::PreparedAutomationArrays32 =
            self.prepare_automation_array_retirement(None)?;
        let automation_allocations = self.automation_process_temporary_allocations()?;
        let automation_retirement =
            self.prepare_windows_runtime_retirement(&automation_allocations)?;
        let mut bindings = self
            .registry
            .as_ref()
            .map(|registry| registry.process_exit_bindings())
            .transpose()
            .map_err(HostError::Registry)?
            .unwrap_or_default();
        bindings
            .try_reserve(media.bindings().len())
            .map_err(|_| HostError::Infrastructure)?;
        bindings.extend_from_slice(media.bindings());
        if !self.finalize_file_process_exit(
            &bindings,
            media.handles(),
            &extra_ranges,
            automation_arrays.words(),
        )? {
            self.pending_process_exit = Some(code);
            self.process_exit_owner = Some(self.threads.current_id());
            return Ok(());
        }
        self.commit_automation_array_retirement(automation_arrays);
        self.commit_windows_runtime_retirement_after_memory(automation_retirement);
        self.commit_automation_temporary_retirement(&automation_allocations);
        self.commit_media_process_exit(media);
        if let Some(gdi) = self.gdi.as_mut() {
            gdi.retire_process();
        }
        self.automation.terminate();
        if let Some(resources) = self.resources.as_mut() {
            resources.commit_process_exit();
        }
        self.version.terminate();
        self.nls.commit_process_exit();
        self.retire_activation_process();
        self.exceptions.deliveries_clear_process_exit();
        self.threads.retire_process_threads(code);
        if let Some(registry) = self.registry.as_mut() {
            registry.commit_process_exit();
        }
        self.arena.as_mut().get_mut()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE].copy_from_slice(&bytes);
        self.exit_code = Some(code);
        self.pending_process_exit = None;
        self.pending_call = None;
        self.module_windows_reply = None;
        self.callback = None;
        self.windows_replies.clear();
        Ok(())
    }
}

fn module_api_error(error: HostError) -> Result<u32, HostError> {
    match error {
        HostError::Module(error) => Ok(match error {
            ModuleError32::MissingModule | ModuleError32::InvalidName => 126,
            ModuleError32::MissingSymbol
            | ModuleError32::ForwarderCycle
            | ModuleError32::InvalidForwarder => 127,
            ModuleError32::InvalidHandle | ModuleError32::NotFound => 6,
            ModuleError32::Capacity
            | ModuleError32::Allocation
            | ModuleError32::ReferenceExhausted => 8,
            ModuleError32::InitializationFailed => 1114,
            ModuleError32::Pe(_) | ModuleError32::InvalidModuleKind => 193,
            ModuleError32::Busy | ModuleError32::ModuleInUse => 170,
            ModuleError32::Memory(_) => 998,
            _ => 87,
        }),
        HostError::Memory(crate::memory::MemoryError::Fault(_)) => Ok(998),
        HostError::Activation(error) => Ok(error.win32()),
        HostError::File(error) => Ok(error.win32()),
        other => Err(other),
    }
}

impl EngineInstance {
    fn prepare_windows_module_request(
        &self,
        api: WindowsApi32,
        frame: CallFrame32,
    ) -> Result<crate::process::loading::modules::ModuleRequest32, HostError> {
        use crate::process::loading::modules::ModuleRequest32;
        use WindowsApi32::*;
        let args = frame.arguments();
        Ok(match api {
            LoadLibraryA | LoadLibraryW | LoadLibraryExA | LoadLibraryExW => {
                if matches!(api, LoadLibraryExA | LoadLibraryExW) && args[1] != 0 {
                    return Err(HostError::Module(ModuleError32::InvalidRequest));
                }
                ModuleRequest32::Load {
                    flags: if matches!(api, LoadLibraryExA | LoadLibraryExW) {
                        args[2]
                    } else {
                        0
                    },
                    name: self.windows_string(
                        args[0],
                        matches!(api, LoadLibraryW | LoadLibraryExW),
                        32767,
                    )?,
                }
            }
            GetProcAddress => ModuleRequest32::Export {
                handle: args[0],
                symbol: if args[1] <= u16::MAX as u32 {
                    ImportSymbol32::Ordinal(args[1] as u16)
                } else {
                    ImportSymbol32::Name(self.windows_export_name(args[1], 4096)?)
                },
            },
            FreeLibrary => ModuleRequest32::Free { handle: args[0] },
            ExitProcess => ModuleRequest32::Shutdown,
            _ => return Err(HostError::Infrastructure),
        })
    }
    pub(in crate::process) fn module_loader_wait_ready(&self) -> bool {
        self.module_source_ready()
            || self
                .module_windows_reply
                .is_some_and(|reply| reply.operation.is_none())
                && self
                    .loader_initialization_owner()
                    .is_none_or(|owner| Some(owner) == self.current_windows_thread_id().ok())
    }
}

impl EngineInstance {
    pub(in crate::process) fn resume_module_loader_wait(&mut self) -> Result<bool, HostError> {
        if !self.module_loader_wait_ready() {
            return Ok(false);
        }
        if let Some(source) = self.pending_module_source()
            && source.ready
        {
            self.resume_module_source(source.epoch, source.token)?;
            return Ok(true);
        }
        let reply = self.module_windows_reply.ok_or(HostError::Infrastructure)?;
        self.retry_module_windows_call(reply.owner, reply.token)
    }
}

impl EngineInstance {
    pub(in crate::process) fn complete_module_source_failure(
        &mut self,
        error: HostError,
    ) -> Result<bool, HostError> {
        if self.fail_registered_com_module_source(error)? {
            return Ok(true);
        }
        let Some(pending) = &self.pending_call else {
            return Ok(false);
        };
        let exit = decode_exit(&pending.exit).map_err(|_| HostError::Infrastructure)?;
        let ExitReason::Gate { id } = exit.reason else {
            return Ok(false);
        };
        let Some(api) = WindowsApi32::from_id(id) else {
            return Ok(false);
        };
        if !matches!(
            api,
            WindowsApi32::LoadLibraryA
                | WindowsApi32::LoadLibraryW
                | WindowsApi32::LoadLibraryExA
                | WindowsApi32::LoadLibraryExW
                | WindowsApi32::GetProcAddress
        ) {
            return Ok(false);
        }
        let frame = pending.frame;
        let code = module_api_error(error)?;
        self.complete_module_api_return(frame, 0, Some(code))?;
        Ok(true)
    }
}
