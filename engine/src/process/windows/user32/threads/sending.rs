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
    pub(in crate::process::windows::user32) fn prepare_user32_remote_call(
        &mut self,
        api: User32Api32,
        frame: &CallFrame32,
        context: User32CallContext32,
    ) -> Result<Option<(PreparedThreadInvocation32, u64)>, HostError> {
        if !matches!(api, User32Api32::SendMessageA | User32Api32::SendMessageW) {
            return Ok(None);
        }
        if frame.convention() != api.convention() || frame.stack_words() != api.stack_words() {
            return Err(HostError::User32(User32Error32::InvalidRequest));
        }
        let args = frame.arguments();
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
            &args[..4],
            ThreadInvocationCompletion32::User32 {
                sender: context.thread_id,
                continuation,
            },
        )?;
        Ok(Some((prepared, continuation)))
    }
}
impl User32Context32 {
    pub(in crate::process) fn waiting_retrieval(&self, continuation: u64) -> bool {
        self.pending.last().is_some_and(|pending| {
            pending.token == continuation
                && matches!(
                    pending.continuation,
                    Continuation32::Retrieve { .. } | Continuation32::MessageWait { .. }
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
