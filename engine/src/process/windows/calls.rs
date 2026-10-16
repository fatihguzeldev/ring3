use crate::process::{CallError, EngineInstance, HostError, calls::call::PendingOwner};
use crate::{
    abi::x86::decode_exit,
    cpu::ExitReason,
    windows::{ProcessContext32, WindowsApi32, WindowsOutcome32},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WindowsGateInfo32 {
    pub convention: crate::windows::CallingConvention32,
    pub stack_words: u32,
}

pub fn windows_gate_info(id: u32) -> Option<WindowsGateInfo32> {
    if let Some(api) = crate::windows::version::VersionApi32::from_id(id) {
        return Some(WindowsGateInfo32 { convention: api.convention(), stack_words: api.stack_words() });
    }
    if let Some(api) = crate::windows::shlwapi::ShlwapiApi32::from_id(id) {
        return Some(WindowsGateInfo32 { convention: api.convention(), stack_words: api.stack_words() });
    }
    if let Some(api) = crate::windows::activation::ActivationApi32::from_id(id) {
        return Some(WindowsGateInfo32 {
            convention: crate::windows::CallingConvention32::Stdcall,
            stack_words: api.stack_words(),
        });
    }
    if let Some(api) = crate::windows::security::SecurityApi32::from_id(id) {
        return Some(WindowsGateInfo32 {
            convention: crate::windows::CallingConvention32::Stdcall,
            stack_words: api.stack_words(),
        });
    }
    if let Some(api) = crate::windows::native::NativeApi32::from_id(id) {
        return Some(WindowsGateInfo32 {
            convention: crate::windows::CallingConvention32::Stdcall,
            stack_words: api.stack_words(),
        });
    }
    if let Some(api) = crate::windows::console::ConsoleApi32::from_id(id) {
        return Some(WindowsGateInfo32 {
            convention: crate::windows::CallingConvention32::Stdcall,
            stack_words: api.stack_words(),
        });
    }
    if let Some(api) = crate::windows::rawinput::RawInputApi32::from_id(id) {
        return Some(WindowsGateInfo32 {
            convention: crate::windows::CallingConvention32::Stdcall,
            stack_words: api.stack_words(),
        });
    }
    if let Some(api) = crate::windows::resources::ResourceApi32::from_id(id) {
        return Some(WindowsGateInfo32 {
            convention: crate::windows::CallingConvention32::Stdcall,
            stack_words: api.stack_words(),
        });
    }
    if let Some(api) = crate::windows::exceptions::ExceptionApi32::from_id(id) {
        return Some(WindowsGateInfo32 {
            convention: api.convention(),
            stack_words: api.stack_words(),
        });
    }
    if let Some(api) = crate::windows::nls::NlsApi32::from_id(id) {
        return Some(WindowsGateInfo32 {
            convention: crate::windows::CallingConvention32::Stdcall,
            stack_words: api.stack_words(),
        });
    }
    if matches!(id, 0x000f_fffe | 0x000f_ffff) {
        return Some(WindowsGateInfo32 {
            convention: crate::windows::CallingConvention32::Stdcall,
            stack_words: 0,
        });
    }
    if let Some(api) = WindowsApi32::from_id(id) {
        return Some(WindowsGateInfo32 {
            convention: api.convention(),
            stack_words: api.stack_words(),
        });
    }
    if let Some(words) = crate::windows::ole32::Ole32Api32::from_id(id)
        .map(|api| api.stack_words())
        .or_else(|| {
            crate::windows::ole32::FactoryMethod32::from_id(id).map(|api| api.stack_words())
        })
        .or_else(|| crate::windows::ole32::StreamMethod32::from_id(id).map(|api| api.stack_words()))
        .or_else(|| {
            crate::windows::ole32::RpcChannelMethod32::from_id(id).map(|api| api.stack_words())
        })
        .or_else(|| crate::windows::winmm::WinmmApi32::from_id(id).map(|api| api.stack_words()))
    {
        return Some(WindowsGateInfo32 {
            convention: crate::windows::CallingConvention32::Stdcall,
            stack_words: words,
        });
    }
    if let Some(api) = crate::windows::thread::ThreadApi32::from_id(id)
        .filter(|api| crate::windows::thread::ThreadApi32::core_apis().contains(api))
    {
        return Some(WindowsGateInfo32 {
            convention: crate::windows::CallingConvention32::Stdcall,
            stack_words: api.stack_words(),
        });
    }
    if let Some(api) = crate::windows::registry::Advapi32Api32::from_id(id) {
        return Some(WindowsGateInfo32 {
            convention: crate::windows::CallingConvention32::Stdcall,
            stack_words: api.stack_words(),
        });
    }
    if let Some(api) =
        crate::windows::synchronization::SynchronizationApi32::from_id(id).filter(|api| {
            crate::windows::synchronization::SynchronizationApi32::object_apis().contains(api)
                || *api == crate::windows::synchronization::SynchronizationApi32::QueueUserAPC
        })
    {
        return Some(WindowsGateInfo32 {
            convention: crate::windows::CallingConvention32::Stdcall,
            stack_words: api.stack_words(),
        });
    }
    if let Some(api) = crate::windows::process::ProcessApi32::from_id(id) {
        return Some(WindowsGateInfo32 {
            convention: crate::windows::CallingConvention32::Stdcall,
            stack_words: api.stack_words(),
        });
    }
    if let Some(api) = crate::windows::memory::MemoryApi32::from_id(id) {
        return Some(WindowsGateInfo32 {
            convention: crate::windows::CallingConvention32::Stdcall,
            stack_words: api.stack_words(),
        });
    }
    if let Some(api) = crate::windows::files::FileApi32::from_id(id) {
        return Some(WindowsGateInfo32 {
            convention: api.convention(),
            stack_words: api.stack_words(),
        });
    }
    if let Some(api) = crate::windows::user32::User32Api32::from_id(id) {
        return Some(WindowsGateInfo32 {
            convention: crate::windows::CallingConvention32::Stdcall,
            stack_words: api.stack_words(),
        });
    }
    if let Some(words) = crate::audio::dsound::FactoryGate::from_id(id)
        .map(|gate| gate.stack_words())
        .or_else(|| {
            crate::audio::dsound::MethodGate::from_id(id)
                .map(|gate| u32::from(gate.method().stack_words))
        })
    {
        return Some(WindowsGateInfo32 {
            convention: crate::windows::CallingConvention32::Stdcall,
            stack_words: words,
        });
    }
    if let Some(words) = crate::input::dinput::FactoryGate::from_id(id)
        .map(|gate| gate.stack_words())
        .or_else(|| {
            crate::input::dinput::MethodGate::from_id(id)
                .map(|gate| u32::from(gate.method().stack_words))
        })
    {
        return Some(WindowsGateInfo32 {
            convention: crate::windows::CallingConvention32::Stdcall,
            stack_words: words,
        });
    }
    let words = if id == crate::graphics::d3d8::api::CREATE_GATE {
        1
    } else {
        u32::from(
            crate::graphics::d3d8::api::MethodGate::from_id(id)?
                .method()
                .stack_words,
        )
    };
    Some(WindowsGateInfo32 {
        convention: crate::windows::CallingConvention32::Stdcall,
        stack_words: words,
    })
}

impl EngineInstance {
    pub fn capture_windows_call(
        &mut self,
        key: u64,
        generation: u32,
    ) -> Result<crate::abi::call_frame::CallRecord32, HostError> {
        let info = self.current_windows_gate_info()?;
        self.capture_call(key, generation, info.convention, info.stack_words)
    }
    pub fn capture_resident_windows_call(
        &mut self,
        key: u64,
        unit_id: u64,
    ) -> Result<crate::abi::call_frame::CallRecord32, HostError> {
        let info = self.current_windows_gate_info()?;
        self.capture_resident_call(key, unit_id, info.convention, info.stack_words)
    }
    pub fn capture_active_resident_callback_windows_call(
        &mut self,
        key: u64,
        unit_id: u64,
        callback_token: u32,
    ) -> Result<crate::abi::call_frame::CallRecord32, HostError> {
        let info = self.current_windows_gate_info()?;
        self.capture_active_resident_callback_call(
            key,
            unit_id,
            callback_token,
            info.convention,
            info.stack_words,
        )
    }
    fn current_windows_gate_info(&self) -> Result<WindowsGateInfo32, HostError> {
        let exit = decode_exit(
            &self.arena()[crate::abi::arena::EXIT_OFFSET
                ..crate::abi::arena::EXIT_OFFSET + crate::abi::x86::EXIT_SIZE],
        )
        .map_err(|_| HostError::Infrastructure)?;
        let ExitReason::Gate { id } = exit.reason else {
            return Err(HostError::Call(CallError::InvalidStop));
        };
        windows_gate_info(id).ok_or(HostError::Call(CallError::InvalidRequest))
    }
    pub fn complete_windows_call(
        &mut self,
        key: u64,
        generation: u32,
        token: u32,
    ) -> Result<(), HostError> {
        self.guard_artifact(key, generation)?;
        self.complete_windows_call_frame(PendingOwner::Replacement(generation), token)
    }

    pub fn complete_resident_windows_call(
        &mut self,
        key: u64,
        unit_id: u64,
        token: u32,
    ) -> Result<(), HostError> {
        self.guard_resident_unit(key, unit_id)?;
        self.complete_windows_call_frame(PendingOwner::Resident(unit_id), token)
    }

    pub fn complete_active_resident_callback_windows_call(
        &mut self,
        key: u64,
        unit_id: u64,
        callback_token: u32,
        inner_token: u32,
    ) -> Result<(), HostError> {
        let owner = self.active_resident_callback_call_owner(key, unit_id, callback_token)?;
        self.complete_windows_call_frame(owner, inner_token)
    }

    fn complete_windows_call_frame(
        &mut self,
        owner: PendingOwner,
        token: u32,
    ) -> Result<(), HostError> {
        let pending = self.checked_pending_call(owner, token)?;
        let frame = pending.frame;
        let exit = decode_exit(&pending.exit).map_err(|_| HostError::Infrastructure)?;
        let ExitReason::Gate { id } = exit.reason else {
            return Err(HostError::Call(CallError::InvalidStop));
        };
        let info = windows_gate_info(id).ok_or(HostError::Call(CallError::InvalidRequest))?;
        if frame.convention() != info.convention || frame.stack_words() != info.stack_words {
            return Err(HostError::Call(CallError::InvalidRequest));
        }
        self.execute_guarded_windows_service(owner, token, id, frame, |engine| {
            engine.dispatch_windows_call_frame(owner, token)
        })
    }

    fn dispatch_windows_call_frame(
        &mut self,
        owner: PendingOwner,
        token: u32,
    ) -> Result<(), HostError> {
        let pending = self.checked_pending_call(owner, token)?;
        let frame = pending.frame;
        let saved_exit = pending.exit;
        if self.retry_module_windows_call(owner, token)? {
            return Ok(());
        }
        if self.retry_windows_callback_reply(owner, token)? {
            return Ok(());
        }
        self.validate_windows_provider_owner(owner)?;
        let exit = decode_exit(&saved_exit).map_err(|_| HostError::Infrastructure)?;
        let ExitReason::Gate { id } = exit.reason else {
            return Err(HostError::Infrastructure);
        };
        if self.try_complete_version_call(owner, token, id, frame)?
            || self.try_complete_shlwapi_call(owner, token, id, frame)?
            || self.try_complete_console_call(owner, token, id, frame)?
            || self.try_complete_native_call(owner, token, id, frame)?
            || self.try_complete_security_call(owner, token, id, frame)?
            || self.try_complete_activation_call(owner, token, id, frame)?
            || self.try_complete_raw_input_call(owner, token, id, frame)?
            || self.try_complete_resource_call(owner, token, id, frame)?
            || self.try_complete_exception_call(owner, token, id, frame)?
            || self.try_complete_nls_call(owner, token, id, frame)?
            || self.try_complete_thread_call(owner, token, id, frame)?
            || self.try_complete_registry_call(owner, token, id, frame)?
            || self.try_complete_ole32_call(owner, token, id, frame)?
            || self.try_complete_winmm_call(owner, token, id, frame)?
            || self.try_complete_file_call(owner, token, id, frame)?
            || self.try_complete_process_call(owner, token, id, frame)?
            || self.try_complete_memory_call(owner, token, id, frame)?
            || self.try_complete_synchronization_call(owner, token, id, frame)?
            || self.try_complete_user32_call(owner, token, id, frame)?
            || self.try_complete_d3d8_call(owner, token, id, frame)?
            || self.try_complete_dsound_call(owner, token, id, frame)?
            || self.try_complete_dinput_call(owner, token, id, frame)?
        {
            return Ok(());
        }
        let api = WindowsApi32::from_id(id).ok_or(HostError::Call(CallError::InvalidRequest))?;
        if self.complete_module_api(api, frame)? {
            return Ok(());
        }
        let thread = self.current_windows_thread()?;
        if matches!(
            api,
            WindowsApi32::SetLastError
                | WindowsApi32::VirtualAlloc
                | WindowsApi32::VirtualFree
                | WindowsApi32::VirtualProtect
        ) {
            self.check_windows_thread_write()?;
        }
        let (outcome, thread) = thread
            .prepare(
                api,
                &frame,
                ProcessContext32 {
                    main_image_base: self
                        .image
                        .map(|image| crate::memory::GuestAddress(image.image_base)),
                },
            )
            .map_err(|_| HostError::Call(CallError::InvalidRequest))?;
        match outcome {
            WindowsOutcome32::Return(result) => {
                let prepared = Self::prepare_completed_call(frame.complete(result))?;
                self.publish_windows_thread(thread)?;
                self.publish_prepared_call(prepared);
            }
            WindowsOutcome32::ExitProcess(code) => {
                self.publish_process_exit(code)?;
            }
            WindowsOutcome32::Allocate { size, permissions } => {
                self.complete_virtual_alloc(frame, thread, size, permissions)?;
            }
            WindowsOutcome32::Release { address } => {
                self.complete_virtual_free(frame, thread, address)?;
            }
            WindowsOutcome32::Protect {
                range,
                permissions,
                old_output,
            } => {
                self.complete_virtual_protect(frame, thread, range, permissions, old_output)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "calls/tests/calls.rs"]
mod windows_tests;

#[cfg(test)]
#[path = "calls/tests/allocation.rs"]
mod windows_allocation_tests;

#[cfg(test)]
#[path = "calls/tests/callback.rs"]
mod windows_callback_tests;
