use super::*;
use crate::{
    process::windows::user32::{callback::Continuation32, memory::Scratch32},
    windows::user32::User32Api32,
};
#[derive(Clone, Copy)]
enum Stage32 {
    Position,
    PrepareNotify,
    Notify,
    Reposition,
    Show,
    Hide,
    Paint,
    Finish,
}
#[derive(Clone, Copy)]
enum Child32 {
    Position,
    Notify,
    Show,
    Hide,
    Reposition,
}
#[derive(Clone, Copy)]
pub(in crate::process::windows::user32) struct TooltipTrack32 {
    window: WindowHandle32,
    target: Option<u64>,
    owner: Option<WindowHandle32>,
    id: u32,
    call: User32CallContext32,
    stage: Stage32,
    notify: bool,
    rect: Rect32,
    scratch: Option<Scratch32>,
    child: Option<(u64, Child32)>,
    completed: Option<u64>,
}
impl TooltipTrack32 {
    pub(in crate::process::windows::user32) fn scratch(self) -> Option<Scratch32> {
        self.scratch
    }
}
impl User32Context32 {
    pub(in crate::process::windows::user32::common::tooltip) fn begin_tooltip_tracking(
        &mut self,
        window: WindowHandle32,
        target: Option<u64>,
        ram: &mut AddressSpace,
        gdi: &mut GdiRuntime32,
        call: User32CallContext32,
    ) -> Result<User32Outcome32, HostError> {
        self.begin_tooltip_tracking_change(window, target, ram, gdi, call, None)
    }
    pub(in crate::process::windows::user32::common::tooltip) fn begin_tooltip_tracking_change(
        &mut self,
        window: WindowHandle32,
        target: Option<u64>,
        ram: &mut AddressSpace,
        gdi: &mut GdiRuntime32,
        call: User32CallContext32,
        change: Option<TrackingChange32<'_>>,
    ) -> Result<User32Outcome32, HostError> {
        self.prepare_thread_slots().map_err(HostError::User32)?;
        if self.pending.len() + 2 > self.continuation_limit {
            return Err(HostError::User32(E::Capacity));
        }
        self.next_token
            .checked_add(2)
            .ok_or(HostError::User32(E::Capacity))?;
        let tip = self.tooltip(window).map_err(HostError::User32)?;
        let target = target.filter(|_| tip.enabled);
        let notify = if target.is_some() {
            tip.active != target
                || !self
                    .windows()
                    .get(window)
                    .map_err(E::from)
                    .map_err(HostError::User32)?
                    .description
                    .visible
        } else {
            self.windows()
                .get(window)
                .map_err(E::from)
                .map_err(HostError::User32)?
                .description
                .visible
        };
        let override_tool = match change {
            Some(TrackingChange32::Text(tool)) => Some(tool),
            _ => None,
        };
        let point = match change {
            Some(TrackingChange32::Position(point) | TrackingChange32::Resolved(point)) => point,
            _ => tip.point,
        };
        let tool = override_tool.or_else(|| {
            if let Some(key) = target {
                tip.tools.iter().find(|tool| tool.key == key)
            } else {
                tip.current()
            }
        });
        if let Some(tool) = tool
            && target.is_some()
            && tool.callback
            && !matches!(change, Some(TrackingChange32::Resolved(_)))
            && (notify || matches!(change, Some(TrackingChange32::Text(_))))
        {
            let source = super::text::TextTool32 {
                key: tool.key,
                revision: tool.revision,
                owner: if tool.callback_owner == 0 {
                    tool.window
                } else {
                    tool.callback_owner
                },
                id: tool.id,
                flags: tool.flags,
                data: tool.data,
            };
            return self.begin_tooltip_text(window, source, ram, call, point);
        }
        let id = tool.map_or(0, |tool| tool.id);
        let owner = tool
            .and_then(|tool| WindowHandle32::try_from(tool.window).ok())
            .filter(|&owner| self.windows().get(owner).is_ok());
        let mut rect = self.state.record(window).map_err(HostError::User32)?.outer;
        let position = if target.is_some() {
            let tool = tool.ok_or(HostError::User32(E::InvalidRequest))?;
            let (width, height) = self.tooltip_extent(window, tool, ram, gdi, call.thread_id)?;
            let x = point.x
                - if tool.flags & 2 != 0 {
                    width as i32 / 2
                } else {
                    0
                };
            let y = point.y;
            rect = Rect32 {
                left: x,
                top: y,
                right: x
                    .checked_add(width as i32)
                    .ok_or(HostError::User32(E::Capacity))?,
                bottom: y
                    .checked_add(height as i32)
                    .ok_or(HostError::User32(E::Capacity))?,
            };
            Some(
                self.prepare_window_position(
                    User32Api32::SetWindowPos,
                    &[window.get(), 0, x as u32, y as u32, width, height, 0x14],
                    ram,
                    call,
                )
                .map_err(HostError::User32)?,
            )
        } else {
            None
        };
        let outcome = self
            .start(Continuation32::TooltipTrack(TooltipTrack32 {
                window,
                target,
                owner,
                id,
                call,
                stage: if target.is_some() {
                    Stage32::Position
                } else {
                    Stage32::Hide
                },
                notify,
                rect,
                scratch: None,
                child: None,
                completed: None,
            }))
            .expect("tooltip pair was preflighted");
        let tip = self
            .tooltip_mut(window)
            .expect("same tooltip owns prepared tracking");
        tip.active = target;
        tip.point = point;
        if let Some(position) = position {
            let token = match outcome {
                User32Outcome32::Wait { continuation } => continuation,
                _ => unreachable!(),
            };
            let outcome = self
                .start(position)
                .expect("tooltip position pair was preflighted");
            let mut plan = self
                .tooltip_tracking(token)
                .expect("same prepared tracking");
            plan.child = Some((
                child_token(outcome).expect("prepared position has a child"),
                Child32::Position,
            ));
            self.publish_tooltip_tracking(token, plan);
            return Ok(outcome);
        }
        Ok(outcome)
    }
    fn tooltip_tracking(&self, token: u64) -> Result<TooltipTrack32, E> {
        let pending = self
            .pending
            .iter()
            .find(|pending| pending.token == token)
            .ok_or(E::InvalidToken)?;
        let Continuation32::TooltipTrack(call) = pending.continuation else {
            return Err(E::InvalidToken);
        };
        Ok(call)
    }
    fn publish_tooltip_tracking(&mut self, token: u64, call: TooltipTrack32) {
        self.pending
            .iter_mut()
            .find(|pending| pending.token == token)
            .expect("same tooltip tracking owner")
            .continuation = Continuation32::TooltipTrack(call);
    }
    pub(in crate::process::windows::user32) fn tooltip_tracking_wait(&self, token: u64) -> bool {
        self.pending.last().is_some_and(|pending| {
            pending.token == token
                && matches!(pending.continuation, Continuation32::TooltipTrack(_))
        })
    }
    pub(in crate::process::windows::user32) fn pending_tooltip_message(
        &self,
        token: u64,
    ) -> Result<Option<[u32; 4]>, E> {
        let call = self.tooltip_tracking(token)?;
        if !matches!(call.stage, Stage32::Notify) || call.child.is_some() {
            return Ok(None);
        }
        if self.windows().get(call.window).is_err()
            || self.tooltip(call.window)?.active != call.target
        {
            return Ok(None);
        }
        Ok(call
            .owner
            .filter(|&owner| self.windows().get(owner).is_ok())
            .map(|owner| {
                [
                    owner.get(),
                    0x4e,
                    call.id,
                    call.scratch.expect("notify scratch was prepared").address.0,
                ]
            }))
    }
    pub(in crate::process::windows::user32) fn resume_tooltip_tracking(
        &mut self,
        token: u64,
        ram: &mut AddressSpace,
        gdi: &mut GdiRuntime32,
    ) -> Result<User32Outcome32, HostError> {
        let mut call = self.tooltip_tracking(token).map_err(HostError::User32)?;
        if let Some(scratch) = call.scratch {
            scratch.check(ram).map_err(HostError::User32)?;
        }
        if call.child.is_some() {
            return Err(HostError::User32(E::InvalidToken));
        }
        if self.windows().get(call.window).is_err()
            || self.tooltip(call.window).map_err(HostError::User32)?.active != call.target
        {
            call.stage = Stage32::Finish;
        }
        let mut outgoing = None;
        match call.stage {
            Stage32::PrepareNotify => {
                if call.notify && call.owner.is_some() {
                    let mut bytes = [0; 12];
                    for (out, value) in bytes.chunks_exact_mut(4).zip([
                        call.window.get(),
                        call.id,
                        if call.target.is_some() {
                            (-521i32) as u32
                        } else {
                            (-522i32) as u32
                        },
                    ]) {
                        out.copy_from_slice(&value.to_le_bytes());
                    }
                    call.scratch = Some(
                        Scratch32::allocate(ram, self.scratch_bounds, &bytes)
                            .map_err(HostError::User32)?,
                    );
                    call.stage = Stage32::Notify;
                } else {
                    call.stage = if call.target.is_some() {
                        Stage32::Show
                    } else {
                        Stage32::Finish
                    };
                }
            }
            Stage32::Notify => {
                if let Some(message) = self
                    .pending_tooltip_message(token)
                    .map_err(HostError::User32)?
                {
                    outgoing = Some((
                        self.send_message(&message, call.call)
                            .map_err(HostError::User32)?,
                        Child32::Notify,
                    ));
                } else {
                    call.stage = if call.target.is_some() {
                        Stage32::Show
                    } else {
                        Stage32::Finish
                    };
                }
            }
            Stage32::Reposition => {
                if self
                    .state
                    .record(call.window)
                    .map_err(HostError::User32)?
                    .outer
                    != call.rect
                {
                    outgoing = Some((
                        self.move_window(
                            User32Api32::SetWindowPos,
                            &[
                                call.window.get(),
                                0,
                                call.rect.left as u32,
                                call.rect.top as u32,
                                (call.rect.right - call.rect.left) as u32,
                                (call.rect.bottom - call.rect.top) as u32,
                                0x14,
                            ],
                            ram,
                            call.call,
                        )
                        .map_err(HostError::User32)?,
                        Child32::Reposition,
                    ));
                } else {
                    call.stage = Stage32::Show;
                }
            }
            Stage32::Show => {
                outgoing = Some((
                    self.show_window(&[call.window.get(), 4], call.call)
                        .map_err(HostError::User32)?,
                    Child32::Show,
                ))
            }
            Stage32::Hide => {
                outgoing = Some((
                    self.show_window(&[call.window.get(), 0], call.call)
                        .map_err(HostError::User32)?,
                    Child32::Hide,
                ))
            }
            Stage32::Paint => {
                self.state.check_paint_change().map_err(HostError::User32)?;
                self.tooltip_changed(call.window);
                self.paint_tooltip(call.window, ram, gdi, call.call.thread_id)?;
                call.stage = Stage32::Finish;
            }
            Stage32::Finish => {
                if let Some(scratch) = call.scratch {
                    scratch.release(ram).map_err(HostError::User32)?;
                }
                self.pending.pop();
                return Ok(User32Outcome32::Return(0));
            }
            Stage32::Position => return Err(HostError::InvalidRequest),
        }
        if let Some((outcome, kind)) = outgoing {
            if let Some(child) = child_token(outcome) {
                call.child = Some((child, kind));
                call.completed = None;
                self.publish_tooltip_tracking(token, call);
                return Ok(outcome);
            }
            call.stage = after_child(kind, 0, call.target.is_some(), call.notify);
        }
        self.publish_tooltip_tracking(token, call);
        Ok(User32Outcome32::Wait {
            continuation: token,
        })
    }
    pub(in crate::process::windows::user32) fn complete_tooltip_child(
        &mut self,
        child: u64,
        outcome: User32Outcome32,
    ) -> Result<User32Outcome32, E> {
        let User32Outcome32::Return(result) = outcome else {
            return Ok(outcome);
        };
        let Some(pending) = self.pending.last() else {
            return Ok(outcome);
        };
        let Continuation32::TooltipTrack(mut call) = pending.continuation else {
            return Ok(outcome);
        };
        let Some((expected, kind)) = call.child else {
            return Ok(outcome);
        };
        if expected != child {
            return Ok(outcome);
        }
        let token = pending.token;
        call.child = None;
        call.completed = Some(child);
        call.stage = after_child(kind, result, call.target.is_some(), call.notify);
        self.publish_tooltip_tracking(token, call);
        Ok(User32Outcome32::Wait {
            continuation: token,
        })
    }
    pub(in crate::process::windows::user32) fn retry_tooltip_completion(
        &self,
        child: u64,
    ) -> Option<User32Outcome32> {
        let pending = self.pending.last()?;
        let Continuation32::TooltipTrack(call) = pending.continuation else {
            return None;
        };
        (call.child.is_none() && call.completed == Some(child)).then_some(User32Outcome32::Wait {
            continuation: pending.token,
        })
    }
}
fn child_token(outcome: User32Outcome32) -> Option<u64> {
    match outcome {
        User32Outcome32::Callback(callback) => Some(callback.continuation),
        User32Outcome32::Wait { continuation } => Some(continuation),
        _ => None,
    }
}
fn after_child(kind: Child32, result: u32, show: bool, notify: bool) -> Stage32 {
    match kind {
        Child32::Position => {
            if notify {
                Stage32::PrepareNotify
            } else {
                Stage32::Paint
            }
        }
        Child32::Notify if show => {
            if result == 0 {
                Stage32::Reposition
            } else {
                Stage32::Show
            }
        }
        Child32::Notify => Stage32::Finish,
        Child32::Show => Stage32::Paint,
        Child32::Hide => Stage32::PrepareNotify,
        Child32::Reposition => Stage32::Show,
    }
}
