use super::{
    ResourceConfig32, ResourceError32 as E, ResourceRuntime32,
    types::ResourceCallOutcome32 as Outcome,
};
use crate::{
    process::{CallError, EngineInstance, HostError, calls::call::PendingOwner},
    windows::{CallFrame32, CallingConvention32, resources::ResourceApi32 as Api},
};
impl EngineInstance {
    pub fn configure_resources(&mut self, config: ResourceConfig32) -> Result<(), HostError> {
        self.memory()?;
        if self.resources.is_some() {
            return Err(HostError::Resource(E::AlreadyConfigured));
        }
        if self.pending_call.is_some() || self.callback.is_some() {
            return Err(HostError::Call(CallError::Busy));
        }
        self.check_guest_reservation_admission(&[config.scratch])?;
        let runtime = ResourceRuntime32::new(config).map_err(HostError::Resource)?;
        self.resources = Some(runtime);
        Ok(())
    }
    pub(in crate::process) fn try_complete_resource_call(
        &mut self,
        owner: PendingOwner,
        token: u32,
        id: u32,
        frame: CallFrame32,
    ) -> Result<bool, HostError> {
        let Some(api) = Api::from_id(id) else {
            return Ok(false);
        };
        self.checked_pending_call(owner, token)?;
        self.validate_windows_provider_owner(owner)?;
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
        if api.is_enumeration() {
            self.check_windows_callback_capacity()?;
        }
        let outcome = match self.resource_call(api, frame.arguments()) {
            Ok(outcome) => outcome,
            Err(error) => Outcome::Return {
                value: 0,
                last_error: Some(error.win32()),
            },
        };
        match outcome {
            Outcome::Return { value, last_error } => {
                let prepared = Self::prepare_completed_call(frame.complete(value))?;
                if let Some(error) = last_error {
                    let thread = self.current_windows_thread()?.with_last_error(error);
                    self.publish_windows_thread(thread)?;
                }
                self.publish_prepared_call(prepared);
            }
            Outcome::Callback(request) => self.park_resource_callback(request)?,
        }
        Ok(true)
    }
    pub(super) fn resource_call(&mut self, api: Api, args: &[u32]) -> Result<Outcome, E> {
        if api.is_message() {
            return self.resource_message(api, args);
        }
        self.resources.as_ref().ok_or(E::NotConfigured)?;
        if api.is_enumeration() {
            return self.begin_resource_enumeration(api, args);
        }
        let value = match api {
            Api::LoadStringA | Api::LoadStringW => self.resource_string(api, args)?,
            Api::FreeResource | Api::UnlockResource => 0,
            Api::LockResource => self.lock_resource(args[0])?,
            _ => self.resource_lookup(api, args)?,
        };
        Ok(value.into())
    }
}
