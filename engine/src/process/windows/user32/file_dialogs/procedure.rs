use super::{Selection32, callbacks::HookStage32, prompts::PromptKind32, template};
use crate::{
    process::{EngineInstance, HostError, windows::user32::User32Outcome32},
    windows::{
        user32::{User32Error32 as E, dialogs::WindowDialog32},
        window::WindowHandle32,
    },
};
impl EngineInstance {
    pub(in crate::process::windows) fn execute_common_file_dialog_procedure(
        &mut self,
        args: &[u32],
        _wide: bool,
    ) -> Result<User32Outcome32, HostError> {
        let thread = self.current_windows_thread_id()?;
        let root = WindowHandle32::try_from(args[0])
            .map_err(E::from)
            .map_err(HostError::User32)?;
        let context = self.user32.as_ref().ok_or(HostError::InvalidRequest)?;
        context.owned(args[0], thread).map_err(HostError::User32)?;
        if args[1] == 0x110 {
            let index = context
                .file_dialogs
                .plans
                .iter()
                .rposition(|plan| {
                    plan.thread == thread
                        && (plan.root == Some(root)
                            || plan.root.is_none() && plan.request.pointer == args[3])
                })
                .ok_or(E::InvalidToken)
                .map_err(HostError::User32)?;
            let token = context.file_dialogs.plans[index].token;
            if context.file_dialogs.plans[index].initialized {
                return self
                    .user32
                    .as_mut()
                    .unwrap()
                    .file_dialog_hook(
                        token,
                        HookStage32::Initialize,
                        self.memory.as_mut().ok_or(HostError::Closed)?,
                    )
                    .map_err(HostError::User32);
            }
            let directory = context.file_dialogs.plans[index].directory.clone();
            self.set_provider_current_directory(&directory)?;
            let context = self.user32.as_mut().unwrap();
            context.file_dialogs.plans[index].root = Some(root);
            context
                .populate_file_dialog(token)
                .map_err(HostError::User32)?;
            let plan = &context.file_dialogs.plans[index];
            if plan.request.input.flag(0x20) && plan.request.input.hook != 0 {
                let hook = context
                    .file_dialog_control(root, template::HOOK)
                    .map_err(HostError::User32)?;
                context
                    .state
                    .set_dialog(
                        hook,
                        WindowDialog32 {
                            base: context.state.dialog(root).map_err(HostError::User32)?.base,
                            font: context.state.dialog(root).map_err(HostError::User32)?.font,
                            modal: false,
                            ended: None,
                        },
                    )
                    .map_err(HostError::User32)?;
                context
                    .state
                    .set_extra(hook, 4, plan.request.input.hook)
                    .map_err(HostError::User32)?;
            }
            context.file_dialogs.plans[index].initialized = true;
            return context
                .file_dialog_hook(
                    token,
                    HookStage32::Initialize,
                    self.memory.as_mut().ok_or(HostError::Closed)?,
                )
                .map_err(HostError::User32);
        }
        let index = context
            .file_dialogs
            .window(root)
            .map_err(HostError::User32)?;
        let token = context.file_dialogs.plans[index].token;
        if let Some(outcome) = self
            .user32
            .as_mut()
            .unwrap()
            .retry_file_dialog_hook(token, self.memory.as_mut().ok_or(HostError::Closed)?)
        {
            return outcome.map_err(HostError::User32);
        }
        if let Some(outcome) = self
            .user32
            .as_mut()
            .unwrap()
            .common_dialog_message(root, args, self.memory.as_mut().ok_or(HostError::Closed)?)
            .map_err(HostError::User32)?
        {
            return Ok(outcome);
        }
        let id = args[2] & 0xffff;
        let code = args[2] >> 16;
        if args[1] == 0x10 || args[1] == 0x111 && id == 2 {
            self.user32
                .as_mut()
                .unwrap()
                .end_file_dialog(root, 0)
                .map_err(HostError::User32)?;
            self.set_common_dialog_error(0)?;
            return Ok(User32Outcome32::Return(1));
        }
        if args[1] != 0x111 {
            return Ok(User32Outcome32::Return(0));
        }
        if id == template::FILTER && code == 1 {
            let context = self.user32.as_ref().unwrap();
            let plan = &context.file_dialogs.plans[index];
            let combo = context
                .file_dialog_control(root, template::FILTER)
                .map_err(HostError::User32)?;
            let selected = context
                .state
                .control_state(combo)
                .map_err(HostError::User32)?
                .selected
                .unwrap_or(0) as u32;
            let selected = if plan.request.filters.custom.is_some() {
                selected
            } else {
                selected + 1
            };
            let directory = plan.directory.clone();
            let entries = self.common_dialog_entries(&directory, &plan.request, selected)?;
            let context = self.user32.as_mut().unwrap();
            context
                .replace_file_dialog_entries(token, directory, entries, selected)
                .map_err(HostError::User32)?;
            return context
                .file_dialog_hook(token, HookStage32::Filter, self.memory.as_mut().unwrap())
                .map_err(HostError::User32);
        }
        if id == template::FILES && matches!(code, 1 | 2) {
            let context = self.user32.as_ref().unwrap();
            let files = context
                .file_dialog_control(root, template::FILES)
                .map_err(HostError::User32)?;
            let selected = context
                .state
                .control_state(files)
                .map_err(HostError::User32)?
                .selected;
            if let Some(selected) = selected {
                let entry = context.file_dialogs.plans[index]
                    .entries
                    .get(selected)
                    .ok_or(E::InvalidRequest)
                    .map_err(HostError::User32)?;
                let path = entry.path.clone();
                let name = entry.name.clone();
                let directory = entry.directory;
                if code == 2 && directory {
                    return self.change_common_dialog_directory(token, path);
                }
                if !directory {
                    let context = self.user32.as_mut().unwrap();
                    let edit = context
                        .file_dialog_control(root, template::NAME)
                        .map_err(HostError::User32)?;
                    context
                        .state
                        .set_text(edit, name)
                        .map_err(HostError::User32)?;
                    context
                        .state
                        .invalidate_control(edit)
                        .map_err(HostError::User32)?;
                    if code == 2 {
                        return self.accept_common_dialog_selection(token);
                    }
                }
            }
            return self
                .user32
                .as_mut()
                .unwrap()
                .file_dialog_hook(token, HookStage32::Selection, self.memory.as_mut().unwrap())
                .map_err(HostError::User32);
        }
        if id == 1 {
            return self.accept_common_dialog_selection(token);
        }
        Ok(User32Outcome32::Return(0))
    }
    fn change_common_dialog_directory(
        &mut self,
        token: u64,
        directory: Vec<u16>,
    ) -> Result<User32Outcome32, HostError> {
        let context = self.user32.as_ref().unwrap();
        let index = context
            .file_dialogs
            .index(token)
            .map_err(HostError::User32)?;
        let plan = &context.file_dialogs.plans[index];
        let entries = self.common_dialog_entries(&directory, &plan.request, plan.filter_index)?;
        let filter = plan.filter_index;
        self.set_provider_current_directory(&directory)?;
        let context = self.user32.as_mut().unwrap();
        context
            .replace_file_dialog_entries(token, directory, entries, filter)
            .map_err(HostError::User32)?;
        context
            .file_dialog_hook(token, HookStage32::Folder, self.memory.as_mut().unwrap())
            .map_err(HostError::User32)
    }
    fn accept_common_dialog_selection(&mut self, token: u64) -> Result<User32Outcome32, HostError> {
        let context = self.user32.as_ref().unwrap();
        let index = context
            .file_dialogs
            .index(token)
            .map_err(HostError::User32)?;
        let plan = &context.file_dialogs.plans[index];
        if let Some(error) = plan.published_error {
            self.set_common_dialog_error(error)?;
            return self
                .user32
                .as_mut()
                .unwrap()
                .file_dialog_hook(token, HookStage32::FileOk, self.memory.as_mut().unwrap())
                .map_err(HostError::User32);
        }
        let root = plan
            .root
            .ok_or(E::InvalidToken)
            .map_err(HostError::User32)?;
        let edit = context
            .file_dialog_control(root, template::NAME)
            .map_err(HostError::User32)?;
        let text = context
            .state
            .text(edit)
            .map_err(HostError::User32)?
            .to_vec();
        if text.is_empty() {
            return Ok(User32Outcome32::Return(1));
        }
        let raw = match crate::files::path::FilePathScope32::from_directory(plan.directory.clone())
            .normalize(&text)
        {
            Ok(path) => path,
            Err(error) => {
                return self
                    .user32
                    .as_mut()
                    .unwrap()
                    .begin_file_prompt(
                        token,
                        PromptKind32::Invalid(error.win32()),
                        None,
                        self.memory.as_mut().unwrap(),
                    )
                    .map_err(HostError::User32);
            }
        };
        if self
            .provider_path_info(&raw)
            .is_ok_and(|info| info.attributes & 0x10 != 0)
        {
            return self.change_common_dialog_directory(token, raw);
        }
        let path = match self.common_dialog_selection(token, &text) {
            Ok(path) => path,
            Err(HostError::File(error))
                if !matches!(
                    error,
                    crate::files::FileError32::Allocation
                        | crate::files::FileError32::Capacity
                        | crate::files::FileError32::Exhausted
                ) =>
            {
                return self
                    .user32
                    .as_mut()
                    .unwrap()
                    .begin_file_prompt(
                        token,
                        PromptKind32::Invalid(error.win32()),
                        None,
                        self.memory.as_mut().unwrap(),
                    )
                    .map_err(HostError::User32);
            }
            Err(error) => return Err(error),
        };
        let context = self.user32.as_ref().unwrap();
        let plan = &context.file_dialogs.plans[index];
        let readonly = context
            .state
            .dialog_item(root, template::READ_ONLY)
            .map_err(HostError::User32)?
            .map(|window| {
                context
                    .state
                    .control_state(window)
                    .map(|state| state.check != 0)
            })
            .transpose()
            .map_err(HostError::User32)?
            .unwrap_or(plan.request.input.flag(1));
        let exists = self.provider_path_info(&path).is_ok();
        let prompt = if plan.request.input.flag(2) && plan.save && exists {
            Some(PromptKind32::Overwrite)
        } else if plan.request.input.flag(0x2000) && !exists {
            Some(PromptKind32::Create)
        } else {
            None
        };
        let selection = Selection32 { path, readonly };
        if let Some(kind) = prompt {
            return self
                .user32
                .as_mut()
                .unwrap()
                .begin_file_prompt(token, kind, Some(selection), self.memory.as_mut().unwrap())
                .map_err(HostError::User32);
        }
        let plan = &self.user32.as_ref().unwrap().file_dialogs.plans[index];
        let error = plan
            .request
            .publish(
                self.memory.as_mut().unwrap(),
                &selection.path,
                plan.filter_index,
                selection.readonly,
            )
            .map_err(HostError::Comdlg)?;
        self.user32.as_mut().unwrap().file_dialogs.plans[index].selection = Some(selection);
        self.user32.as_mut().unwrap().file_dialogs.plans[index].published_error = Some(error);
        self.set_common_dialog_error(error)?;
        self.user32
            .as_mut()
            .unwrap()
            .file_dialog_hook(token, HookStage32::FileOk, self.memory.as_mut().unwrap())
            .map_err(HostError::User32)
    }
}
