use super::{Dialog32, User32DialogRequest32};
use crate::{
    process::windows::user32::{User32Context32, User32Outcome32, callback::Continuation32},
    process::{EngineInstance, HostError},
    windows::{user32::User32Error32 as E, window::WindowHandle32},
};
impl User32Context32 {
    pub(in crate::process::windows::user32) fn begin_dialog(
        &mut self,
        mut dialog: Dialog32,
    ) -> Result<User32Outcome32, E> {
        if dialog.owner != 0 {
            self.windows()
                .get(WindowHandle32::try_from(dialog.owner)?)?;
        }
        let token = self.check_continuation()?;
        if self.dialogs.len() >= self.continuation_limit {
            return Err(E::Capacity);
        }
        self.dialogs.try_reserve(1).map_err(|_| E::Allocation)?;
        self.prepare_thread_slots()?;
        self.runtime_input.thread_id = dialog.thread;
        dialog.id = token;
        self.dialogs.push(dialog);
        self.start(Continuation32::Dialog { id: token })
    }
    pub(in crate::process::windows::user32) fn dialog_blocks_window(&self, raw: u32) -> bool {
        let Ok(handle) = WindowHandle32::try_from(raw) else {
            return false;
        };
        let Ok(window) = self.windows().get(handle) else {
            return false;
        };
        self.dialogs.iter().any(|dialog| {
            if dialog.result.is_some() {
                return false;
            }
            if dialog.owner == 0 {
                return dialog.style & 0x3000 == 0x2000
                    && window.description.thread_id == dialog.thread;
            }
            let mut current = Some(handle);
            for _ in 0..=self.timers.window_limit {
                let Some(handle) = current else {
                    break;
                };
                if handle.get() == dialog.owner {
                    return true;
                }
                current = self
                    .state
                    .record(handle)
                    .ok()
                    .and_then(|record| record.parent);
            }
            false
        })
    }
    pub(in crate::process::windows::user32) fn dialog_ready(&self, id: u64) -> bool {
        self.dialogs
            .iter()
            .find(|dialog| dialog.id == id)
            .is_none_or(|dialog| dialog.result.is_some() || dialog.help)
    }
}
impl EngineInstance {
    pub fn user32_dialog_request(
        &self,
        index: u32,
    ) -> Result<Option<User32DialogRequest32<'_>>, HostError> {
        let epoch = self.memory()?.identity();
        Ok(self
            .user32
            .as_ref()
            .ok_or(HostError::User32(E::InvalidRequest))?
            .dialogs
            .iter()
            .filter(|dialog| dialog.result.is_none())
            .nth(index as usize)
            .map(|dialog| dialog.request(epoch)))
    }
    pub fn complete_user32_dialog(
        &mut self,
        epoch: u64,
        id: u64,
        button: u32,
        error: u32,
    ) -> Result<(), HostError> {
        if self.memory()?.identity() != epoch {
            return Err(HostError::User32(E::InvalidToken));
        }
        let dialog = self
            .user32
            .as_mut()
            .ok_or(HostError::User32(E::InvalidRequest))?
            .dialogs
            .iter_mut()
            .find(|dialog| dialog.id == id)
            .ok_or(HostError::User32(E::InvalidToken))?;
        if dialog.result.is_some()
            || dialog.help
            || !((error == 0 && dialog.buttons().contains(&button)) || (error != 0 && button == 0))
        {
            return Err(HostError::User32(E::InvalidRequest));
        }
        dialog.result = Some((button, error));
        Ok(())
    }
    pub fn request_user32_dialog_help(&mut self, epoch: u64, id: u64) -> Result<(), HostError> {
        if self.memory()?.identity() != epoch {
            return Err(HostError::User32(E::InvalidToken));
        }
        let context = self
            .user32
            .as_mut()
            .ok_or(HostError::User32(E::InvalidRequest))?;
        let index = context
            .dialogs
            .iter()
            .position(|dialog| dialog.id == id)
            .ok_or(HostError::User32(E::InvalidToken))?;
        let dialog = &context.dialogs[index];
        if dialog.style & 0x4000 == 0 || dialog.result.is_some() || dialog.help {
            return Err(HostError::User32(E::InvalidRequest));
        }
        if dialog.owner != 0 {
            context
                .windows()
                .get(
                    WindowHandle32::try_from(dialog.owner)
                        .map_err(|error| HostError::User32(error.into()))?,
                )
                .map_err(|error| HostError::User32(error.into()))?;
        }
        context.dialogs[index].help = true;
        Ok(())
    }
}
