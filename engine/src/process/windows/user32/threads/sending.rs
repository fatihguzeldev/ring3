use super::super::{User32CallContext32, User32Context32, callback::Continuation32};
use crate::{
    memory::GuestAddress,
    process::{
        EngineInstance, HostError,
        windows::threads::{PreparedThreadInvocation32, ThreadInvocationCompletion32},
    },
    windows::{
        CallFrame32,
        user32::{User32Api32, User32Error32},
        window::WindowHandle32,
    },
};
impl EngineInstance {
    pub(in crate::process) fn prepare_user32_wait_send(
        &self,
        token: u64,
    ) -> Result<Option<(Option<PreparedThreadInvocation32>, u64)>, HostError> {
        let context = self.user32.as_ref().ok_or(HostError::InvalidRequest)?;
        let Some(args) = context
            .pending_user32_wait_send_message(token)
            .map_err(HostError::User32)?
        else {
            return Ok(None);
        };
        let sender = self.current_windows_thread_id()?;
        let child = context.check_continuation().map_err(HostError::User32)?;
        let window = WindowHandle32::try_from(args[0])
            .map_err(User32Error32::from)
            .map_err(HostError::User32)?;
        let record = context
            .windows()
            .get(window)
            .map_err(User32Error32::from)
            .map_err(HostError::User32)?;
        let prepared = if record.description.thread_id != sender {
            Some(self.prepare_thread_invocation(
                record.description.thread_id,
                record.description.procedure,
                &args,
                ThreadInvocationCompletion32::User32 {
                    sender,
                    continuation: child,
                },
            )?)
        } else {
            None
        };
        Ok(Some((prepared, child)))
    }
}

impl EngineInstance {
    pub(in crate::process::windows::user32) fn prepare_user32_remote_call(
        &mut self,
        api: User32Api32,
        frame: &CallFrame32,
        context: User32CallContext32,
    ) -> Result<Option<(PreparedThreadInvocation32, u64)>, HostError> {
        let Some(args) = api.sent_message(frame.arguments()) else {
            return Ok(None);
        };
        if frame.convention() != api.convention() || frame.stack_words() != api.stack_words() {
            return Err(HostError::User32(User32Error32::InvalidRequest));
        }
        if args[1] > 0xffff {
            return Ok(None);
        }
        let Some(window) = WindowHandle32::try_from(args[0])
            .ok()
            .and_then(|handle| self.user32.as_ref()?.windows().get(handle).ok())
        else {
            return Ok(None);
        };
        if window.description.thread_id == context.thread_id {
            return Ok(None);
        }
        let Ok(continuation) = self.user32.as_ref().unwrap().check_continuation() else {
            return Ok(None);
        };
        let prepared = self.prepare_thread_invocation(
            window.description.thread_id,
            GuestAddress(window.description.procedure.0),
            &args,
            ThreadInvocationCompletion32::User32 {
                sender: context.thread_id,
                continuation,
            },
        )?;
        Ok(Some((prepared, continuation)))
    }
}
impl User32Context32 {
    pub(in crate::process) fn pending_user32_wait_send_message(
        &self,
        token: u64,
    ) -> Result<Option<[u32; 4]>, User32Error32> {
        if self.ui_wait(token).is_some() {
            return Ok(self.pending_ui_message(token));
        }
        if self.system_colors_wait(token).is_some() {
            return Ok(self.pending_system_colors_message(token));
        }
        if self.clipboard_wait(token).is_some() {
            return Ok(self.pending_clipboard_message(token));
        }
        if self.sent_paint_wait(token).is_some() {
            return self.pending_sent_paint_message(token);
        }
        if matches!(
            self.pending.last().map(|pending| pending.continuation),
            Some(Continuation32::MenuPopup { .. })
        ) {
            return self.pending_popup_message(token);
        }
        self.pending_accelerator_message(token)
    }

    pub(in crate::process) fn waiting_retrieval(&self, continuation: u64) -> bool {
        self.pending.last().is_some_and(|pending| {
            pending.token == continuation
                && matches!(
                    pending.continuation,
                    Continuation32::ResourceDialog { .. }
                        | Continuation32::MenuPopup { .. }
                        | Continuation32::Accelerator(_)
                        | Continuation32::Clipboard(_)
                        | Continuation32::SystemColors(_)
                        | Continuation32::UiState(_)
                        | Continuation32::SentPaint(_)
                        | Continuation32::Retrieve { .. }
                        | Continuation32::MessageWait { .. }
                )
        })
    }
    pub(in crate::process::windows::user32) fn defer_retrieval(
        &mut self,
        api: User32Api32,
        frame: &CallFrame32,
        context: User32CallContext32,
    ) -> Result<super::super::User32Outcome32, User32Error32> {
        if frame.convention() != api.convention() || frame.stack_words() != api.stack_words() {
            return Err(User32Error32::InvalidRequest);
        }
        let mut arguments = [0; 5];
        arguments[..frame.arguments().len()].copy_from_slice(frame.arguments());
        if arguments[2] > 0xffff
            || arguments[3] > 0xffff
            || arguments[3] != 0 && arguments[2] > arguments[3]
        {
            return Err(User32Error32::Win32(87));
        }
        self.prepare_thread_slots()?;
        self.prepare_thread_layout(context.thread_id)?;
        self.start(Continuation32::Retrieve {
            api,
            arguments,
            thread: context.thread_id,
        })
    }
}
