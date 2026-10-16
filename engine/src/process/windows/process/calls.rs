use super::buffers;
use crate::{
    cpu::x86::Register32,
    process::{CallError, EngineInstance, HostError, calls::call::PendingOwner},
    windows::{
        CallFrame32, CallingConvention32,
        process::{ProcessApi32, ProcessError32},
    },
};
struct ProcessReply32 {
    value: u64,
    error: Option<u32>,
}
impl From<u32> for ProcessReply32 {
    fn from(value: u32) -> Self {
        Self {
            value: u64::from(value),
            error: None,
        }
    }
}
fn guest(error: ProcessError32) -> HostError {
    HostError::WindowsProcess(error)
}
impl EngineInstance {
    pub(in crate::process) fn try_complete_process_call(
        &mut self,
        owner: PendingOwner,
        token: u32,
        id: u32,
        frame: CallFrame32,
    ) -> Result<bool, HostError> {
        let Some(api) = ProcessApi32::from_id(id) else {
            return Ok(false);
        };
        self.validate_windows_provider_owner(owner)?;
        self.checked_pending_call(owner, token)?;
        if frame.convention() != CallingConvention32::Stdcall
            || frame.stack_words() != api.stack_words()
        {
            return Err(HostError::Call(CallError::InvalidRequest));
        }
        Self::prepare_completed_call(frame.complete(0))?;
        self.check_windows_thread_write()?;
        self.memory()?
            .ensure_transaction_versions(2)
            .map_err(HostError::Memory)?;
        let result = match self.execute_process_call(api, frame.arguments()) {
            Ok(result) => result,
            Err(HostError::WindowsProcess(error)) => ProcessReply32 {
                value: 0,
                error: Some(error.win32()),
            },
            Err(error) => return Err(error),
        };
        let mut state = frame.complete(result.value as u32);
        if api == ProcessApi32::GetTickCount64 {
            state.registers[Register32::Edx.index()] = (result.value >> 32) as u32;
        }
        let prepared = Self::prepare_completed_call(state)?;
        if let Some(error) = result.error {
            let thread = self.current_windows_thread()?.with_last_error(error);
            self.publish_windows_thread(thread)?;
        }
        self.publish_prepared_call(prepared);
        Ok(true)
    }
    fn execute_process_call(
        &mut self,
        api: ProcessApi32,
        args: &[u32],
    ) -> Result<ProcessReply32, HostError> {
        use ProcessApi32::*;
        let wide = api.name().ends_with('W');
        let result = match api {
            GetCurrentProcessId => self.process_runtime.launch.process_id,
            GetCurrentThreadId => self.current_windows_thread_id()?,
            GetCurrentThread => u32::MAX - 1,
            EncodePointer => crate::windows::process::PointerCodec32::new(self.key).encode(args[0]),
            DecodePointer => crate::windows::process::PointerCodec32::new(self.key).decode(args[0]),
            IsProcessorFeaturePresent => {
                u32::from(crate::windows::process::processor_feature_present(args[0]))
            }
            GetCommandLineA | GetCommandLineW => {
                self.process_runtime
                    .startup
                    .as_ref()
                    .ok_or(guest(ProcessError32::InvalidParameter))?
                    .command_line[usize::from(wide)]
            }
            GetEnvironmentVariableA | GetEnvironmentVariableW => {
                let name = buffers::string(self.memory()?, args[0], wide).map_err(guest)?;
                let value = self
                    .process_runtime
                    .environment
                    .get(&name)
                    .ok_or(guest(ProcessError32::NotFound))?
                    .to_owned();
                let count = buffers::counted(
                    self.memory.as_mut().unwrap(),
                    args[1],
                    args[2],
                    &value,
                    wide,
                )
                .map_err(guest)?;
                if value.is_empty() {
                    return Ok(ProcessReply32 {
                        value: 0,
                        error: Some(0),
                    });
                }
                count
            }
            SetEnvironmentVariableA | SetEnvironmentVariableW => {
                let name = buffers::string(self.memory()?, args[0], wide).map_err(guest)?;
                let value = if args[1] == 0 {
                    None
                } else {
                    Some(buffers::string(self.memory()?, args[1], wide).map_err(guest)?)
                };
                let mut environment = self.process_runtime.environment.clone();
                environment.set(&name, value.as_deref()).map_err(guest)?;
                if let Some(startup) = self.process_runtime.startup.as_mut() {
                    startup
                        .replace_environment(self.memory.as_mut().unwrap(), &environment)
                        .map_err(guest)?;
                }
                self.process_runtime.environment = environment;
                1
            }
            ExpandEnvironmentStringsA | ExpandEnvironmentStringsW => {
                let source = buffers::string(self.memory()?, args[0], wide).map_err(guest)?;
                let expanded = self.process_runtime.environment.expand(&source);
                let length = buffers::encoded(&expanded, wide).len() / if wide { 2 } else { 1 } + 1;
                buffers::counted(
                    self.memory.as_mut().unwrap(),
                    args[1],
                    args[2],
                    &expanded,
                    wide,
                )
                .map_err(guest)?;
                length as u32
            }
            GetEnvironmentStringsW => {
                let runtime = &mut self.process_runtime;
                let startup = runtime
                    .startup
                    .as_mut()
                    .ok_or(guest(ProcessError32::InvalidParameter))?;
                startup
                    .environment_snapshot(self.memory.as_mut().unwrap(), &runtime.environment)
                    .map_err(guest)?
            }
            FreeEnvironmentStringsW => {
                self.process_runtime
                    .startup
                    .as_mut()
                    .ok_or(guest(ProcessError32::InvalidParameter))?
                    .free_environment_snapshot(args[0])
                    .map_err(guest)?;
                1
            }
            QueryPerformanceCounter | QueryPerformanceFrequency => {
                let value = if api == QueryPerformanceCounter {
                    self.cpu_clock.timestamp()?
                } else {
                    self.cpu_clock.frequency()?
                };
                buffers::write(self.memory.as_mut().unwrap(), args[0], &value.to_le_bytes())
                    .map_err(guest)?;
                1
            }
            GetTickCount | GetTickCount64 => {
                let (epoch, now) = self.cpu_clock.monotonic_bounds()?;
                return Ok(ProcessReply32 {
                    value: (now - epoch) / 1_000_000,
                    error: None,
                });
            }
            GetSystemTimeAsFileTime => {
                let value = self.windows_filetime()?;
                buffers::write(self.memory.as_mut().unwrap(), args[0], &value.to_le_bytes())
                    .map_err(guest)?;
                0
            }
            GetStartupInfoA | GetStartupInfoW => {
                let mut bytes = [0; 68];
                bytes[..4].copy_from_slice(&68u32.to_le_bytes());
                buffers::write(self.memory.as_mut().unwrap(), args[0], &bytes).map_err(guest)?;
                0
            }
            GetModuleFileNameA | GetModuleFileNameW => {
                if args[2] == 0 {
                    return Err(guest(ProcessError32::InvalidParameter));
                }
                let mut bytes = if wide {
                    self.virtual_module_file_name_units(args[0])?
                        .into_iter()
                        .flat_map(u16::to_le_bytes)
                        .collect::<Vec<_>>()
                } else {
                    buffers::encoded(&self.virtual_module_file_name(args[0])?, false)
                };
                let width = if wide { 2 } else { 1 };
                let length = bytes.len() / width;
                if length >= args[2] as usize {
                    bytes.truncate(args[2] as usize * width);
                    let tail = bytes.len() - width;
                    bytes[tail..].fill(0);
                    buffers::write(self.memory.as_mut().unwrap(), args[1], &bytes)
                        .map_err(guest)?;
                    return Ok(ProcessReply32 {
                        value: u64::from(args[2]),
                        error: Some(122),
                    });
                }
                bytes.resize(bytes.len() + width, 0);
                buffers::write(self.memory.as_mut().unwrap(), args[1], &bytes).map_err(guest)?;
                length as u32
            }
            IsDebuggerPresent => 0,
            CheckRemoteDebuggerPresent => {
                if let Err(error) = self.validate_windows_process_handle(args[0], 0x400) {
                    return Ok(ProcessReply32 {
                        value: 0,
                        error: Some(error.win32()),
                    });
                }
                buffers::write(self.memory.as_mut().unwrap(), args[1], &0u32.to_le_bytes())
                    .map_err(guest)?;
                1
            }
            GetSystemInfo => {
                let words = [0, 4096, 0x10000, 0x7ffeffff, 1, 1, 586, 65536, 0x0301_0006];
                let bytes = words
                    .into_iter()
                    .flat_map(u32::to_le_bytes)
                    .collect::<Vec<_>>();
                buffers::write(self.memory.as_mut().unwrap(), args[0], &bytes).map_err(guest)?;
                0
            }
            GetVersion => 0x0a28_0105,
            GetVersionExA | GetVersionExW => {
                let mut size = [0; 4];
                self.memory()?
                    .read(crate::memory::GuestAddress(args[0]), &mut size)
                    .map_err(|_| guest(ProcessError32::NoAccess))?;
                let size = u32::from_le_bytes(size) as usize;
                let basic = if wide { 276 } else { 148 };
                if size != basic && size != basic + 8 {
                    return Err(guest(ProcessError32::InvalidParameter));
                }
                let mut bytes = vec![0; size];
                for (index, word) in [size as u32, 5, 1, 2600, 2].into_iter().enumerate() {
                    bytes[index * 4..index * 4 + 4].copy_from_slice(&word.to_le_bytes());
                }
                let service = buffers::encoded("Service Pack 3", wide);
                bytes[20..20 + service.len()].copy_from_slice(&service);
                if size > basic {
                    bytes[basic..basic + 2].copy_from_slice(&3u16.to_le_bytes());
                    bytes[basic + 6] = 1;
                }
                buffers::write(self.memory.as_mut().unwrap(), args[0], &bytes).map_err(guest)?;
                1
            }
        };
        Ok(result.into())
    }
}

#[cfg(test)]
#[path = "tests/module_names.rs"]
mod module_name_tests;

#[cfg(test)]
#[path = "tests/runtime/mod.rs"]
mod runtime_tests;
