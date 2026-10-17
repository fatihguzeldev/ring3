use crate::{
    process::{CallError, EngineInstance, HostError, calls::call::PendingOwner},
    windows::{CallFrame32, CallingConvention32, imm32::ImmApi32, user32::User32Error32},
};

#[cfg(test)]
mod tests;

impl EngineInstance {
    pub fn register_imm32_module(&mut self, base: u32) -> Result<(), HostError> {
        self.register_system_module(crate::windows::imm32::imm32_module(base))
    }
    pub(in crate::process) fn try_complete_imm32_call(
        &mut self,
        owner: PendingOwner,
        token: u32,
        id: u32,
        frame: CallFrame32,
    ) -> Result<bool, HostError> {
        let Some(api) = ImmApi32::from_id(id) else {
            return Ok(false);
        };
        self.validate_windows_provider_owner(owner)?;
        if self.checked_pending_call(owner, token)?.frame != frame
            || frame.convention() != CallingConvention32::Stdcall
            || frame.stack_words() != api.stack_words()
        {
            return Err(HostError::Call(CallError::InvalidRequest));
        }
        Self::prepare_completed_call(frame.complete(0))?;
        self.check_windows_thread_write()?;
        self.memory()?
            .ensure_transaction_versions(2)
            .map_err(HostError::Memory)?;
        let thread = self.current_windows_thread_id()?;
        if api == ImmApi32::ImmGetDefaultIMEWnd {
            let value = self.default_ime_window(frame.arguments()[0], thread)?;
            self.publish_prepared_call(Self::prepare_completed_call(frame.complete(value))?);
            return Ok(true);
        }
        let code_page = self.windows_ansi_code_page();
        let context = self.user32.as_mut().ok_or(HostError::InvalidRequest)?;
        let memory = self.memory.as_mut().ok_or(HostError::Closed)?;
        let value = match context.complete_imm32_call(api, frame.arguments(), thread, code_page, memory) {
            Ok(value) => value,
            Err(User32Error32::Memory(error)) => return Err(HostError::Memory(error)),
            Err(_) => 0,
        };
        self.publish_prepared_call(Self::prepare_completed_call(frame.complete(value))?);
        Ok(true)
    }
}
