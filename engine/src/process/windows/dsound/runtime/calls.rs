use super::super::{DSoundConfig32, DSoundError32, DSoundRuntime32};
use crate::{
    audio::dsound::{FactoryGate, MethodGate},
    process::{
        CallError, EngineInstance, HostError, WindowsProviderOutcome32, calls::call::PendingOwner,
    },
    windows::{CallFrame32, CallingConvention32},
};
impl EngineInstance {
    pub fn configure_dsound(&mut self, config: DSoundConfig32) -> Result<(), HostError> {
        self.media_contexts()?;
        if self.dsound.is_some() {
            return Err(HostError::DSound(DSoundError32::AlreadyConfigured));
        }
        if self.pending_call.is_some() || self.callback.is_some() {
            return Err(HostError::Call(CallError::Busy));
        }
        self.check_guest_reservation_admission(&[config.objects, config.locks])?;
        self.dsound = Some(DSoundRuntime32::new(config).map_err(HostError::DSound)?);
        Ok(())
    }
    pub fn register_dsound_module(&mut self, preferred_base: u32) -> Result<(), HostError> {
        self.register_system_module(crate::audio::dsound::system_module(preferred_base))
    }
    pub(in crate::process) fn try_complete_dsound_call(
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
        self.checked_pending_call(owner, token)?;
        self.validate_windows_provider_owner(owner)?;
        let words = factory.map_or_else(
            || u32::from(method.unwrap().method().stack_words),
            FactoryGate::stack_words,
        );
        if frame.convention() != CallingConvention32::Stdcall || frame.stack_words() != words {
            return Err(HostError::Call(CallError::InvalidRequest));
        }
        Self::prepare_completed_call(frame.complete(0))?;
        self.check_windows_callback_capacity()?;
        let gates = if matches!(
            factory,
            Some(
                FactoryGate::Create
                    | FactoryGate::Create8
                    | FactoryGate::CaptureCreate
                    | FactoryGate::CaptureCreate8
                    | FactoryGate::FullDuplexCreate8
            )
        ) {
            Some(self.module_gates()?)
        } else {
            None
        };
        let runtime = self
            .dsound
            .as_mut()
            .ok_or(HostError::DSound(DSoundError32::NotConfigured))?;
        let memory = self.memory.as_mut().ok_or(HostError::Closed)?;
        let audio = &mut self
            .media
            .as_mut()
            .ok_or(HostError::Media(crate::process::MediaError::NotConfigured))?
            .audio;
        let windows = self
            .user32
            .as_ref()
            .map(super::super::super::user32::User32Context32::windows);
        let outcome = (|| {
            if let Some(gates) = &gates {
                runtime.initialize_methods(gates)?;
            }
            if let Some(factory) = factory {
                runtime.factory_call(memory, audio, windows, factory, frame.arguments())
            } else {
                let sync = self
                    .synchronization
                    .as_mut()
                    .map(|runtime| runtime.context_mut());
                runtime
                    .method_call(
                        memory,
                        audio,
                        sync,
                        windows,
                        method.unwrap(),
                        frame.arguments(),
                    )
                    .map(WindowsProviderOutcome32::Return)
            }
        })()
        .unwrap_or_else(|error| WindowsProviderOutcome32::Return(error.0));
        self.refresh_dsound_focus()?;
        match outcome {
            WindowsProviderOutcome32::Return(result) => {
                self.publish_prepared_call(Self::prepare_completed_call(frame.complete(result))?)
            }
            WindowsProviderOutcome32::Callback(request) => self.park_media_callback(request)?,
        };
        Ok(true)
    }
    pub(in crate::process) fn resume_dsound_callback(
        &mut self,
        continuation: u64,
        result: u32,
    ) -> Result<WindowsProviderOutcome32, HostError> {
        let runtime = self
            .dsound
            .as_mut()
            .ok_or(HostError::DSound(DSoundError32::NotConfigured))?;
        Ok(
            match runtime.resume_enumeration(
                self.memory.as_ref().ok_or(HostError::Closed)?,
                continuation,
                result,
            ) {
                Ok(outcome) => outcome,
                Err(error) => {
                    runtime
                        .enumerations
                        .retain(|pending| pending.id() != continuation);
                    WindowsProviderOutcome32::Return(error.0)
                }
            },
        )
    }
}
