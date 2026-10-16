use super::{
    requests::Creation32,
    types::{Plan32, Stage32},
};
use crate::{
    memory::{AddressSpace, GuestAddress},
    process::windows::user32::{
        User32CallContext32, User32Context32, User32Outcome32,
        callback::{Continuation32, Pending32},
        memory::Scratch32,
    },
    windows::{
        user32::{User32Api32, User32Error32 as E, WindowClass32, dialogs::WindowDialog32},
        window::WindowHandle32,
    },
};

impl User32Context32 {
    pub(in crate::process::windows::user32) fn begin_resource_dialog(
        &mut self,
        input: Creation32,
        memory: &mut AddressSpace,
    ) -> Result<User32Outcome32, E> {
        let Creation32 {
            prepared,
            args,
            wide,
            modal,
            font,
            context,
        } = input;
        self.prepare_thread_slots()?;
        if self.pending.len() + 2 > self.continuation_limit
            || self.resource_dialogs.plans.len() == self.resource_dialogs.limit
            || self.next_token.checked_add(2).is_none()
            || (font != 0 && self.resource_dialogs.fonts.len() == self.state.window_limit())
        {
            return Err(E::Capacity);
        }
        self.state.check_window_capacity(prepared.windows.len())?;
        let owner = if args[2] == 0 {
            None
        } else {
            Some(self.owned(args[2], context.thread_id)?)
        };
        let owner_enabled = owner.is_some_and(|window| {
            self.windows()
                .get(window)
                .is_ok_and(|record| record.description.enabled)
        });
        let mut windows = Vec::new();
        windows
            .try_reserve_exact(prepared.windows.len())
            .map_err(|_| E::Allocation)?;
        let token = self.check_continuation()?;
        memory.ensure_transaction_versions(3)?;
        let scratch = Scratch32::allocate(memory, self.scratch_bounds, &prepared.bytes)?;
        let context = User32CallContext32 {
            instance: if args[0] == 0 {
                context.instance
            } else {
                args[0]
            },
            ..context
        };
        let first = prepared.windows[0].arguments(scratch.address.0, context.instance, args[2]);
        let first = match first {
            Ok(first) => first,
            Err(error) => {
                scratch.release(memory)?;
                return Err(error);
            }
        };
        let plan = Plan32 {
            token,
            thread: context.thread_id,
            context,
            prepared,
            windows,
            procedure: GuestAddress(args[3]),
            parameter: args[4],
            owner,
            owner_enabled,
            owner_disabled: false,
            previous_focus: self.windows().focus(),
            modal,
            wide,
            font,
            stage: Stage32::Create(0),
            result: if modal { u32::MAX } else { 0 },
        };
        self.resource_dialogs.plans.push(plan);
        if font != 0 {
            self.resource_dialogs
                .fonts
                .push(super::types::FontLease32 { window: None, font });
        }
        self.next_token = token;
        self.pending.push(Pending32 {
            token,
            continuation: Continuation32::ResourceDialog {
                plan: token,
                scratch,
                child: None,
                completed: None,
            },
            activation: None,
        });
        let api = if wide {
            User32Api32::CreateWindowExW
        } else {
            User32Api32::CreateWindowExA
        };
        match self.create_window(api, &first, memory, context) {
            Ok(outcome) => self.resource_delegate_outcome(token, outcome),
            Err(error) => {
                self.pending.pop();
                self.resource_dialogs.plans.pop();
                if font != 0 {
                    self.resource_dialogs.fonts.pop();
                }
                scratch.release(memory)?;
                Err(error)
            }
        }
    }

    pub(in crate::process::windows::user32) fn install_dialog_class(
        &mut self,
        procedures: [GuestAddress; 2],
    ) -> Result<(), E> {
        self.state.install_system_class(WindowClass32 {
            name: "#32770".encode_utf16().collect(),
            procedure: procedures[1],
            unicode: true,
            extra_bytes: 30,
            style: 0x800,
            background_brush: 16,
            ..Default::default()
        })?;
        self.resource_dialogs.procedures = Some(procedures);
        Ok(())
    }

    pub(in crate::process::windows::user32) fn resource_delegate_outcome(
        &mut self,
        plan: u64,
        outcome: User32Outcome32,
    ) -> Result<User32Outcome32, E> {
        let child = match outcome {
            User32Outcome32::Callback(callback) => callback.continuation,
            User32Outcome32::Wait { continuation } => continuation,
            User32Outcome32::Return(_) => return Err(E::InvalidRequest),
        };
        let pending = self
            .pending
            .iter_mut()
            .find(|pending| pending.token == plan)
            .ok_or(E::InvalidToken)?;
        let Continuation32::ResourceDialog {
            child: delegated,
            completed,
            ..
        } = &mut pending.continuation
        else {
            return Err(E::InvalidToken);
        };
        *delegated = Some(child);
        *completed = None;
        Ok(outcome)
    }

    pub(in crate::process::windows::user32) fn accept_resource_delegate(
        &mut self,
        plan: u64,
        child: u64,
        result: u32,
        memory: &mut AddressSpace,
    ) -> Result<User32Outcome32, E> {
        let pending = self
            .pending
            .last_mut()
            .filter(|pending| pending.token == plan)
            .ok_or(E::InvalidToken)?;
        let Continuation32::ResourceDialog {
            child: delegated,
            completed,
            ..
        } = &mut pending.continuation
        else {
            return Err(E::InvalidToken);
        };
        if *delegated != Some(child) {
            return Err(E::InvalidToken);
        }
        *delegated = None;
        *completed = Some(child);
        self.resource_action_completed(plan, result, memory)
    }

    pub(in crate::process::windows::user32) fn resource_action_completed(
        &mut self,
        plan: u64,
        result: u32,
        memory: &mut AddressSpace,
    ) -> Result<User32Outcome32, E> {
        let index = self.resource_dialog_index(plan)?;
        let entry = &mut self.resource_dialogs.plans[index];
        match entry.stage {
            Stage32::Create(at) => {
                let window = WindowHandle32::try_from(result)
                    .ok()
                    .filter(|&window| self.state.windows().get(window).is_ok());
                if let Some(window) = window {
                    entry.windows.push(window);
                    self.state
                        .set_window_context_help(window, entry.prepared.windows[at].help)?;
                    if at == 0 {
                        self.state.set_dialog(
                            window,
                            WindowDialog32 {
                                base: entry.prepared.base,
                                font: entry.font,
                                modal: entry.modal,
                                ended: None,
                            },
                        )?;
                        self.state.set_extra(window, 4, entry.procedure.0)?;
                        if entry.font != 0 {
                            self.resource_dialogs
                                .fonts
                                .iter_mut()
                                .find(|lease| lease.font == entry.font)
                                .expect("accepted dialog retains its prepared template font")
                                .window = Some(window);
                        }
                    }
                    entry.stage = if at + 1 < entry.prepared.windows.len() {
                        Stage32::Create(at + 1)
                    } else if entry.font != 0 {
                        Stage32::Font(0)
                    } else {
                        Stage32::Disable
                    };
                } else {
                    entry.stage = Stage32::Destroy;
                }
            }
            Stage32::Font(at) => {
                entry.stage = if at + 1 < entry.windows.len() {
                    Stage32::Font(at + 1)
                } else {
                    Stage32::Disable
                };
            }
            Stage32::Disable => entry.stage = Stage32::Initialize,
            Stage32::Initialize => {
                entry.stage = if result != 0 {
                    Stage32::Focus
                } else {
                    Stage32::Show
                };
            }
            Stage32::Focus => entry.stage = Stage32::Show,
            Stage32::Show => entry.stage = Stage32::Wait,
            Stage32::Retrieve => {
                entry.stage = if result == 0 {
                    Stage32::Destroy
                } else {
                    Stage32::Dispatch
                };
            }
            Stage32::Dispatch => entry.stage = Stage32::Wait,
            Stage32::Destroy => entry.stage = Stage32::Restore,
            Stage32::Restore => entry.stage = Stage32::RestoreFocus,
            Stage32::RestoreFocus => return self.finish_resource_dialog(plan, memory),
            Stage32::Wait => return Err(E::InvalidToken),
        }
        self.advance_resource_dialog(plan, memory)
    }
}
