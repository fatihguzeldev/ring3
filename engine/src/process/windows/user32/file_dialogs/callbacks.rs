use super::template;
use crate::{
    memory::{AddressSpace, GuestAddress},
    process::windows::user32::{
        User32Context32, User32Outcome32, callback::Continuation32, memory::Scratch32,
    },
    windows::{user32::User32Error32 as E, window::WindowHandle32},
};

#[derive(Clone, Copy)]
pub(super) enum HookStage32 {
    Initialize,
    Initialized,
    Folder,
    Selection,
    Filter,
    FileOk,
    Help,
    HelpOwner,
    Share,
    ShareWarning,
    Message {
        arguments: [u32; 4],
        procedure: GuestAddress,
    },
    ResumeMessage {
        arguments: [u32; 4],
        procedure: GuestAddress,
    },
}
#[derive(Clone, Copy)]
pub(in crate::process::windows::user32) struct FileDialogCall32 {
    pub(super) plan: u64,
    pub(super) stage: HookStage32,
    pub(in crate::process::windows::user32) scratch: Option<Scratch32>,
    issued: bool,
    result: Option<u32>,
    message_result: Option<u32>,
}
impl User32Context32 {
    pub(super) fn file_dialog_hook(
        &mut self,
        plan: u64,
        mut stage: HookStage32,
        memory: &mut AddressSpace,
    ) -> Result<User32Outcome32, E> {
        if self.pending.last().is_some_and(|pending| matches!(pending.continuation,Continuation32::FileDialog(call) if call.plan == plan && !matches!(call.stage,HookStage32::ResumeMessage { .. }) && (!call.issued || call.result.is_some()))) {
            return self.advance_file_dialog_hook(memory);
        }
        let index = self.file_dialogs.index(plan)?;
        let entry = &self.file_dialogs.plans[index];
        let root = entry.root.ok_or(E::InvalidToken)?;
        if matches!(stage, HookStage32::Help)
            && (!entry.explorer || !entry.request.input.flag(0x20) || entry.request.input.hook == 0)
        {
            if entry.request.input.owner == 0 {
                return Ok(User32Outcome32::Return(1));
            }
            stage = HookStage32::HelpOwner;
        }
        if !entry.explorer && matches!(stage, HookStage32::Initialized | HookStage32::Folder) {
            return Ok(User32Outcome32::Return(1));
        }
        if !matches!(stage, HookStage32::HelpOwner)
            && (!entry.request.input.flag(0x20) || entry.request.input.hook == 0)
        {
            if matches!(stage, HookStage32::FileOk) {
                self.end_file_dialog(root, 1)?;
            }
            return Ok(User32Outcome32::Return(1));
        }
        let token = self.check_continuation()?;
        let call = self.prepare_file_hook(
            FileDialogCall32 {
                plan,
                stage,
                scratch: None,
                issued: false,
                result: None,
                message_result: None,
            },
            memory,
        )?;
        let (procedure, arguments) = self
            .file_dialog_callback(call)
            .expect("prepared hook uses the same live plan and child");
        self.next_token = token;
        self.pending.push(super::super::callback::Pending32 {
            token,
            continuation: Continuation32::FileDialog(call),
            activation: None,
        });
        Ok(User32Outcome32::Callback(super::super::User32Callback32 {
            procedure,
            arguments,
            count: 4,
            continuation: token,
        }))
    }
    pub(super) fn prepare_file_dialog_call(
        &mut self,
        plan: u64,
        stage: HookStage32,
        memory: &mut AddressSpace,
    ) -> Result<FileDialogCall32, E> {
        self.prepare_file_hook(
            FileDialogCall32 {
                plan,
                stage,
                scratch: None,
                issued: false,
                result: None,
                message_result: None,
            },
            memory,
        )
    }
    fn prepare_file_hook(
        &mut self,
        mut call: FileDialogCall32,
        memory: &mut AddressSpace,
    ) -> Result<FileDialogCall32, E> {
        self.check_file_dialog_output(call, memory)?;
        let plan = &self.file_dialogs.plans[self.file_dialogs.index(call.plan)?];
        let root = plan.root.ok_or(E::InvalidToken)?;
        if matches!(call.stage, HookStage32::HelpOwner) {
            self.windows()
                .get(WindowHandle32::try_from(plan.request.input.owner)?)?;
            call.issued = true;
            return Ok(call);
        }
        let hook = if plan.explorer {
            self.file_dialog_control(root, template::HOOK)?
        } else {
            root
        };
        self.state.extra(hook, 0)?;
        let scratch = if !plan.explorer
            || matches!(
                call.stage,
                HookStage32::Initialize
                    | HookStage32::Message { .. }
                    | HookStage32::ResumeMessage { .. }
            ) {
            None
        } else {
            let code = match call.stage {
                HookStage32::Initialized => -601i32,
                HookStage32::Folder => -603,
                HookStage32::Selection => -602,
                HookStage32::Filter => -607,
                HookStage32::FileOk => -606,
                HookStage32::Help => -605,
                HookStage32::Share => -604,
                _ => return Err(E::InvalidToken),
            };
            let filename = if matches!(call.stage, HookStage32::Share) {
                let selection = plan.selection.as_ref().ok_or(E::InvalidToken)?;
                super::super::super::commdlg::publication::encode(
                    &selection.path,
                    plan.request.wide,
                    plan.request.ansi,
                )
                .map_err(|error| match error {
                    crate::windows::commdlg::ComdlgError32::Memory(error) => E::Memory(error),
                    crate::windows::commdlg::ComdlgError32::Allocation => E::Allocation,
                    _ => E::InvalidRequest,
                })?
            } else {
                Vec::new()
            };
            let length = 20usize.checked_add(filename.len()).ok_or(E::Capacity)?;
            let mut bytes = Vec::new();
            bytes.try_reserve_exact(length).map_err(|_| E::Allocation)?;
            bytes.resize(20, 0);
            let pointer = if filename.is_empty() {
                0
            } else {
                let pages = u32::try_from(length.div_ceil(4096)).map_err(|_| E::Capacity)?;
                let range = memory.find_free_range(self.scratch_bounds, pages, 4096)?;
                (range.first as u32)
                    .checked_mul(4096)
                    .and_then(|address| address.checked_add(20))
                    .ok_or(E::Capacity)?
            };
            for (index, word) in [root.get(), 0, code as u32, plan.request.pointer, pointer]
                .into_iter()
                .enumerate()
            {
                bytes[index * 4..index * 4 + 4].copy_from_slice(&word.to_le_bytes());
            }
            bytes.extend_from_slice(&filename);
            let scratch = Scratch32::allocate_span(memory, self.scratch_bounds, &bytes)?;
            debug_assert!(pointer == 0 || pointer == scratch.address.0 + 20);
            Some(scratch)
        };
        if !plan.explorer && matches!(call.stage, HookStage32::Share) {
            self.publish_file_dialog_selection(call.plan, memory)?;
        }
        self.state
            .set_extra(hook, 0, 0)
            .expect("hook extra bytes were preflighted without a yield");
        call.scratch = scratch;
        call.issued = true;
        Ok(call)
    }

    pub(in crate::process::windows::user32) fn check_file_dialog_output(
        &self,
        call: FileDialogCall32,
        memory: &AddressSpace,
    ) -> Result<(), E> {
        self.file_dialogs.plans[self.file_dialogs.index(call.plan)?]
            .request
            .check(memory)
            .map_err(|error| match error {
                crate::windows::commdlg::ComdlgError32::Memory(error) => E::Memory(error),
                _ => E::InvalidRequest,
            })
    }
    pub(super) fn retry_file_dialog_hook(
        &mut self,
        plan: u64,
        memory: &mut AddressSpace,
    ) -> Option<Result<User32Outcome32, E>> {
        self.pending.last().is_some_and(|pending| matches!(pending.continuation,Continuation32::FileDialog(call) if call.plan == plan && (!call.issued || call.result.is_some()))).then(|| self.advance_file_dialog_hook(memory))
    }
    pub(in crate::process::windows::user32) fn file_dialog_callback(
        &self,
        call: FileDialogCall32,
    ) -> Result<(GuestAddress, [u32; 4]), E> {
        let plan = &self.file_dialogs.plans[self.file_dialogs.index(call.plan)?];
        let root = plan.root.ok_or(E::InvalidToken)?;
        if matches!(call.stage, HookStage32::HelpOwner) {
            return Ok((
                plan.send_message,
                [
                    plan.request.input.owner,
                    plan.help_message,
                    root.get(),
                    plan.request.pointer,
                ],
            ));
        }
        let hook = if plan.explorer {
            self.file_dialog_control(root, template::HOOK)?
        } else {
            root
        };
        if let HookStage32::ResumeMessage {
            arguments,
            procedure,
        } = call.stage
        {
            return Ok((procedure, arguments));
        }
        if let HookStage32::Message { arguments, .. } = call.stage {
            return Ok((GuestAddress(plan.request.input.hook), arguments));
        }
        let arguments = if !plan.explorer {
            let messages = plan.legacy_messages.ok_or(E::InvalidToken)?;
            match call.stage {
                HookStage32::Initialize => [root.get(), 0x110, 0, plan.request.pointer],
                HookStage32::FileOk => [root.get(), messages[0], 0, plan.request.pointer],
                HookStage32::Share => [root.get(), plan.share_message, 0, plan.request.pointer],
                HookStage32::Selection | HookStage32::Filter => {
                    let id = if matches!(call.stage, HookStage32::Filter) {
                        template::FILTER
                    } else {
                        template::FILES
                    };
                    let window = self.file_dialog_control(root, id)?;
                    let state = self.state.control_state(window)?;
                    let index = state.selected.map_or(u32::MAX, |index| index as u32);
                    let change = if plan.request.input.flag(0x200) {
                        if state.selected.is_some_and(|index| {
                            state.selected_items.get(index).copied().unwrap_or(false)
                        }) {
                            2
                        } else {
                            1
                        }
                    } else {
                        0
                    };
                    [
                        root.get(),
                        messages[1],
                        id,
                        (index & 0xffff) | (change << 16),
                    ]
                }
                _ => return Err(E::InvalidToken),
            }
        } else if matches!(call.stage, HookStage32::Initialize) {
            [hook.get(), 0x110, 0, plan.request.pointer]
        } else {
            [
                hook.get(),
                0x4e,
                0,
                call.scratch.ok_or(E::InvalidToken)?.address.0,
            ]
        };
        Ok((GuestAddress(plan.request.input.hook), arguments))
    }
    pub(in crate::process::windows::user32) fn complete_file_dialog_hook(
        &mut self,
        call: FileDialogCall32,
        result: u32,
        memory: &mut AddressSpace,
    ) -> Result<User32Outcome32, E> {
        if call.issued && call.result.is_none() {
            let plan = &self.file_dialogs.plans[self.file_dialogs.index(call.plan)?];
            let message_result = if plan.explorer
                && matches!(call.stage, HookStage32::Share | HookStage32::FileOk)
            {
                let hook =
                    self.file_dialog_control(plan.root.ok_or(E::InvalidToken)?, template::HOOK)?;
                Some(self.state.extra(hook, 0)?)
            } else {
                None
            };
            let Continuation32::FileDialog(current) =
                &mut self.pending.last_mut().ok_or(E::InvalidToken)?.continuation
            else {
                return Err(E::InvalidToken);
            };
            current.result = Some(result);
            current.message_result = message_result;
        }
        self.advance_file_dialog_hook(memory)
    }
    fn advance_file_dialog_hook(
        &mut self,
        memory: &mut AddressSpace,
    ) -> Result<User32Outcome32, E> {
        loop {
            let pending = self.pending.last().ok_or(E::InvalidToken)?;
            let Continuation32::FileDialog(mut call) = pending.continuation else {
                return Err(E::InvalidToken);
            };
            let index = self.file_dialogs.index(call.plan)?;
            let plan = &self.file_dialogs.plans[index];
            let root = plan.root.ok_or(E::InvalidToken)?;
            if matches!(call.stage, HookStage32::ShareWarning) {
                return self.begin_file_prompt(
                    call.plan,
                    super::prompts::PromptKind32::Invalid(32),
                    None,
                    memory,
                );
            }
            if !matches!(call.stage, HookStage32::HelpOwner)
                && (!plan.request.input.flag(0x20) || plan.request.input.hook == 0)
            {
                if matches!(call.stage, HookStage32::FileOk) {
                    self.end_file_dialog(root, 1)?;
                }
                self.pending.pop();
                return Ok(User32Outcome32::Return(1));
            }
            if !call.issued {
                call = self.prepare_file_hook(call, memory)?;
                self.pending.last_mut().unwrap().continuation = Continuation32::FileDialog(call);
                return self.next_outcome();
            }
            let result = call.result.ok_or(E::InvalidToken)?;
            if let HookStage32::ResumeMessage { .. } = call.stage {
                self.pending.pop();
                return Ok(User32Outcome32::Return(result));
            }
            if let HookStage32::Message {
                arguments,
                procedure,
            } = call.stage
            {
                if result != 0 {
                    self.pending.pop();
                    return Ok(User32Outcome32::Return(result));
                }
                call.stage = HookStage32::ResumeMessage {
                    arguments,
                    procedure,
                };
                call.result = None;
                self.pending.last_mut().unwrap().continuation = Continuation32::FileDialog(call);
                return self.next_outcome();
            }
            if let Some(scratch) = call.scratch {
                scratch.check(memory)?;
            }
            if matches!(call.stage, HookStage32::Share) {
                let action = if plan.explorer {
                    if result == 0 {
                        0
                    } else {
                        call.message_result.ok_or(E::InvalidToken)?
                    }
                } else {
                    result
                };
                if action == 2 {
                    self.publish_file_dialog_selection(call.plan, memory)?;
                }
                if let Some(scratch) = call.scratch {
                    scratch.release(memory)?;
                }
                if action == 1 {
                    self.file_dialogs.plans[index].selection = None;
                    self.file_dialogs.plans[index].published_error = None;
                    self.pending.pop();
                    return Ok(User32Outcome32::Return(1));
                }
                let stage = if action == 2 {
                    HookStage32::FileOk
                } else {
                    self.file_dialogs.plans[index].selection = None;
                    self.file_dialogs.plans[index].published_error = None;
                    HookStage32::ShareWarning
                };
                self.pending.last_mut().unwrap().continuation =
                    Continuation32::FileDialog(FileDialogCall32 {
                        plan: call.plan,
                        stage,
                        scratch: None,
                        issued: false,
                        result: None,
                        message_result: None,
                    });
                continue;
            }
            let veto = matches!(call.stage, HookStage32::FileOk)
                && result != 0
                && (!plan.explorer || call.message_result.ok_or(E::InvalidToken)? != 0);
            // a sealed hook result survives a failed scratch release and is never executed twice.
            if let Some(scratch) = call.scratch {
                scratch.release(memory)?;
            }
            let next = match call.stage {
                HookStage32::Initialize if plan.explorer => Some(HookStage32::Initialized),
                HookStage32::Initialized if plan.explorer => Some(HookStage32::Folder),
                _ => None,
            };
            if let Some(stage) = next {
                self.pending.last_mut().unwrap().continuation =
                    Continuation32::FileDialog(FileDialogCall32 {
                        plan: call.plan,
                        stage,
                        scratch: None,
                        issued: false,
                        result: None,
                        message_result: None,
                    });
                continue;
            }
            if matches!(call.stage, HookStage32::FileOk) {
                if veto {
                    self.file_dialogs.plans[index].published_error = None;
                    self.file_dialogs.plans[index].selection = None;
                } else {
                    self.end_file_dialog(root, 1)?;
                }
            }
            self.pending.pop();
            return Ok(User32Outcome32::Return(1));
        }
    }
    pub(super) fn end_file_dialog(&mut self, root: WindowHandle32, result: u32) -> Result<(), E> {
        let mut dialog = self.state.dialog(root)?;
        dialog.ended = Some(result);
        self.state.set_dialog(root, dialog)
    }
}
