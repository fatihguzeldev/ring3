use super::super::DirectInputError32;
use crate::{
    input::dinput::{FactoryGate, MethodGate},
    process::{
        CallError, EngineInstance, HostError, WindowsProviderOutcome32, calls::call::PendingOwner,
    },
    windows::{CallFrame32, CallingConvention32},
};
impl EngineInstance {
    pub(in crate::process) fn try_complete_dinput_call(
        &mut self,
        owner: PendingOwner,
        token: u32,
        id: u32,
        frame: CallFrame32,
    ) -> Result<bool, HostError> {
        let factory = FactoryGate::from_id(id);
        let method = MethodGate::from_id(id);
        if factory.is_none() && method.is_none() {
            return Ok(false);
        }
        self.validate_windows_provider_owner(owner)?;
        let count = factory.map_or_else(
            || u32::from(method.unwrap().stack_words()),
            FactoryGate::stack_words,
        );
        if frame.convention() != CallingConvention32::Stdcall || frame.stack_words() != count {
            return Err(HostError::Call(CallError::InvalidRequest));
        }
        self.validate_new_windows_reply(owner, token)?;
        self.checked_pending_call(owner, token)?;
        self.check_windows_callback_capacity()?;
        Self::prepare_completed_call(frame.complete(0))?;
        self.sync_dinput_input()?;
        if method.is_some_and(|gate| gate.interface.device() && gate.slot == 13)
            && frame.arguments()[1] != 0
            && !self
                .user32
                .as_ref()
                .is_some_and(|user| user.is_top_level_window(frame.arguments()[1]))
        {
            let prepared = Self::prepare_completed_call(
                frame.complete(crate::input::dinput::DirectInputResult32::HANDLE.0),
            )?;
            self.publish_prepared_call(prepared);
            return Ok(true);
        }
        if let Some(gate) = method
            && self.try_complete_dinput_file_call(owner, token, gate, frame)?
        {
            return Ok(true);
        }
        let gates = if factory.is_some() {
            Some(self.module_gates()?)
        } else {
            None
        };
        let previous_policy = self
            .dinput
            .as_ref()
            .map_or((false, false), |runtime| runtime.input_policy());
        let policy_receipt = if method
            .is_some_and(|gate| gate.interface.device() && matches!(gate.slot, 2 | 7 | 8))
        {
            self.prepare_dinput_policy()?
        } else {
            None
        };
        let now = self.cpu_clock.monotonic_bounds()?.1;
        let ansi = self.windows_ansi_code_page();
        let runtime = self
            .dinput
            .as_mut()
            .ok_or(HostError::DirectInput(DirectInputError32::NotConfigured))?;
        runtime.now = now;
        runtime.ansi = ansi;
        if let Some(factory) = factory {
            runtime
                .initialize_methods(gates.as_deref().unwrap(), factory == FactoryGate::Create8)
                .map_err(|_| HostError::DirectInput(DirectInputError32::InvalidRequest))?;
        }
        let memory = self.memory.as_mut().ok_or(HostError::Closed)?;
        let source = &self
            .media
            .as_ref()
            .ok_or(HostError::DirectInput(DirectInputError32::NotConfigured))?
            .input
            .source();
        let windows = self
            .user32
            .as_ref()
            .map(super::super::super::user32::User32Context32::windows);
        let sync = self
            .synchronization
            .as_mut()
            .map(|runtime| runtime.context_mut());
        let result = if let Some(factory) = factory {
            runtime.factory(memory, factory, frame.arguments())
        } else {
            runtime.method(
                memory,
                windows,
                sync,
                source,
                method.unwrap(),
                frame.arguments(),
            )
        };
        let next_policy = runtime.input_policy();
        if next_policy != previous_policy {
            self.commit_dinput_policy(policy_receipt, next_policy);
        }
        match result.unwrap_or_else(|error| WindowsProviderOutcome32::Return(error.0)) {
            WindowsProviderOutcome32::Return(value) => {
                let prepared = Self::prepare_completed_call(frame.complete(value))?;
                self.publish_prepared_call(prepared);
            }
            WindowsProviderOutcome32::Callback(request) => self.park_media_callback(request)?,
        }
        Ok(true)
    }
    pub(in crate::process) fn resume_dinput_callback(
        &mut self,
        continuation: u64,
        result: u32,
    ) -> Result<WindowsProviderOutcome32, HostError> {
        let runtime = self
            .dinput
            .as_mut()
            .ok_or(HostError::DirectInput(DirectInputError32::NotConfigured))?;
        let memory = self.memory.as_mut().ok_or(HostError::Closed)?;
        runtime
            .complete_enumeration(
                memory,
                continuation,
                result,
                self.synchronization
                    .as_mut()
                    .map(|runtime| runtime.context_mut()),
            )
            .map_err(HostError::DirectInput)
    }
}
