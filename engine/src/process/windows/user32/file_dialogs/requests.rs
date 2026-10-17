use super::{FileDialog32, template};
use crate::{
    memory::GuestAddress,
    process::{
        EngineInstance, HostError,
        windows::{
            commdlg::CapturedFileDialog32,
            user32::{User32Outcome32, dialogs::resources::InternalDialog32},
        },
    },
    windows::{
        commdlg::{ComdlgApi32 as Api, DIALOG_PROCEDURE_A, DIALOG_PROCEDURE_W},
        user32::User32Error32 as E,
    },
};

impl EngineInstance {
    pub(in crate::process::windows) fn begin_common_file_dialog(
        &mut self,
        api: Api,
        request: CapturedFileDialog32,
        owner: crate::process::calls::call::PendingOwner,
        call_token: u32,
    ) -> Result<User32Outcome32, HostError> {
        self.bind_user32_builtin_classes()?;
        let context = self.user32_call_context()?;
        let user32 = self.user32.as_ref().ok_or(HostError::InvalidRequest)?;
        if user32.file_dialogs.plans.len() == user32.file_dialogs.limit {
            return Err(HostError::User32(E::Capacity));
        }
        let token = user32.check_continuation().map_err(HostError::User32)?;
        // unsupported flags are rejected before any window, output or directory publication.
        if request.input.flags & (0x40 | 0x80 | 0x10 | 0x200 | 0x8000 | 0x400000) != 0
            || request.input.flag(0x20) && !request.input.flag(0x80000)
            || request.input.flags_ex != 0
        {
            return Err(HostError::User32(E::Win32(50)));
        }
        let directory = self.initial_common_dialog_directory(&request)?;
        let entries =
            self.common_dialog_entries(&directory, &request, request.input.filter_index)?;
        let procedure_id = if api.wide() {
            DIALOG_PROCEDURE_W
        } else {
            DIALOG_PROCEDURE_A
        };
        let procedure = self
            .loaded_windows_modules()
            .flat_map(|module| module.gates())
            .find(|gate| gate.id == procedure_id)
            .map(|gate| gate.entry)
            .ok_or(HostError::InvalidRequest)?;
        let title = if request.title.is_empty() {
            if api.save() { "Save As" } else { "Open" }
                .encode_utf16()
                .collect()
        } else {
            request.title.clone()
        };
        let creation = self.prepare_internal_dialog(
            InternalDialog32 {
                template: template::template(
                    title,
                    api.save(),
                    !request.input.flag(4),
                    request.input.flag(0x20) && request.input.hook != 0,
                ),
                owner: request.input.owner,
                procedure: GuestAddress(procedure.0),
                parameter: request.pointer,
                wide: true,
            },
            context,
        )?;
        let original_directory = self
            .configured_file_current_directory()
            .ok_or(HostError::InvalidRequest)?
            .to_vec();
        let plan = FileDialog32 {
            token,
            call_owner: owner,
            call_token,
            original_directory,
            completed: false,
            thread: context.thread_id,
            root: None,
            save: api.save(),
            initialized: false,
            published_error: None,
            selection: None,
            directory,
            entries,
            filter_index: request.input.filter_index,
            request,
        };
        let user32 = self.user32.as_mut().unwrap();
        // the preallocated plan and resource dialog share the canonical modal continuation token.
        user32.file_dialogs.plans.push(plan);
        let outcome =
            user32.begin_resource_dialog(creation, self.memory.as_mut().ok_or(HostError::Closed)?);
        if outcome.is_err() {
            user32.file_dialogs.retire(token);
        }
        outcome.map_err(HostError::User32)
    }
}

impl EngineInstance {
    pub(in crate::process) fn finish_commdlg_publication(
        &mut self,
        owner: crate::process::calls::call::PendingOwner,
        token: u32,
    ) -> Result<(), HostError> {
        let Some(context) = self.user32.as_ref() else {
            return Ok(());
        };
        let Some(plan) =
            context.file_dialogs.plans.iter().find(|plan| {
                plan.completed && plan.call_owner == owner && plan.call_token == token
            })
        else {
            return Ok(());
        };
        let modal = plan.token;
        let error = plan.published_error.unwrap_or(0);
        if plan.save && plan.request.input.flag(8) {
            let original = plan.original_directory.clone();
            self.set_provider_current_directory(&original)?;
        }
        self.set_common_dialog_error(error)?;
        self.user32.as_mut().unwrap().file_dialogs.retire(modal);
        Ok(())
    }
}
