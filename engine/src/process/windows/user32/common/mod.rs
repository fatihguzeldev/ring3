pub(super) mod animate;
pub(super) mod header;
mod hotkey;
pub(super) mod layout;
pub(super) mod listview;
pub(super) mod numeric;
mod painting;
mod progress;
pub(super) mod status;
pub(super) mod tab;
mod toolbar;
pub(super) mod tooltip;
pub(super) mod trackbar;
pub(super) mod updown;

use super::{User32Outcome32, memory};
use crate::{
    memory::GuestAddress,
    process::{EngineInstance, HostError},
    windows::comctl32::{COMMON_PROC_A, COMMON_PROC_W},
};

impl EngineInstance {
    pub(in crate::process::windows) fn initialize_common_controls(
        &mut self,
        address: u32,
    ) -> Result<User32Outcome32, HostError> {
        let [size, flags] =
            memory::words::<2>(self.memory.as_ref().ok_or(HostError::Closed)?, address)
                .map_err(HostError::User32)?;
        if size != 8 {
            return Ok(User32Outcome32::Return(0));
        }
        if flags & 0xffff & !0xfd != 0 {
            return Err(HostError::User32(
                crate::windows::user32::User32Error32::Win32(50),
            ));
        }
        if flags & 0xfd == 0 {
            return Ok(User32Outcome32::Return(1));
        }
        self.bind_common_control_classes(flags)?;
        Ok(User32Outcome32::Return(1))
    }

    pub(in crate::process::windows) fn bind_common_control_classes(
        &mut self,
        flags: u32,
    ) -> Result<(), HostError> {
        let mut procedures = [GuestAddress(0); 2];
        for gate in self
            .loaded_windows_modules()
            .flat_map(|module| module.gates())
        {
            match gate.id {
                COMMON_PROC_A => procedures[0] = gate.entry,
                COMMON_PROC_W => procedures[1] = gate.entry,
                _ => {}
            }
        }
        if procedures.iter().any(|entry| entry.0 == 0) {
            return Err(HostError::InvalidRequest);
        }
        let retain = self
            .user32
            .as_ref()
            .ok_or(HostError::InvalidRequest)?
            .common_procedures
            .is_none();
        let (instance, reference) = self
            .prepare_module_handle_reference(None, Some(procedures[1]), if retain { 4 } else { 6 })?
            .ok_or(HostError::InvalidRequest)?;
        let context = self.user32.as_mut().ok_or(HostError::InvalidRequest)?;
        context
            .state
            .install_common_classes(flags, procedures[1], instance)
            .map_err(HostError::User32)?;
        context.common_procedures = Some(procedures);
        if retain {
            self.commit_module_handle_reference(reference);
        }
        Ok(())
    }

    pub(in crate::process::windows) fn execute_common_control_procedure(
        &mut self,
        args: &[u32],
        wide: bool,
    ) -> Result<User32Outcome32, HostError> {
        let call = self.user32_call_context()?;
        let queue = self.prepare_user32_queue_change_for(call.thread_id)?;
        if let Some(outcome) = self.begin_common_animation_open(args, call, wide)? {
            self.commit_user32_queue_change(queue);
            return Ok(outcome);
        }
        let result = self
            .user32
            .as_mut()
            .ok_or(HostError::InvalidRequest)?
            .common_control_message(
                args,
                wide,
                self.memory.as_mut().ok_or(HostError::Closed)?,
                self.gdi.as_mut(),
                call,
            )?;
        self.commit_user32_queue_change(queue);
        Ok(result)
    }
}

impl EngineInstance {
    pub(in crate::process::windows::user32) fn bind_named_common_control_class(
        &mut self,
        api: crate::windows::user32::User32Api32,
        args: &[u32],
    ) -> Result<(), HostError> {
        use crate::windows::{comctl32::CommonControlKind32, user32::User32Api32 as Api};
        if !matches!(api, Api::CreateWindowExA | Api::CreateWindowExW)
            || args[1] < 0x10000
            || !self
                .loaded_windows_modules()
                .flat_map(|module| module.gates())
                .any(|gate| gate.id == COMMON_PROC_W)
        {
            return Ok(());
        }
        let name = memory::string(
            self.memory()?,
            args[1],
            api.wide(),
            256,
            self.windows_ansi_code_page(),
        )
        .map_err(HostError::User32)?;
        if let Some(kind) = CommonControlKind32::ALL.into_iter().find(|kind| {
            name.len() == kind.name().len()
                && name
                    .iter()
                    .copied()
                    .zip(kind.name().bytes())
                    .all(|(unit, byte)| unit <= 127 && (unit as u8).eq_ignore_ascii_case(&byte))
        }) {
            self.bind_common_control_classes(kind.flags())?;
        }
        Ok(())
    }
}
