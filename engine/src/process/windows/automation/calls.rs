use crate::{
    process::{CallError, EngineInstance, HostError, calls::call::PendingOwner},
    windows::{
        CallFrame32,
        automation::{AutomationError32 as E, Oleaut32Api32 as Api},
    },
};
impl EngineInstance {
    pub fn register_oleaut32_module(&mut self, base: u32) -> Result<(), HostError> {
        self.register_system_module(crate::windows::automation::oleaut32_module(base))
    }
    pub(in crate::process) fn try_complete_automation_call(
        &mut self,
        owner: PendingOwner,
        token: u32,
        id: u32,
        frame: CallFrame32,
    ) -> Result<bool, HostError> {
        let Some(api) = Api::from_id(id) else {
            return Ok(false);
        };
        self.validate_windows_provider_owner(owner)?;
        self.checked_pending_call(owner, token)?;
        if frame.convention() != api.convention() || frame.stack_words() != api.stack_words() {
            return Err(HostError::Call(CallError::InvalidRequest));
        }
        Self::prepare_completed_call(frame.complete(0))?;
        self.check_windows_thread_write()?;
        self.memory()?
            .ensure_transaction_versions(1)
            .map_err(HostError::Memory)?;
        let result = if api.is_variant() || api.is_array() {
            Err(HostError::Automation(E::NotImplemented))
        } else {
            self.automation_string(api, frame.arguments())
        };
        let value = match result {
            Ok(value) => value,
            Err(HostError::Automation(error)) if !matches!(error, E::Memory(_)) => {
                if api.returns_hresult() {
                    error.hresult()
                } else {
                    0
                }
            }
            Err(HostError::WindowsMemory(
                crate::windows::memory::MemoryError32::Allocation
                | crate::windows::memory::MemoryError32::Capacity,
            )) => {
                if api.returns_hresult() {
                    E::Allocation.hresult()
                } else {
                    0
                }
            }
            Err(error) => return Err(error),
        };
        self.publish_prepared_call(Self::prepare_completed_call(frame.complete(value))?);
        Ok(true)
    }
}
