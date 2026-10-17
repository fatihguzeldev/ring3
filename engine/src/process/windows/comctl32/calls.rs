use crate::{
    process::{CallError, EngineInstance, HostError, calls::call::PendingOwner},
    windows::{
        CallFrame32, CallingConvention32,
        comctl32::{COMMON_PROC_A, COMMON_PROC_W, CommonControlsApi32},
        gdi::GdiError32,
        user32::User32Error32,
    },
};

impl EngineInstance {
    pub fn register_comctl32_module(&mut self, preferred_base: u32) -> Result<(), HostError> {
        self.register_system_module(crate::windows::comctl32::comctl32_module(preferred_base))
    }

    pub(in crate::process) fn try_complete_common_controls_call(
        &mut self,
        owner: PendingOwner,
        token: u32,
        id: u32,
        frame: CallFrame32,
    ) -> Result<bool, HostError> {
        let api = CommonControlsApi32::from_id(id);
        if api.is_none() && !matches!(id, COMMON_PROC_A | COMMON_PROC_W) {
            return Ok(false);
        }
        self.validate_windows_provider_owner(owner)?;
        self.checked_pending_call(owner, token)?;
        if frame.convention() != CallingConvention32::Stdcall
            || frame.stack_words() != api.map_or(4, CommonControlsApi32::stack_words)
        {
            return Err(HostError::Call(CallError::InvalidRequest));
        }
        self.validate_new_windows_reply(owner, token)?;
        self.reserve_windows_reply()?;
        self.check_windows_thread_write()?;
        Self::prepare_completed_call(frame.complete(0))?;
        let result = match api {
            Some(CommonControlsApi32::InitCommonControlsEx) => {
                self.initialize_common_controls(frame.arguments()[0])
            }
            Some(api) => self.create_status_window(
                frame.arguments(),
                api == CommonControlsApi32::CreateStatusWindowW,
            ),
            None => self.execute_common_control_procedure(frame.arguments(), id == COMMON_PROC_W),
        };
        let (outcome, error) = match result {
            Ok(outcome) => (outcome, None),
            Err(HostError::User32(User32Error32::Win32(error))) => (
                super::super::user32::User32Outcome32::Return(0),
                Some(error),
            ),
            Err(HostError::Gdi(GdiError32::Memory(
                error @ (crate::memory::MemoryError::Allocation
                | crate::memory::MemoryError::VersionExhausted),
            ))) => return Err(HostError::Memory(error)),
            Err(HostError::Gdi(error)) => (
                super::super::user32::User32Outcome32::Return(0),
                Some(error.win32()),
            ),
            Err(error) => return Err(error),
        };
        self.publish_user32_result(owner, token, frame, outcome, error)?;
        Ok(true)
    }
}
