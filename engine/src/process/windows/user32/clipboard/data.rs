use super::Intent32;
use crate::{
    process::{CallError, EngineInstance, HostError},
    windows::user32::{
        User32Error32 as Error,
        clipboard::{ClipboardContent32 as Content, ClipboardOwner32},
    },
};
enum SetData32 {
    Ready(u32),
    NeedBytes(crate::files::FileReadRequest32),
}
impl EngineInstance {
    pub(super) fn set_clipboard_data(
        &mut self,
        format: u32,
        handle: u32,
        thread: u32,
    ) -> Result<crate::process::windows::user32::User32Outcome32, HostError> {
        match self.try_set_clipboard_data(format, handle, thread)? {
            SetData32::Ready(value) => {
                if handle != 0 {
                    self.user32.as_mut().unwrap().last_error = Some(0);
                }
                Ok(crate::process::windows::user32::User32Outcome32::Return(
                    value,
                ))
            }
            SetData32::NeedBytes(request) => {
                let outcome = self.user32.as_mut().unwrap().begin_clipboard(
                    Intent32::Set { format, handle },
                    thread,
                    None,
                );
                let outcome = match outcome {
                    Ok(outcome) => outcome,
                    Err(error) => {
                        self.cancel_file_request(request)?;
                        return Err(HostError::User32(error));
                    }
                };
                let crate::process::windows::user32::User32Outcome32::Wait { continuation } =
                    outcome
                else {
                    unreachable!()
                };
                self.user32
                    .as_mut()
                    .unwrap()
                    .set_clipboard_bytes_wait(continuation, true);
                self.park_clipboard_file_bytes(request)?;
                Ok(outcome)
            }
        }
    }
    fn try_set_clipboard_data(
        &mut self,
        format: u32,
        handle: u32,
        thread: u32,
    ) -> Result<SetData32, HostError> {
        let context = self.user32.as_ref().unwrap();
        if context.clipboard.check_open(thread).is_err() && !self.current_clipboard_renderer(format)
        {
            return Err(HostError::User32(Error::Win32(1418)));
        }
        if matches!(format, 3 | 14 | 0x80 | 0x82 | 0x83 | 0x8e) {
            return Err(HostError::User32(Error::Win32(50)));
        }
        let gdi = if handle != 0 && matches!(format, 2 | 9) {
            Some(
                self.gdi
                    .as_ref()
                    .ok_or(HostError::Gdi(
                        crate::windows::gdi::GdiError32::InvalidConfig,
                    ))?
                    .capture_clipboard_object(
                        self.memory.as_ref().ok_or(HostError::Closed)?,
                        format,
                        handle,
                    )
                    .map_err(HostError::Gdi)?,
            )
        } else {
            None
        };
        let owned = if handle == 0 || gdi.is_some() {
            None
        } else {
            Some(self.windows_global_memory(handle)?)
        };
        let mut retired = Vec::new();
        let mut graphics = Vec::new();
        retired
            .try_reserve_exact(context.clipboard.entries().len())
            .map_err(|_| HostError::User32(Error::Allocation))?;
        graphics
            .try_reserve_exact(context.clipboard.entries().len())
            .map_err(|_| HostError::User32(Error::Allocation))?;
        for entry in context
            .clipboard
            .entries()
            .iter()
            .filter(|entry| entry.replaced_by(format))
        {
            if let Some(memory) = entry.content.memory() {
                let elsewhere = context.clipboard.entries().iter().any(|other| {
                    !other.replaced_by(format)
                        && other
                            .content
                            .memory()
                            .is_some_and(|other| other.handle == memory.handle)
                });
                if !elsewhere && owned.is_none_or(|new| new.handle != memory.handle) {
                    retired.push(memory);
                }
            }
            if let Some(object) = entry.content.gdi() {
                let elsewhere =
                    context.clipboard.entries().iter().any(|other| {
                        !other.replaced_by(format) && other.content.gdi() == Some(object)
                    });
                if !elsewhere && gdi != Some(object) {
                    graphics.push(object);
                }
            }
        }
        let globals = self.prepare_windows_global_retirement(&retired)?;
        if let Some(owned) = owned {
            self.check_windows_global_memory(owned)?;
        }
        self.user32
            .as_mut()
            .unwrap()
            .clipboard
            .prepare_set(format)
            .map_err(HostError::User32)?;
        if let Some(request) = self.retire_clipboard_graphics(&graphics)? {
            return Ok(SetData32::NeedBytes(request));
        }
        if let Some(owned) = owned {
            self.commit_windows_global_transfer(owned);
        }
        if let Some(object) = gdi {
            self.gdi.as_mut().unwrap().commit_clipboard_transfer(object);
        }
        self.commit_windows_global_retirement(globals);
        self.user32.as_mut().unwrap().clipboard.commit_set(
            format,
            if let Some(object) = gdi {
                Content::Gdi(object)
            } else {
                owned.map_or(Content::Delayed, Content::Memory)
            },
        );
        Ok(SetData32::Ready(handle))
    }
    pub(in crate::process) fn finish_clipboard_wait(
        &mut self,
        token: u64,
    ) -> Result<bool, HostError> {
        let Some(call) = self
            .user32
            .as_ref()
            .and_then(|context| context.clipboard_wait(token))
        else {
            return Ok(false);
        };
        if call.ready.is_some()
            || call.child.is_some()
            || !call.dispatched && call.message.is_some()
        {
            return Ok(true);
        }
        if call.bytes_wait && !self.clipboard_file_bytes_ready()? {
            return Ok(true);
        }
        if !matches!(call.intent, Intent32::Get(_)) {
            let (owner, token, id, frame) = self.clipboard_file_owner()?;
            if let Some(files) = self.files.as_ref() {
                files.check_gdi_file_call(owner, token, id, frame)?;
            }
        }
        let result = match call.intent {
            Intent32::Empty(owner) => self.empty_clipboard(call.thread, owner),
            Intent32::Get(format) => self.resolve_clipboard_data(format).map(Some),
            Intent32::Set { format, handle } => {
                match self.try_set_clipboard_data(format, handle, call.thread) {
                    Ok(SetData32::Ready(value)) => Ok(Some(value)),
                    Ok(SetData32::NeedBytes(request)) => {
                        self.park_clipboard_file_bytes(request)?;
                        Ok(None)
                    }
                    Err(error) => Err(error),
                }
            }
        };
        let (value, error) = match result {
            Ok(Some(value)) => (
                value,
                if matches!(call.intent,Intent32::Set{handle,..} if handle!=0) {
                    Some(0)
                } else {
                    None
                },
            ),
            Ok(None) => {
                self.user32
                    .as_mut()
                    .unwrap()
                    .set_clipboard_bytes_wait(token, true);
                return Ok(true);
            }
            Err(HostError::User32(Error::Win32(error))) => (0, Some(error)),
            Err(HostError::WindowsMemory(error)) => (0, Some(error.win32())),
            Err(HostError::Gdi(error)) => (0, Some(error.win32())),
            Err(HostError::File(error))
                if !matches!(
                    error,
                    crate::files::FileError32::Allocation | crate::files::FileError32::Exhausted
                ) =>
            {
                (0, Some(error.win32()))
            }
            Err(error) => return Err(error),
        };
        self.finish_clipboard_file_bytes()?;
        self.user32
            .as_mut()
            .unwrap()
            .set_clipboard_bytes_wait(token, false);
        self.user32
            .as_mut()
            .unwrap()
            .publish_clipboard_value(token, value, error);
        Ok(true)
    }
    fn empty_clipboard(
        &mut self,
        thread: u32,
        mut owner: ClipboardOwner32,
    ) -> Result<Option<u32>, HostError> {
        let context = self.user32.as_ref().unwrap();
        let open = context
            .clipboard
            .check_open(thread)
            .map_err(HostError::User32)?;
        if open.window != owner.window {
            return Err(HostError::User32(Error::Win32(1418)));
        }
        if owner
            .window
            .is_some_and(|window| context.windows().get(window).is_err())
        {
            owner.window = None;
            owner.thread = thread;
        }
        let mut retired = Vec::new();
        let mut graphics = Vec::new();
        retired
            .try_reserve_exact(context.clipboard.entries().len())
            .map_err(|_| HostError::User32(Error::Allocation))?;
        graphics
            .try_reserve_exact(context.clipboard.entries().len())
            .map_err(|_| HostError::User32(Error::Allocation))?;
        retired.extend(
            context
                .clipboard
                .entries()
                .iter()
                .filter_map(|entry| entry.content.memory()),
        );
        graphics.extend(
            context
                .clipboard
                .entries()
                .iter()
                .filter_map(|entry| entry.content.gdi()),
        );
        let globals = self.prepare_windows_global_retirement(&retired)?;
        if let Some(request) = self.retire_clipboard_graphics(&graphics)? {
            self.park_clipboard_file_bytes(request)?;
            return Ok(None);
        }
        self.commit_windows_global_retirement(globals);
        let locale = self.windows_thread_locale();
        self.user32
            .as_mut()
            .unwrap()
            .clipboard
            .commit_empty(owner, locale);
        Ok(Some(1))
    }
    pub(super) fn current_clipboard_renderer(&self, format: u32) -> bool {
        let Some(context) = self.user32.as_ref() else {
            return false;
        };
        let Some(window) = context.clipboard.owner().and_then(|owner| owner.window) else {
            return false;
        };
        let local = self.callback.as_ref().is_some_and(|callback| {
            self.windows_replies.iter().any(|reply| {
                reply
                    .active_user32_callback(callback.token())
                    .is_some_and(|child| context.owns_clipboard_render(child, window.get(), format))
            })
        });
        local || self.foreign_clipboard_renderer(window.get(), format)
    }
}

impl EngineInstance {
    fn clipboard_file_owner(
        &self,
    ) -> Result<
        (
            crate::process::calls::call::PendingOwner,
            u32,
            u32,
            crate::windows::CallFrame32,
        ),
        HostError,
    > {
        let continuation = self
            .waiting_user32_message()
            .or_else(|| {
                self.user32
                    .as_ref()
                    .and_then(|context| context.current_clipboard_continuation())
            })
            .ok_or(HostError::Call(CallError::InvalidToken))?;
        let call = self
            .user32
            .as_ref()
            .and_then(|context| context.clipboard_wait(continuation))
            .ok_or(HostError::Call(CallError::InvalidToken))?;
        let pending = self
            .pending_call
            .as_ref()
            .ok_or(HostError::Call(CallError::InvalidToken))?;
        self.checked_pending_call(pending.owner, pending.token)?;
        let id = match call.intent {
            Intent32::Set { format, handle } => {
                if pending.frame.arguments() != [format, handle] {
                    return Err(HostError::Call(CallError::StateChanged));
                }
                crate::windows::user32::User32Api32::SetClipboardData.id()
            }
            Intent32::Empty(_) => crate::windows::user32::User32Api32::EmptyClipboard.id(),
            _ => return Err(HostError::Call(CallError::InvalidRequest)),
        };
        Ok((pending.owner, pending.token, id, pending.frame))
    }
    fn park_clipboard_file_bytes(
        &mut self,
        request: crate::files::FileReadRequest32,
    ) -> Result<(), HostError> {
        let (owner, token, id, frame) = self.clipboard_file_owner()?;
        let files = self.files.as_mut().ok_or(HostError::Infrastructure)?;
        files.check_gdi_file_call(owner, token, id, frame)?;
        files.park_gdi_file_call(owner, token, id, frame, request);
        Ok(())
    }
    fn clipboard_file_bytes_ready(&self) -> Result<bool, HostError> {
        let (owner, token, id, frame) = self.clipboard_file_owner()?;
        let files = self.files.as_ref().ok_or(HostError::Infrastructure)?;
        files.check_gdi_file_call(owner, token, id, frame)?;
        files.gdi_file_request_ready(owner, token)
    }
    fn finish_clipboard_file_bytes(&mut self) -> Result<(), HostError> {
        if self.pending_file_call().is_some() {
            let (owner, token, id, frame) = self.clipboard_file_owner()?;
            let files = self.files.as_mut().unwrap();
            files.check_gdi_file_call(owner, token, id, frame)?;
            files.finish_gdi_file_call();
        }
        Ok(())
    }
    pub(in crate::process) fn resume_clipboard_file_wait(
        &mut self,
        owner: crate::process::calls::call::PendingOwner,
        token: u32,
        frame: crate::windows::CallFrame32,
    ) -> Result<bool, HostError> {
        let Some(continuation) = self.waiting_user32_message() else {
            return Ok(false);
        };
        if self
            .user32
            .as_ref()
            .and_then(|context| context.clipboard_wait(continuation))
            .is_none_or(|call| !call.bytes_wait)
        {
            return Ok(false);
        }
        let actual = self.clipboard_file_owner()?;
        if (actual.0, actual.1, actual.3) != (owner, token, frame) {
            return Err(HostError::Call(CallError::StateChanged));
        }
        self.resume_user32_wait(self.key(), continuation)?;
        Ok(true)
    }
}
