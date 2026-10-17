use super::{ClipboardCall32, Intent32};
use crate::{
    process::windows::user32::{
        User32CallContext32, User32Context32, User32Outcome32, callback::Continuation32,
    },
    windows::user32::User32Error32 as Error,
};

impl User32Context32 {
    pub(super) fn begin_clipboard(
        &mut self,
        intent: Intent32,
        thread: u32,
        message: Option<[u32; 4]>,
    ) -> Result<User32Outcome32, Error> {
        self.next_token.checked_add(2).ok_or(Error::Capacity)?;
        self.prepare_thread_slots()?;
        if self.pending.len() + 2 > self.continuation_limit {
            return Err(Error::Capacity);
        }
        self.start(Continuation32::Clipboard(ClipboardCall32 {
            intent,
            thread,
            message,
            child: None,
            completed: None,
            dispatched: false,
            ready: None,
            bytes_wait: false,
        }))
    }

    pub(in crate::process::windows::user32) fn clipboard_wait(
        &self,
        token: u64,
    ) -> Option<ClipboardCall32> {
        let pending = self
            .pending
            .last()
            .filter(|pending| pending.token == token)?;
        let Continuation32::Clipboard(call) = pending.continuation else {
            return None;
        };
        Some(call)
    }

    pub(in crate::process) fn pending_clipboard_message(&self, token: u64) -> Option<[u32; 4]> {
        self.clipboard_wait(token)
            .filter(|call| !call.dispatched && call.child.is_none())
            .and_then(|call| call.message)
    }

    pub(in crate::process::windows::user32) fn resume_clipboard(
        &mut self,
        token: u64,
    ) -> Result<User32Outcome32, Error> {
        let mut call = self.clipboard_wait(token).ok_or(Error::InvalidToken)?;
        if let Some((value, error)) = call.ready {
            self.pending.pop();
            self.last_error = error;
            return Ok(User32Outcome32::Return(value));
        }
        if let Some(message) = self.pending_clipboard_message(token) {
            let outcome = self.send_message(
                &message,
                User32CallContext32 {
                    thread_id: call.thread,
                    ..self.runtime_input
                },
            )?;
            call.child = Some(match outcome {
                User32Outcome32::Callback(callback) => callback.continuation,
                User32Outcome32::Wait { continuation } => continuation,
                User32Outcome32::Return(_) => return Err(Error::InvalidRequest),
            });
            call.dispatched = true;
            self.pending
                .iter_mut()
                .find(|pending| pending.token == token)
                .unwrap()
                .continuation = Continuation32::Clipboard(call);
            return Ok(outcome);
        }
        Ok(User32Outcome32::Wait {
            continuation: token,
        })
    }

    pub(in crate::process::windows::user32) fn complete_clipboard_child(
        &mut self,
        child: u64,
        outcome: User32Outcome32,
    ) -> Result<User32Outcome32, Error> {
        if !matches!(outcome, User32Outcome32::Return(_)) {
            return Ok(outcome);
        }
        let Some(pending) = self.pending.last_mut() else {
            return Ok(outcome);
        };
        let Continuation32::Clipboard(mut call) = pending.continuation else {
            return Ok(outcome);
        };
        if call.child != Some(child) {
            return Ok(outcome);
        }
        call.child = None;
        call.completed = Some(child);
        pending.continuation = Continuation32::Clipboard(call);
        Ok(User32Outcome32::Wait {
            continuation: pending.token,
        })
    }

    pub(in crate::process::windows::user32) fn retry_clipboard_completion(
        &self,
        child: u64,
    ) -> Option<User32Outcome32> {
        let pending = self.pending.last()?;
        let Continuation32::Clipboard(call) = pending.continuation else {
            return None;
        };
        (call.child.is_none() && call.completed == Some(child)).then_some(User32Outcome32::Wait {
            continuation: pending.token,
        })
    }

    pub(super) fn publish_clipboard_value(&mut self, token: u64, value: u32, error: Option<u32>) {
        let pending = self
            .pending
            .last_mut()
            .filter(|pending| pending.token == token)
            .expect("clipboard parent was preflighted");
        let Continuation32::Clipboard(call) = &mut pending.continuation else {
            unreachable!()
        };
        call.ready = Some((value, error));
    }
}

impl User32Context32 {
    pub(super) fn set_clipboard_bytes_wait(&mut self, token: u64, value: bool) {
        let pending = self
            .pending
            .last_mut()
            .filter(|pending| pending.token == token)
            .expect("checked clipboard parent");
        let Continuation32::Clipboard(call) = &mut pending.continuation else {
            unreachable!()
        };
        call.bytes_wait = value;
    }
}

impl User32Context32 {
    pub(super) fn current_clipboard_continuation(&self) -> Option<u64> {
        self.pending
            .last()
            .filter(|pending| matches!(pending.continuation, Continuation32::Clipboard(_)))
            .map(|pending| pending.token)
    }
}
