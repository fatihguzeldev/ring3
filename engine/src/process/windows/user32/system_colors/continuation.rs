use super::{ColorChange32, Stage32};
use crate::{
    process::windows::user32::{
        User32CallContext32, User32Context32, User32Outcome32, callback::Continuation32,
    },
    windows::user32::User32Error32 as Error,
};

impl User32Context32 {
    pub(super) fn begin_system_colors(&mut self, thread: u32) -> Result<User32Outcome32, Error> {
        self.next_token.checked_add(2).ok_or(Error::Capacity)?;
        self.prepare_thread_slots()?;
        if self.pending.len() + 2 > self.continuation_limit {
            return Err(Error::Capacity);
        }
        self.start(Continuation32::SystemColors(ColorChange32 {
            thread,
            through: self.state.color_window_boundary(),
            after: 0,
            stage: Stage32::Notify,
            child: None,
            completed: None,
        }))
    }

    pub(in crate::process::windows::user32) fn system_colors_wait(
        &self,
        token: u64,
    ) -> Option<ColorChange32> {
        let pending = self
            .pending
            .last()
            .filter(|pending| pending.token == token)?;
        let Continuation32::SystemColors(call) = pending.continuation else {
            return None;
        };
        Some(call)
    }

    pub(in crate::process) fn pending_system_colors_message(&self, token: u64) -> Option<[u32; 4]> {
        let call = self.system_colors_wait(token)?;
        if call.stage != Stage32::Notify || call.child.is_some() {
            return None;
        }
        self.state
            .next_color_window(call.after, call.through, false)
            .map(|window| [window.get(), 0x15, 0, 0])
    }

    pub(in crate::process::windows::user32) fn resume_system_colors(
        &mut self,
        token: u64,
    ) -> Result<User32Outcome32, Error> {
        let mut call = self.system_colors_wait(token).ok_or(Error::InvalidToken)?;
        if call.stage == Stage32::Finish {
            self.pending.pop();
            self.last_error = Some(0);
            return Ok(User32Outcome32::Return(1));
        }
        if call.stage == Stage32::Notify && call.child.is_none() {
            if let Some(message) = self.pending_system_colors_message(token) {
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
                call.after = message[0];
                call.completed = None;
                self.publish_system_colors(token, call);
                return Ok(outcome);
            }
            call.stage = Stage32::Repaint;
            call.after = 0;
            self.publish_system_colors(token, call);
        }
        Ok(User32Outcome32::Wait {
            continuation: token,
        })
    }

    pub(in crate::process::windows::user32) fn publish_system_colors(
        &mut self,
        token: u64,
        call: ColorChange32,
    ) {
        self.pending
            .iter_mut()
            .find(|pending| pending.token == token)
            .expect("preflighted color notification parent")
            .continuation = Continuation32::SystemColors(call);
    }

    pub(in crate::process::windows::user32) fn complete_system_colors_child(
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
        let Continuation32::SystemColors(mut call) = pending.continuation else {
            return Ok(outcome);
        };
        if call.child != Some(child) {
            return Ok(outcome);
        }
        call.child = None;
        call.completed = Some(child);
        pending.continuation = Continuation32::SystemColors(call);
        Ok(User32Outcome32::Wait {
            continuation: pending.token,
        })
    }

    pub(in crate::process::windows::user32) fn retry_system_colors_completion(
        &self,
        child: u64,
    ) -> Option<User32Outcome32> {
        let pending = self.pending.last()?;
        let Continuation32::SystemColors(call) = pending.continuation else {
            return None;
        };
        (call.child.is_none() && call.completed == Some(child)).then_some(User32Outcome32::Wait {
            continuation: pending.token,
        })
    }
}
