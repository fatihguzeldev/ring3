use super::*;
impl User32Context32 {
    pub(in crate::process::windows::user32) fn animation_wait(
        &self,
        token: u64,
    ) -> Option<AnimationCall32> {
        let pending = self
            .pending
            .last()
            .filter(|pending| pending.token == token)?;
        let Continuation32::Animation(call) = pending.continuation else {
            return None;
        };
        Some(call)
    }
    pub(super) fn publish_animation_call(&mut self, token: u64, call: AnimationCall32) {
        self.pending
            .iter_mut()
            .find(|pending| pending.token == token)
            .expect("animation parent remains current")
            .continuation = Continuation32::Animation(call);
    }
    pub(in crate::process::windows::user32) fn pending_animation_message(
        &self,
        token: u64,
    ) -> Result<Option<[u32; 4]>, E> {
        let call = self.animation_wait(token).ok_or(E::InvalidToken)?;
        if !call.ready || call.child.is_some() || call.layout.is_some() || call.next == call.count {
            return Ok(None);
        }
        let record = self.state.record(call.window)?;
        let Some(parent) = record.parent else {
            return Ok(None);
        };
        if self.windows().get(parent).is_err() {
            return Ok(None);
        }
        Ok(Some([
            parent.get(),
            0x111,
            (record.menu & 0xffff) | (call.notifications[call.next] << 16),
            call.window.get(),
        ]))
    }
    pub(in crate::process::windows::user32) fn resume_animation(
        &mut self,
        token: u64,
        ram: &mut AddressSpace,
    ) -> Result<User32Outcome32, E> {
        let mut call = self.animation_wait(token).ok_or(E::InvalidToken)?;
        if !call.ready || call.child.is_some() {
            return Ok(User32Outcome32::Wait {
                continuation: token,
            });
        }
        if self.windows().get(call.window).is_err() {
            self.pending.pop();
            return Ok(User32Outcome32::Return(0));
        }
        if let Some((width, height)) = call.layout {
            let outcome = self.prepare_arguments(
                User32Api32::SetWindowPos,
                &[call.window.get(), 0, 0, 0, width, height, 0x16],
                ram,
                call.call,
            )?;
            call.layout = None;
            if let Some(child) = child_token(outcome) {
                call.child = Some((child, true));
                call.completed = None;
                self.publish_animation_call(token, call);
                return Ok(outcome);
            }
        }
        self.publish_animation_call(token, call);
        if let Some(args) = self.pending_animation_message(token)? {
            let outcome = self.send_message(&args, call.call)?;
            let child = child_token(outcome).ok_or(E::InvalidRequest)?;
            call.child = Some((child, false));
            call.completed = None;
            self.publish_animation_call(token, call);
            return Ok(outcome);
        }
        if call.next < call.count {
            call.next += 1;
            self.publish_animation_call(token, call);
            return Ok(User32Outcome32::Wait {
                continuation: token,
            });
        }
        self.pending.pop();
        Ok(User32Outcome32::Return(call.result))
    }
    pub(in crate::process::windows::user32) fn complete_animation_child(
        &mut self,
        child: u64,
        outcome: User32Outcome32,
    ) -> Result<User32Outcome32, E> {
        if !matches!(outcome, User32Outcome32::Return(_)) {
            return Ok(outcome);
        }
        let Some(pending) = self.pending.last() else {
            return Ok(outcome);
        };
        let token = pending.token;
        let Some(mut call) = self.animation_wait(token) else {
            return Ok(outcome);
        };
        let Some((expected, layout)) = call.child else {
            return Ok(outcome);
        };
        if expected != child {
            return Ok(outcome);
        }
        call.child = None;
        call.completed = Some(child);
        if !layout {
            call.next += 1;
        }
        self.publish_animation_call(token, call);
        Ok(User32Outcome32::Wait {
            continuation: token,
        })
    }
    pub(in crate::process::windows::user32) fn retry_animation_completion(
        &self,
        child: u64,
    ) -> Option<User32Outcome32> {
        let pending = self.pending.last()?;
        let call = self.animation_wait(pending.token)?;
        (call.child.is_none() && call.completed == Some(child)).then_some(User32Outcome32::Wait {
            continuation: pending.token,
        })
    }
}
fn child_token(outcome: User32Outcome32) -> Option<u64> {
    match outcome {
        User32Outcome32::Callback(callback) => Some(callback.continuation),
        User32Outcome32::Wait { continuation } => Some(continuation),
        User32Outcome32::Return(_) => None,
    }
}
