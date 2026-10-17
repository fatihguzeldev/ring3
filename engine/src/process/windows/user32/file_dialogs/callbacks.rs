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
}
#[derive(Clone, Copy)]
pub(in crate::process::windows::user32) struct FileDialogCall32 {
    pub(super) plan: u64,
    pub(super) stage: HookStage32,
    pub(in crate::process::windows::user32) scratch: Option<Scratch32>,
    issued: bool,
    result: Option<u32>,
}
impl User32Context32 {
    pub(super) fn file_dialog_hook(
        &mut self,
        plan: u64,
        stage: HookStage32,
        memory: &mut AddressSpace,
    ) -> Result<User32Outcome32, E> {
        if self.pending.last().is_some_and(|pending| matches!(pending.continuation,Continuation32::FileDialog(call) if call.plan == plan)) {
            return self.advance_file_dialog_hook(memory);
        }
        let index = self.file_dialogs.index(plan)?;
        let entry = &self.file_dialogs.plans[index];
        let root = entry.root.ok_or(E::InvalidToken)?;
        if !entry.request.input.flag(0x20) || entry.request.input.hook == 0 {
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
        let hook = self.file_dialog_control(root, template::HOOK)?;
        self.state.extra(hook, 0)?;
        let scratch = if matches!(call.stage, HookStage32::Initialize) {
            None
        } else {
            let code = match call.stage {
                HookStage32::Initialized => -601i32,
                HookStage32::Folder => -603,
                HookStage32::Selection => -602,
                HookStage32::Filter => -607,
                HookStage32::FileOk => -606,
                HookStage32::Initialize => unreachable!(),
            };
            let mut bytes = [0; 20];
            for (index, word) in [root.get(), 0, code as u32, plan.request.pointer, 0]
                .into_iter()
                .enumerate()
            {
                bytes[index * 4..index * 4 + 4].copy_from_slice(&word.to_le_bytes());
            }
            Some(Scratch32::allocate(memory, self.scratch_bounds, &bytes)?)
        };
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
        let hook = self.file_dialog_control(root, template::HOOK)?;
        let arguments = if matches!(call.stage, HookStage32::Initialize) {
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
            let Continuation32::FileDialog(current) =
                &mut self.pending.last_mut().ok_or(E::InvalidToken)?.continuation
            else {
                return Err(E::InvalidToken);
            };
            current.result = Some(result);
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
            if !plan.request.input.flag(0x20) || plan.request.input.hook == 0 {
                if matches!(call.stage, HookStage32::FileOk) {
                    self.end_file_dialog(root, 1)?;
                }
                self.pending.pop();
                return Ok(User32Outcome32::Return(1));
            }
            let hook = self.file_dialog_control(root, template::HOOK)?;
            if !call.issued {
                call = self.prepare_file_hook(call, memory)?;
                self.pending.last_mut().unwrap().continuation = Continuation32::FileDialog(call);
                return self.next_outcome();
            }
            let result = call.result.ok_or(E::InvalidToken)?;
            if let Some(scratch) = call.scratch {
                scratch.check(memory)?;
            }
            let veto = matches!(call.stage, HookStage32::FileOk)
                && result != 0
                && self.state.extra(hook, 0)? != 0;
            // a sealed hook result survives a failed scratch release and is never executed twice.
            if let Some(scratch) = call.scratch {
                scratch.release(memory)?;
            }
            let next = match call.stage {
                HookStage32::Initialize => Some(HookStage32::Initialized),
                HookStage32::Initialized => Some(HookStage32::Folder),
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
