use super::{User32CallContext32, User32Context32, User32Outcome32, callback::Continuation32};
use crate::windows::{user32::User32Error32 as Error, window::WindowHandle32};

#[derive(Clone, Copy)]
enum Targets32 {
    One(Option<WindowHandle32>, u32),
    Children { after: u32, through: u32 },
}

#[derive(Clone, Copy)]
pub(super) struct UiChange32 {
    window: WindowHandle32,
    thread: u32,
    wparam: u32,
    targets: Targets32,
    child: Option<u64>,
    completed: Option<u64>,
}

impl User32Context32 {
    pub(super) fn ui_message(
        &mut self,
        args: &[u32],
        context: User32CallContext32,
    ) -> Result<Option<User32Outcome32>, Error> {
        if !matches!(args[1], 0x127..=0x129) {
            return Ok(None);
        }
        let window = self.owned(args[0], context.thread_id)?;
        let old = self.state.ui_state(window)?;
        if args[1] == 0x129 {
            return Ok(Some(User32Outcome32::Return(old)));
        }
        let (action, flags) = match args[2] & 0xffff {
            action @ (1 | 2) => (action, (args[2] >> 16) & 7),
            3 => (if self.input.last_ui_keyboard { 2 } else { 1 }, 3),
            _ => return Ok(Some(User32Outcome32::Return(0))),
        };
        let data = self.state.record(window)?;
        let targets = if args[1] == 0x127 && data.style & 0x4000_0000 != 0 {
            Targets32::One(data.parent, 0x127)
        } else {
            let new = if action == 1 {
                old | flags
            } else {
                old & !flags
            };
            if new == old {
                return Ok(Some(User32Outcome32::Return(0)));
            }
            if args[1] == 0x127 {
                Targets32::One(Some(window), 0x128)
            } else {
                Targets32::Children {
                    after: 0,
                    through: self.state.ui_child_boundary(window),
                }
            }
        };
        self.next_token.checked_add(2).ok_or(Error::Capacity)?;
        self.prepare_thread_slots()?;
        if self.pending.len() + 2 > self.continuation_limit {
            return Err(Error::Capacity);
        }
        if args[1] == 0x128 {
            self.state.set_ui_state(
                window,
                if action == 1 {
                    old | flags
                } else {
                    old & !flags
                },
            )?;
        }
        Ok(Some(self.start(Continuation32::UiState(UiChange32 {
            window,
            thread: context.thread_id,
            wparam: action | (flags << 16),
            targets,
            child: None,
            completed: None,
        }))?))
    }

    pub(super) fn ui_wait(&self, token: u64) -> Option<UiChange32> {
        let pending = self
            .pending
            .last()
            .filter(|pending| pending.token == token)?;
        let Continuation32::UiState(call) = pending.continuation else {
            return None;
        };
        Some(call)
    }

    pub(super) fn pending_ui_message(&self, token: u64) -> Option<[u32; 4]> {
        let call = self.ui_wait(token)?;
        if call.child.is_some() || self.windows().get(call.window).is_err() {
            return None;
        }
        let (target, message) = match call.targets {
            Targets32::One(target, message) => (target?, message),
            Targets32::Children { after, through } => (
                self.state.next_ui_child(call.window, after, through)?,
                0x128,
            ),
        };
        self.windows().get(target).ok()?;
        Some([target.get(), message, call.wparam, 0])
    }

    pub(super) fn resume_ui_change(&mut self, token: u64) -> Result<User32Outcome32, Error> {
        let mut call = self.ui_wait(token).ok_or(Error::InvalidToken)?;
        if call.child.is_some() {
            return Err(Error::InvalidToken);
        }
        let Some(message) = self.pending_ui_message(token) else {
            self.pending.pop();
            return Ok(User32Outcome32::Return(0));
        };
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
        call.targets = match call.targets {
            Targets32::One(_, message) => Targets32::One(None, message),
            Targets32::Children { through, .. } => Targets32::Children {
                after: message[0],
                through,
            },
        };
        call.completed = None;
        self.pending
            .iter_mut()
            .find(|pending| pending.token == token)
            .ok_or(Error::InvalidToken)?
            .continuation = Continuation32::UiState(call);
        Ok(outcome)
    }

    pub(super) fn complete_ui_child(
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
        let Continuation32::UiState(mut call) = pending.continuation else {
            return Ok(outcome);
        };
        if call.child != Some(child) {
            return Ok(outcome);
        }
        call.child = None;
        call.completed = Some(child);
        pending.continuation = Continuation32::UiState(call);
        Ok(User32Outcome32::Wait {
            continuation: pending.token,
        })
    }

    pub(super) fn retry_ui_completion(&self, child: u64) -> Option<User32Outcome32> {
        let pending = self.pending.last()?;
        let Continuation32::UiState(call) = pending.continuation else {
            return None;
        };
        (call.child.is_none() && call.completed == Some(child)).then_some(User32Outcome32::Wait {
            continuation: pending.token,
        })
    }
}
