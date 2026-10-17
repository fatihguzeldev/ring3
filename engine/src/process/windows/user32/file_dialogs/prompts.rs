use super::{Selection32, callbacks::HookStage32};
use crate::{
    memory::AddressSpace,
    process::windows::user32::{
        User32Context32, User32Outcome32, callback::Continuation32, dialogs::Dialog32,
    },
    windows::user32::User32Error32 as E,
};
#[derive(Clone, Copy)]
pub(super) enum PromptKind32 {
    Invalid(u32),
    Overwrite,
    Create,
}
#[derive(Clone, Copy)]
pub(in crate::process::windows::user32) struct FilePrompt32 {
    plan: u64,
    kind: PromptKind32,
    child: Option<u64>,
}
impl User32Context32 {
    pub(super) fn begin_file_prompt(
        &mut self,
        plan: u64,
        kind: PromptKind32,
        selection: Option<Selection32>,
        ram: &mut AddressSpace,
    ) -> Result<User32Outcome32, E> {
        let index = self.file_dialogs.index(plan)?;
        let entry = &self.file_dialogs.plans[index];
        let root = entry.root.ok_or(E::InvalidToken)?;
        self.prepare_thread_slots()?;
        if self.pending.len() + 2 > self.continuation_limit
            || self.dialogs.len() >= self.continuation_limit
            || self.next_token.checked_add(2).is_none()
        {
            return Err(E::Capacity);
        }
        self.dialogs.try_reserve(1).map_err(|_| E::Allocation)?;
        let entry = &self.file_dialogs.plans[index];
        let path = selection
            .as_ref()
            .map(|selection| String::from_utf16_lossy(&selection.path))
            .unwrap_or_default();
        let text: Vec<u16> = match kind {
            PromptKind32::Overwrite => format!("{path} already exists. Replace it?"),
            PromptKind32::Create => format!("{path} does not exist. Create it?"),
            PromptKind32::Invalid(code) => {
                format!("The selected path cannot be used (error {code}).")
            }
        }
        .encode_utf16()
        .collect();
        if text.len() > 32767 {
            return Err(E::Capacity);
        }
        let dialog = Dialog32 {
            id: 0,
            thread: entry.thread,
            owner: root.get(),
            style: if matches!(kind, PromptKind32::Invalid(_)) {
                0x30
            } else {
                0x34
            },
            language: 0,
            help_context: 0,
            help_callback: 0,
            instance: 0,
            caption: if entry.save { "Save As" } else { "Open" }
                .encode_utf16()
                .collect(),
            text,
            icon_id: None,
            icon_name: Vec::new(),
            result: None,
            help: false,
        };
        let token = self.check_continuation()?;
        self.next_token = token;
        self.pending.push(super::super::callback::Pending32 {
            token,
            continuation: Continuation32::FilePrompt(FilePrompt32 {
                plan,
                kind,
                child: None,
            }),
            activation: None,
        });
        self.file_dialogs.plans[index].selection = selection;
        let outcome = self.begin_dialog(dialog)?;
        let child = match outcome {
            User32Outcome32::Wait { continuation } => continuation,
            _ => return Err(E::InvalidRequest),
        };
        self.publish_file_prompt_child(token, child);
        let _ = ram;
        Ok(outcome)
    }
    fn publish_file_prompt_child(&mut self, token: u64, child: u64) {
        let pending = self
            .pending
            .iter_mut()
            .find(|pending| pending.token == token)
            .expect("prepared file prompt parent");
        let Continuation32::FilePrompt(call) = &mut pending.continuation else {
            unreachable!()
        };
        call.child = Some(child);
    }
    pub(in crate::process::windows::user32) fn resume_file_prompt_dialog(
        &mut self,
        token: u64,
        ram: &mut AddressSpace,
    ) -> Option<Result<User32Outcome32, E>> {
        let child = self.pending.len().checked_sub(1)?;
        let parent = child.checked_sub(1)?;
        let Continuation32::Dialog { id } = self.pending[child].continuation else {
            return None;
        };
        let Continuation32::FilePrompt(call) = self.pending[parent].continuation else {
            return None;
        };
        if self.pending[child].token != token || call.child != Some(token) {
            return None;
        }
        let index = self.dialogs.iter().position(|dialog| dialog.id == id)?;
        let (result, error) = self.dialogs[index].result?;
        Some(self.finish_file_prompt(parent, child, index, call, (result, error), ram))
    }
    fn finish_file_prompt(
        &mut self,
        parent: usize,
        child: usize,
        dialog: usize,
        call: FilePrompt32,
        result: (u32, u32),
        ram: &mut AddressSpace,
    ) -> Result<User32Outcome32, E> {
        let index = self.file_dialogs.index(call.plan)?;
        if matches!(call.kind, PromptKind32::Invalid(_)) || result.0 != 6 || result.1 != 0 {
            self.file_dialogs.plans[index].selection = None;
            self.dialogs.swap_remove(dialog);
            self.pending.remove(child);
            self.pending.remove(parent);
            if result.1 != 0 {
                self.last_error = Some(result.1);
            }
            return Ok(User32Outcome32::Return(1));
        }
        self.publish_file_dialog_selection(call.plan, ram)?;
        let entry = &self.file_dialogs.plans[index];
        let root = entry.root.ok_or(E::InvalidToken)?;
        let hook = entry.request.input.flag(0x20) && entry.request.input.hook != 0;
        if hook {
            let prepared = self.prepare_file_dialog_call(call.plan, HookStage32::FileOk, ram)?;
            self.pending[parent].continuation = Continuation32::FileDialog(prepared);
            self.dialogs.swap_remove(dialog);
            self.pending.remove(child);
            self.next_outcome()
        } else {
            self.end_file_dialog(root, 1)?;
            self.dialogs.swap_remove(dialog);
            self.pending.remove(child);
            self.pending.remove(parent);
            Ok(User32Outcome32::Return(1))
        }
    }
    pub(super) fn publish_file_dialog_selection(
        &mut self,
        plan: u64,
        ram: &mut AddressSpace,
    ) -> Result<(), E> {
        let index = self.file_dialogs.index(plan)?;
        let entry = &self.file_dialogs.plans[index];
        if entry.published_error.is_some() {
            return Ok(());
        }
        let selection = entry.selection.as_ref().ok_or(E::InvalidToken)?;
        let error = selection
            .publish(&entry.request, ram, entry.filter_index)
            .map_err(|error| match error {
                crate::windows::commdlg::ComdlgError32::Memory(error) => E::Memory(error),
                crate::windows::commdlg::ComdlgError32::Allocation => E::Allocation,
                _ => E::InvalidRequest,
            })?;
        self.file_dialogs.plans[index].published_error = Some(error);
        Ok(())
    }
}
