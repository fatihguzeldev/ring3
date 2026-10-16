mod messages;
pub(super) mod painting;
mod text;
use super::User32Outcome32;
use crate::{
    memory::GuestAddress,
    process::{EngineInstance, HostError, calls::call::PendingOwner},
    windows::{
        CallFrame32,
        controls::{BUILTIN_PROC_A, BUILTIN_PROC_W},
    },
};

impl EngineInstance {
    pub(in crate::process::windows) fn bind_user32_builtin_classes(
        &mut self,
    ) -> Result<(), HostError> {
        if self
            .user32
            .as_ref()
            .is_none_or(|context| context.builtin_procedures.is_some())
        {
            return Ok(());
        }
        let mut procedures = [GuestAddress(0); 2];
        for gate in self
            .loaded_windows_modules()
            .flat_map(|module| module.gates())
        {
            match gate.id {
                BUILTIN_PROC_A => procedures[0] = gate.entry,
                BUILTIN_PROC_W => procedures[1] = gate.entry,
                _ => {}
            }
        }
        if procedures.iter().any(|procedure| procedure.0 == 0) {
            return Ok(());
        }
        let context = self.user32.as_mut().unwrap();
        context
            .state
            .install_standard_classes(procedures[1])
            .map_err(HostError::User32)?;
        context.builtin_procedures = Some(procedures);
        Ok(())
    }
    pub(in crate::process) fn try_complete_builtin_control_call(
        &mut self,
        owner: PendingOwner,
        token: u32,
        id: u32,
        frame: CallFrame32,
    ) -> Result<bool, HostError> {
        if !matches!(id, BUILTIN_PROC_A | BUILTIN_PROC_W) {
            return Ok(false);
        }
        self.validate_windows_provider_owner(owner)?;
        self.checked_pending_call(owner, token)?;
        self.validate_new_windows_reply(owner, token)?;
        self.reserve_windows_reply()?;
        self.check_windows_thread_write()?;
        Self::prepare_completed_call(frame.complete(0))?;
        let call = self.user32_call_context()?;
        let outcome = if matches!(frame.arguments()[1], 0x0f | 0x14) {
            self.execute_builtin_control_paint(frame.arguments())
        } else {
            self.user32
                .as_mut()
                .ok_or(HostError::InvalidRequest)?
                .control_message(
                    frame.arguments(),
                    id == BUILTIN_PROC_W,
                    self.memory.as_mut().ok_or(HostError::Closed)?,
                    call,
                )
                .map_err(HostError::User32)
        };
        let (outcome, error) = match outcome {
            Ok(outcome) => (outcome, None),
            Err(HostError::User32(crate::windows::user32::User32Error32::Win32(error))) => {
                (User32Outcome32::Return(0), Some(error))
            }
            Err(HostError::Gdi(crate::windows::gdi::GdiError32::Memory(
                error @ (crate::memory::MemoryError::Allocation
                | crate::memory::MemoryError::VersionExhausted),
            ))) => return Err(HostError::Memory(error)),
            Err(HostError::Gdi(error)) => (User32Outcome32::Return(0), Some(error.win32())),
            Err(error) => return Err(error),
        };
        self.publish_user32_result(owner, token, frame, outcome, error)?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests;
