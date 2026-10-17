use super::Intent32;
use crate::{
    process::{EngineInstance, HostError, windows::user32::User32Outcome32},
    windows::user32::{
        User32Error32 as Error,
        clipboard::{ClipboardContent32 as Content, ClipboardOwner32},
    },
};
impl EngineInstance {
    pub(super) fn set_clipboard_data(
        &mut self,
        format: u32,
        handle: u32,
        thread: u32,
    ) -> Result<User32Outcome32, HostError> {
        let context = self.user32.as_ref().unwrap();
        if context.clipboard.check_open(thread).is_err() && !self.current_clipboard_renderer(format)
        {
            return Err(HostError::User32(Error::Win32(1418)));
        }
        if matches!(format, 2 | 3 | 9 | 14 | 0x80 | 0x82 | 0x83 | 0x8e) {
            return Err(HostError::User32(Error::Win32(50)));
        }
        let owned = if handle == 0 {
            None
        } else {
            Some(self.windows_global_memory(handle)?)
        };
        let mut retired = Vec::new();
        retired
            .try_reserve_exact(context.clipboard.entries().len())
            .map_err(|_| HostError::User32(Error::Allocation))?;
        for memory in context.clipboard.replaced_memory(format) {
            let retained_elsewhere = context.clipboard.entries().iter().any(|entry| {
                !entry.replaced_by(format)
                    && entry
                        .content
                        .memory()
                        .is_some_and(|other| other.handle == memory.handle)
            });
            if !retained_elsewhere && owned.is_none_or(|new| new.handle != memory.handle) {
                retired.push(memory)
            }
        }
        let retirement = self.prepare_windows_global_retirement(&retired)?;
        if let Some(owned) = owned {
            self.check_windows_global_memory(owned)?;
        }
        self.user32
            .as_mut()
            .unwrap()
            .clipboard
            .prepare_set(format)
            .map_err(HostError::User32)?;
        if let Some(owned) = owned {
            self.commit_windows_global_transfer(owned);
        }
        self.commit_windows_global_retirement(retirement);
        let context = self.user32.as_mut().unwrap();
        context
            .clipboard
            .commit_set(format, owned.map_or(Content::Delayed, Content::Memory));
        if handle != 0 {
            context.last_error = Some(0);
        }
        Ok(User32Outcome32::Return(handle))
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
        let result = match call.intent {
            Intent32::Empty(owner) => self.empty_clipboard(call.thread, owner),
            Intent32::Get(format) => self.resolve_clipboard_data(format),
        };
        let (value, error) = match result {
            Ok(value) => (value, None),
            Err(HostError::User32(Error::Win32(error))) => (0, Some(error)),
            Err(HostError::WindowsMemory(error)) => (0, Some(error.win32())),
            Err(error) => return Err(error),
        };
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
    ) -> Result<u32, HostError> {
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
        retired
            .try_reserve_exact(context.clipboard.entries().len())
            .map_err(|_| HostError::User32(Error::Allocation))?;
        retired.extend(
            context
                .clipboard
                .entries()
                .iter()
                .filter_map(|entry| entry.content.memory()),
        );
        let retirement = self.prepare_windows_global_retirement(&retired)?;
        self.commit_windows_global_retirement(retirement);
        let locale = self.windows_thread_locale();
        self.user32
            .as_mut()
            .unwrap()
            .clipboard
            .commit_empty(owner, locale);
        Ok(1)
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
