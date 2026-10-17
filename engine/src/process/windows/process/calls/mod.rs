mod atoms;
mod environment;
mod handles;
mod identity;
mod lifecycle;
mod modules;
mod numeric;
mod time;

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
        if api == ProcessApi32::TerminateProcess {
            if let Err(error) = self.validate_windows_process_handle(frame.arguments()[0], 1) {
                self.publish_windows_thread(
                    self.current_windows_thread()?
                        .with_last_error(error.win32()),
                )?;
                self.publish_prepared_call(Self::prepare_completed_call(frame.complete(0))?);
            } else {
                self.publish_process_exit(frame.arguments()[1])?;
            }
            return Ok(true);
        }
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
        match api {
            MulDiv => Ok(numeric::mul_div(args[0] as i32, args[1] as i32, args[2] as i32).into()),
            GlobalAddAtomA | GlobalFindAtomA | GlobalDeleteAtom | GlobalGetAtomNameA => {
                self.execute_process_atom_call(api, args)
            }
            CreateProcessA | CreateProcessW | GetExitCodeProcess | GetPriorityClass
            | SetPriorityClass | GetProcessVersion | TerminateProcess | SetHandleCount
            | SetLocalTime | Beep | OutputDebugStringA => {
                self.execute_process_lifecycle_call(api, args)
            }
            GetHandleInformation | SetHandleInformation => {
                self.execute_handle_information(api, args)
            }
            GetCurrentProcessId
            | GetCurrentThreadId
            | GetCurrentThread
            | GetStdHandle
            | SetStdHandle
            | EncodePointer
            | DecodePointer
            | IsProcessorFeaturePresent
            | GetStartupInfoA
            | GetStartupInfoW
            | IsDebuggerPresent
            | CheckRemoteDebuggerPresent
            | GetSystemInfo
            | GetVersion
            | GetVersionExA
            | GetVersionExW
            | GetComputerNameA
            | GetComputerNameW => self.execute_process_identity_call(api, args),
            GetCommandLineA
            | GetCommandLineW
            | GetEnvironmentVariableA
            | GetEnvironmentVariableW
            | SetEnvironmentVariableA
            | SetEnvironmentVariableW
            | ExpandEnvironmentStringsA
            | ExpandEnvironmentStringsW
            | GetEnvironmentStringsA
            | GetEnvironmentStringsW
            | FreeEnvironmentStringsA
            | FreeEnvironmentStringsW => self.execute_process_environment_call(api, args),
            GetModuleHandleExA | GetModuleHandleExW | GetModuleFileNameA | GetModuleFileNameW => {
                self.execute_process_modules_call(api, args)
            }
            QueryPerformanceCounter
            | QueryPerformanceFrequency
            | GetTickCount
            | GetTickCount64
            | GetSystemTimeAsFileTime => self.execute_process_time_call(api, args),
        }
    }
}

#[cfg(test)]
#[path = "../tests/module_names.rs"]
mod module_name_tests;

#[cfg(test)]
#[path = "../tests/runtime/mod.rs"]
mod runtime_tests;
