use super::types::Stage32;
use crate::{
    memory::AddressSpace,
    process::windows::user32::{
        User32Context32, User32Outcome32, callback::Continuation32, memory::Scratch32,
    },
    windows::user32::{User32Api32, User32Error32 as E},
};

impl User32Context32 {
    pub(in crate::process::windows::user32) fn resource_dialog_index(
        &self,
        token: u64,
    ) -> Result<usize, E> {
        self.resource_dialogs
            .plans
            .iter()
            .position(|plan| plan.token == token)
            .ok_or(E::InvalidToken)
    }

    pub(in crate::process::windows::user32) fn resource_dialog_scratch(
        &self,
        plan: u64,
    ) -> Result<Scratch32, E> {
        let pending = self
            .pending
            .last()
            .filter(|pending| pending.token == plan)
            .ok_or(E::InvalidToken)?;
        let Continuation32::ResourceDialog { scratch, .. } = pending.continuation else {
            return Err(E::InvalidToken);
        };
        Ok(scratch)
    }

    pub(in crate::process::windows::user32) fn advance_resource_dialog(
        &mut self,
        token: u64,
        ram: &mut AddressSpace,
    ) -> Result<User32Outcome32, E> {
        self.advance_resource_dialog_with_input(token, ram, None)
    }

    pub(in crate::process::windows::user32) fn resume_resource_dialog(
        &mut self,
        token: u64,
        ram: &mut AddressSpace,
    ) -> Result<User32Outcome32, E> {
        let stage = self.resource_dialogs.plans[self.resource_dialog_index(token)?].stage;
        self.advance_resource_dialog_with_input(token, ram, Some(stage))
    }

    fn advance_resource_dialog_with_input(
        &mut self,
        token: u64,
        ram: &mut AddressSpace,
        admitted: Option<Stage32>,
    ) -> Result<User32Outcome32, E> {
        loop {
            let scratch = self.resource_dialog_scratch(token)?;
            scratch.check(ram)?;
            let index = self.resource_dialog_index(token)?;
            let plan = &self.resource_dialogs.plans[index];
            let context = super::super::super::User32CallContext32 {
                now_ns: self.timers.now,
                time_ms: self.timers.tick,
                cursor: self.runtime_input.cursor,
                shift: self.runtime_input.shift,
                control: self.runtime_input.control,
                caps_lock: self.runtime_input.caps_lock,
                ..plan.context
            };
            let root = plan.windows.first().copied();
            let stage = plan.stage;
            let ended = root
                .and_then(|root| self.state.dialog(root).ok())
                .and_then(|dialog| dialog.ended);
            if let Some(result) = ended
                && !matches!(
                    stage,
                    Stage32::Destroy | Stage32::Restore | Stage32::RestoreFocus
                )
            {
                self.resource_dialogs.plans[index].result = result;
                self.resource_dialogs.plans[index].stage = Stage32::Destroy;
                continue;
            }
            let outcome = match stage {
                Stage32::Create(at) => {
                    let parent = if at == 0 {
                        super::super::super::raw(plan.owner)
                    } else {
                        super::super::super::raw(root)
                    };
                    let args = plan.prepared.windows[at].arguments(
                        scratch.address.0,
                        context.instance,
                        parent,
                    )?;
                    let api = if plan.wide {
                        User32Api32::CreateWindowExW
                    } else {
                        User32Api32::CreateWindowExA
                    };
                    self.create_window(api, &args, ram, context)?
                }
                Stage32::Font(at) => {
                    self.send_message(&[plan.windows[at].get(), 0x30, plan.font, 0], context)?
                }
                Stage32::Disable => {
                    if !plan.modal || !plan.owner_enabled || plan.owner_disabled {
                        self.resource_dialogs.plans[index].stage = Stage32::Initialize;
                        continue;
                    }
                    let owner = plan.owner.ok_or(E::InvalidRequest)?;
                    if self.windows().get(owner)?.description.thread_id != plan.thread
                        && admitted != Some(Stage32::Disable)
                    {
                        return Ok(User32Outcome32::Wait {
                            continuation: token,
                        });
                    }
                    let mut description = self.windows().get(owner)?.description;
                    description.enabled = false;
                    self.state
                        .windows_mut()
                        .update_description(owner, description)?;
                    self.resource_dialogs.plans[index].owner_disabled = true;
                    self.send_message(&[owner.get(), 0xa, 0, 0], context)?
                }
                Stage32::Initialize => {
                    if plan.procedure.0 == 0 {
                        self.resource_dialogs.plans[index].stage = Stage32::Focus;
                        continue;
                    }
                    let root = root.ok_or(E::InvalidRequest)?;
                    let first = self.state.dialog_tab_item(root, None, false)?;
                    self.start(Continuation32::Call {
                        procedure: plan.procedure,
                        arguments: [
                            root.get(),
                            0x110,
                            super::super::super::raw(first),
                            plan.parameter,
                        ],
                    })?
                }
                Stage32::Focus => {
                    let Some(root) = root else {
                        self.resource_dialogs.plans[index].stage = Stage32::Destroy;
                        continue;
                    };
                    let first = self.state.dialog_tab_item(root, None, false)?;
                    if first.is_none() || self.windows().focus() == first {
                        self.resource_dialogs.plans[index].stage = Stage32::Show;
                        continue;
                    }
                    if admitted != Some(Stage32::Focus) {
                        return Ok(User32Outcome32::Wait {
                            continuation: token,
                        });
                    }
                    self.set_focus_capture(
                        User32Api32::SetFocus,
                        &[super::super::super::raw(first)],
                        context,
                    )?
                }
                Stage32::Show => {
                    if !plan.prepared.visible && !plan.modal {
                        self.resource_dialogs.plans[index].stage = Stage32::Wait;
                        continue;
                    }
                    self.show_window(&[super::super::super::raw(root), 5], context)?
                }
                Stage32::Wait => {
                    if !plan.modal {
                        self.resource_dialogs.plans[index].result = super::super::super::raw(root);
                        return self.finish_resource_dialog(token, ram);
                    }
                    if root.is_none_or(|root| self.windows().get(root).is_err()) {
                        self.resource_dialogs.plans[index].stage = Stage32::Restore;
                        continue;
                    }
                    if admitted.is_none() || !self.resource_dialog_ready(token) {
                        return Ok(User32Outcome32::Wait {
                            continuation: token,
                        });
                    }
                    self.resource_dialogs.plans[index].stage = Stage32::Retrieve;
                    continue;
                }
                Stage32::Retrieve => {
                    // the modal loop uses the ordinary per-thread message retrieval authority.
                    self.retrieve_message(
                        User32Api32::GetMessageW,
                        &[scratch.address.0, 0, 0, 0, 0],
                        ram,
                        context,
                    )?
                }
                Stage32::Dispatch => {
                    if admitted != Some(Stage32::Dispatch) {
                        return Ok(User32Outcome32::Wait {
                            continuation: token,
                        });
                    }
                    self.check_message_dispatch(scratch.address.0, ram, context)?;
                    self.translate_message(scratch.address.0, ram, context)?;
                    self.dispatch_message(scratch.address.0, ram, context)?
                }
                Stage32::Destroy => {
                    if let Some(root) = root.filter(|&root| self.windows().get(root).is_ok()) {
                        if admitted != Some(Stage32::Destroy) {
                            return Ok(User32Outcome32::Wait {
                                continuation: token,
                            });
                        }
                        self.begin_destroy(root, context.thread_id, 1, None, true, false)?
                    } else {
                        self.resource_dialogs.plans[index].stage = Stage32::Restore;
                        continue;
                    }
                }
                Stage32::Restore => {
                    if !plan.owner_disabled {
                        self.resource_dialogs.plans[index].stage = Stage32::RestoreFocus;
                        continue;
                    }
                    let owner = plan
                        .owner
                        .filter(|&owner| self.windows().get(owner).is_ok());
                    if let Some(owner) = owner {
                        if self.windows().get(owner)?.description.thread_id != plan.thread
                            && admitted != Some(Stage32::Restore)
                        {
                            return Ok(User32Outcome32::Wait {
                                continuation: token,
                            });
                        }
                        let mut description = self.windows().get(owner)?.description;
                        description.enabled = true;
                        self.state
                            .windows_mut()
                            .update_description(owner, description)?;
                        self.resource_dialogs.plans[index].owner_disabled = false;
                        self.send_message(&[owner.get(), 0xa, 1, 0], context)?
                    } else {
                        self.resource_dialogs.plans[index].owner_disabled = false;
                        self.resource_dialogs.plans[index].stage = Stage32::RestoreFocus;
                        continue;
                    }
                }
                Stage32::RestoreFocus => {
                    let previous = plan.previous_focus.filter(|&window| {
                        self.windows()
                            .get(window)
                            .is_ok_and(|record| record.description.enabled)
                    });
                    if previous.is_none() || self.windows().focus() == previous {
                        return self.finish_resource_dialog(token, ram);
                    }
                    if admitted != Some(Stage32::RestoreFocus) {
                        return Ok(User32Outcome32::Wait {
                            continuation: token,
                        });
                    }
                    self.set_focus_capture(
                        User32Api32::SetFocus,
                        &[super::super::super::raw(previous)],
                        context,
                    )?
                }
            };
            return match outcome {
                User32Outcome32::Return(value) => self.resource_action_completed(token, value, ram),
                outcome => self.resource_delegate_outcome(token, outcome),
            };
        }
    }

    pub(in crate::process::windows::user32) fn finish_resource_dialog(
        &mut self,
        token: u64,
        ram: &mut AddressSpace,
    ) -> Result<User32Outcome32, E> {
        let index = self.resource_dialog_index(token)?;
        self.resource_dialog_scratch(token)?.release(ram)?;
        let plan = self.resource_dialogs.plans.remove(index);
        let result = self.file_dialogs.complete(token, plan.result);
        self.pending.pop();
        Ok(User32Outcome32::Return(result))
    }

    pub(in crate::process::windows::user32) fn resource_dialog_ready(&self, token: u64) -> bool {
        self.resource_dialog_index(token).is_ok_and(|index| {
            let plan = &self.resource_dialogs.plans[index];
            matches!(
                plan.stage,
                Stage32::Disable
                    | Stage32::Restore
                    | Stage32::Focus
                    | Stage32::Destroy
                    | Stage32::RestoreFocus
                    | Stage32::Retrieve
                    | Stage32::Dispatch
            ) || plan.windows.first().is_none_or(|&window| {
                self.state.dialog(window).is_err()
                    || self
                        .state
                        .dialog(window)
                        .is_ok_and(|dialog| dialog.ended.is_some())
            }) || self
                .state
                .find_message(plan.thread, 0, 0, 0)
                .is_ok_and(|message| message.is_some())
                || self
                    .state
                    .paint_message(
                        plan.thread,
                        0,
                        0,
                        0,
                        u32::MAX,
                        self.timers.tick,
                        self.runtime_input.cursor,
                    )
                    .is_ok_and(|message| message.is_some())
                || self.timer_message_ready(plan.thread, 0, 0, 0)
        })
    }

    pub(in crate::process::windows::user32) fn pending_resource_dialog_send(
        &self,
        token: u64,
    ) -> Option<[u32; 4]> {
        let pending = self
            .pending
            .last()
            .filter(|pending| pending.token == token)?;
        let Continuation32::ResourceDialog {
            plan, child: None, ..
        } = pending.continuation
        else {
            return None;
        };
        let entry = &self.resource_dialogs.plans[self.resource_dialog_index(plan).ok()?];
        let owner = entry.owner?;
        if self.windows().get(owner).ok()?.description.thread_id == entry.thread {
            return None;
        }
        let enabled = match entry.stage {
            Stage32::Disable if entry.modal && entry.owner_enabled && !entry.owner_disabled => 0,
            Stage32::Restore if entry.owner_disabled => 1,
            _ => return None,
        };
        Some([owner.get(), 0xa, enabled, 0])
    }
}
