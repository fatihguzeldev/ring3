use super::{
    ReplyStage32, WINDOWS_CALLBACK_ARGUMENTS32, WindowsCallback32, WindowsCallbackProvider32,
    WindowsProviderOutcome32, WindowsReply32, validate_request,
};
use crate::{
    process::{
        CallError, EngineInstance, HostError, calls::call::PendingOwner,
        windows::user32::User32Outcome32,
    },
    windows::CallFrame32,
};

impl EngineInstance {
    pub(in crate::process) fn retry_windows_callback_reply(
        &mut self,
        owner: PendingOwner,
        token: u32,
    ) -> Result<bool, HostError> {
        let Some(reply) = self
            .windows_replies
            .last()
            .copied()
            .filter(|reply| reply.owner == owner && reply.token == token)
        else {
            return Ok(false);
        };
        self.checked_pending_call(owner, token)?;
        match reply.stage {
            ReplyStage32::Received(_) => self.resume_windows_reply()?,
            ReplyStage32::Complete(_) => self.publish_windows_reply()?,
            _ => {}
        }
        Ok(true)
    }

    pub(super) fn resume_windows_reply(&mut self) -> Result<(), HostError> {
        let reply = *self
            .windows_replies
            .last()
            .ok_or(HostError::Call(CallError::InvalidToken))?;
        self.checked_pending_call(reply.owner, reply.token)?;
        let ReplyStage32::Received(result) = reply.stage else {
            return Err(HostError::Call(CallError::InvalidToken));
        };
        self.check_windows_thread_write()?;
        match reply.provider {
            WindowsCallbackProvider32::User32 => {
                let flags = self.user32.as_ref().and_then(|context| {
                    context.preview_completion_input_flags(reply.continuation, result)
                });
                let input = flags
                    .map(|(focused, captured)| self.prepare_windows_input_flags(focused, captured))
                    .transpose()?
                    .flatten();
                let context = self.user32.as_mut().ok_or(HostError::InvalidRequest)?;
                let outcome = context
                    .complete(
                        reply.continuation,
                        result,
                        self.memory.as_mut().ok_or(HostError::Closed)?,
                    )
                    .map_err(HostError::User32)?;
                let error = context.take_last_error();
                let actual = (
                    context.windows().focus().is_some(),
                    context.windows().capture().is_some(),
                );
                self.commit_windows_input_flags(if flags == Some(actual) { input } else { None });
                self.accept_user32_outcome(outcome, error)?;
            }
            WindowsCallbackProvider32::D3D8 => {
                let outcome = self.resume_d3d8_callback(reply.continuation, result)?;
                let last = self.windows_replies.last_mut().unwrap();
                match outcome {
                    WindowsProviderOutcome32::Return(value) => {
                        last.stage = ReplyStage32::Complete(value)
                    }
                    WindowsProviderOutcome32::Callback(request) => {
                        validate_request(request)?;
                        if request.provider != reply.provider {
                            return Err(HostError::Infrastructure);
                        }
                        last.continuation = request.continuation;
                        last.stage = ReplyStage32::Ready(request);
                    }
                }
            }
        }
        if self
            .windows_replies
            .last()
            .is_some_and(|reply| matches!(reply.stage, ReplyStage32::Complete(_)))
        {
            self.publish_windows_reply()?;
        }
        Ok(())
    }

    fn publish_windows_reply(&mut self) -> Result<(), HostError> {
        let reply = *self
            .windows_replies
            .last()
            .ok_or(HostError::Call(CallError::InvalidToken))?;
        let ReplyStage32::Complete(result) = reply.stage else {
            return Err(HostError::Call(CallError::InvalidToken));
        };
        self.checked_pending_call(reply.owner, reply.token)?;
        let prepared = Self::prepare_completed_call(reply.frame.complete(result))?;
        if let Some(error) = reply.last_error {
            let thread = self.current_windows_thread()?.with_last_error(error);
            self.publish_windows_thread(thread)?;
        }
        self.publish_prepared_call(prepared);
        self.windows_replies.pop();
        Ok(())
    }

    pub(in crate::process::windows) fn publish_user32_result(
        &mut self,
        owner: PendingOwner,
        token: u32,
        frame: CallFrame32,
        outcome: User32Outcome32,
        error: Option<u32>,
    ) -> Result<(), HostError> {
        self.windows_replies.push(WindowsReply32 {
            owner,
            token,
            frame,
            provider: WindowsCallbackProvider32::User32,
            continuation: 0,
            last_error: None,
            stage: ReplyStage32::Complete(0),
        });
        self.accept_user32_outcome(outcome, error)?;
        if self
            .windows_replies
            .last()
            .is_some_and(|reply| matches!(reply.stage, ReplyStage32::Complete(_)))
        {
            self.publish_windows_reply()?;
        }
        Ok(())
    }

    pub fn waiting_user32_message(&self) -> Option<u64> {
        let reply = self.windows_replies.last()?;
        if !matches!(reply.stage, ReplyStage32::Wait)
            || reply.provider != WindowsCallbackProvider32::User32
        {
            return None;
        }
        self.checked_pending_call(reply.owner, reply.token).ok()?;
        Some(reply.continuation)
    }

    pub fn resume_user32_wait(&mut self, key: u64, continuation: u64) -> Result<(), HostError> {
        if key != self.key {
            return Err(HostError::InvalidArtifact);
        }
        let reply = *self
            .windows_replies
            .last()
            .ok_or(HostError::Call(CallError::InvalidToken))?;
        if !matches!(reply.stage, ReplyStage32::Wait)
            || reply.provider != WindowsCallbackProvider32::User32
            || reply.continuation != continuation
        {
            return Err(HostError::Call(CallError::InvalidToken));
        }
        self.checked_pending_call(reply.owner, reply.token)?;
        self.check_windows_thread_write()?;
        let context = self.user32.as_mut().ok_or(HostError::InvalidRequest)?;
        let outcome = context
            .resume_wait(continuation, self.memory.as_mut().ok_or(HostError::Closed)?)
            .map_err(HostError::User32)?;
        let error = context.take_last_error();
        self.accept_user32_outcome(outcome, error)?;
        if self
            .windows_replies
            .last()
            .is_some_and(|reply| matches!(reply.stage, ReplyStage32::Complete(_)))
        {
            self.publish_windows_reply()?;
        }
        Ok(())
    }

    pub(in crate::process::windows) fn accept_user32_outcome(
        &mut self,
        outcome: User32Outcome32,
        error: Option<u32>,
    ) -> Result<(), HostError> {
        let reply = self
            .windows_replies
            .last_mut()
            .ok_or(HostError::Infrastructure)?;
        if error.is_some() {
            reply.last_error = error;
        }
        match outcome {
            User32Outcome32::Return(result) => reply.stage = ReplyStage32::Complete(result),
            User32Outcome32::Wait { continuation } => {
                reply.continuation = continuation;
                reply.stage = ReplyStage32::Wait;
            }
            User32Outcome32::Callback(callback) => {
                let mut arguments = [0; WINDOWS_CALLBACK_ARGUMENTS32];
                arguments[..4].copy_from_slice(&callback.arguments);
                let request = WindowsCallback32 {
                    procedure: callback.procedure,
                    arguments,
                    count: 4,
                    continuation: callback.continuation,
                    provider: WindowsCallbackProvider32::User32,
                };
                reply.continuation = callback.continuation;
                reply.stage = ReplyStage32::Ready(request);
            }
        }
        Ok(())
    }
}
