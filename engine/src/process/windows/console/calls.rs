use crate::{
    memory::{Access, GuestAddress},
    process::{CallError, EngineInstance, HostError, calls::call::PendingOwner},
    windows::{CallFrame32, CallingConvention32, console::ConsoleApi32},
};
impl EngineInstance {
    pub(in crate::process) fn try_complete_console_call(
        &mut self,
        owner: PendingOwner,
        token: u32,
        id: u32,
        frame: CallFrame32,
    ) -> Result<bool, HostError> {
        let Some(api) = ConsoleApi32::from_id(id) else {
            return Ok(false);
        };
        self.validate_windows_provider_owner(owner)?;
        if self.checked_pending_call(owner, token)?.frame != frame
            || frame.convention() != CallingConvention32::Stdcall
            || frame.stack_words() != api.stack_words()
        {
            return Err(HostError::Call(CallError::InvalidRequest));
        }
        let args = frame.arguments();
        let change = if api == ConsoleApi32::SetConsoleCtrlHandler {
            if args[0] != 0 && args[1] != 0 {
                self.memory()?
                    .check_span(GuestAddress(args[0]), 1, Access::Execute)
                    .map_err(HostError::Memory)?;
            }
            self.console.prepare(args[0], args[1] != 0).map(Some)
        } else {
            Err(6)
        };
        let value = u32::from(change.is_ok());
        let prepared = Self::prepare_completed_call(frame.complete(value))?;
        if let Err(error) = change {
            let thread = self.current_windows_thread()?.with_last_error(error);
            self.publish_windows_thread(thread)?;
        } else if let Ok(Some(change)) = change {
            self.console.commit(change);
        }
        self.publish_prepared_call(prepared);
        Ok(true)
    }
}
