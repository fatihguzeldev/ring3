use super::*;
use crate::{
    process::windows::user32::{callback::Continuation32, timers::TimerSet32},
    windows::{comctl32::CommonControlKind32, user32::Message32},
};
const TIMER: u32 = 1;
#[derive(Clone, Copy)]
enum Stage32 {
    Relay,
    Original,
    Finish,
}
#[derive(Clone, Copy)]
pub(in crate::process::windows::user32) struct TooltipRelay32 {
    message: Message32,
    procedure: GuestAddress,
    call: User32CallContext32,
    after: u32,
    stage: Stage32,
    child: Option<u64>,
    completed: Option<u64>,
    result: u32,
}
fn mouse_message(message: u32) -> bool {
    matches!(message, 0x200..=0x20e | 0x2a3)
}
impl User32Context32 {
    fn tooltip_hit(&self, window: WindowHandle32, message: Message32) -> Result<Option<u64>, E> {
        let tip = self.tooltip(window)?;
        if !tip.enabled || message.message != 0x200 {
            return Ok(None);
        }
        let point = Point32 {
            x: i32::from(message.lparam as i16),
            y: i32::from((message.lparam >> 16) as i16),
        };
        Ok(tip
            .tools
            .iter()
            .find(|tool| {
                tool.flags & 0x20 == 0
                    && if tool.flags & 1 != 0 {
                        tool.id == message.window
                    } else {
                        tool.window == message.window
                            && point.x >= tool.rect.left
                            && point.x < tool.rect.right
                            && point.y >= tool.rect.top
                            && point.y < tool.rect.bottom
                    }
            })
            .map(|tool| tool.key))
    }
    fn tooltip_subclasses(&self, tooltip: WindowHandle32, source: u32) -> bool {
        self.tooltip(tooltip).is_ok_and(|tip| {
            tip.tools.iter().any(|tool| {
                tool.flags & 0x10 != 0
                    && if tool.flags & 1 != 0 {
                        tool.id == source
                    } else {
                        tool.window == source
                    }
            })
        })
    }
    pub(in crate::process::windows::user32) fn has_tooltip_subclass(
        &self,
        source: u32,
        message: u32,
    ) -> bool {
        mouse_message(message)
            && self
                .state
                .common_windows(CommonControlKind32::Tooltip)
                .any(|tip| self.tooltip_subclasses(tip, source))
    }
    fn tooltip_set_delay(&mut self, window: WindowHandle32, delay: u32, call: User32CallContext32) {
        self.timers
            .set(TimerSet32 {
                thread: call.thread_id,
                window: window.get(),
                id: TIMER,
                delay,
                now: call.now_ns,
                procedure: 0,
                activation: self.call_activation.clone(),
            })
            .expect("same window timer registration was preflighted");
    }
    pub(in crate::process::windows::user32::common::tooltip) fn relay_tooltip_event(
        &mut self,
        window: WindowHandle32,
        message: Message32,
        ram: &mut AddressSpace,
        gdi: &mut GdiRuntime32,
        call: User32CallContext32,
    ) -> Result<User32Outcome32, HostError> {
        if !mouse_message(message.message) {
            return Ok(User32Outcome32::Return(0));
        }
        self.owned(message.window, call.thread_id)
            .map_err(HostError::User32)?;
        let target = self
            .tooltip_hit(window, message)
            .map_err(HostError::User32)?;
        let tip = self.tooltip(window).map_err(HostError::User32)?;
        if tip.hover == target && message.message == 0x200 {
            return Ok(User32Outcome32::Return(0));
        }
        let hide = tip.automatic && tip.active.is_some();
        let delay = if hide { tip.delays[2] } else { tip.delays[0] };
        if target.is_some() {
            self.timers
                .check_window_set(call.thread_id, window.get(), TIMER, delay, call.now_ns)
                .map_err(HostError::User32)?;
        }
        let outcome = if hide {
            self.begin_tooltip_tracking(window, None, ram, gdi, call)?
        } else {
            User32Outcome32::Return(0)
        };
        if target.is_some() {
            self.tooltip_set_delay(window, delay, call);
        } else {
            self.timers.kill(call.thread_id, window.get(), TIMER);
        }
        let tip = self
            .tooltip_mut(window)
            .expect("same tooltip owns the prepared relay");
        tip.hover = target;
        tip.automatic = false;
        tip.suppressed = false;
        tip.point = Point32 {
            x: message.point.x.saturating_add(16),
            y: message.point.y.saturating_add(16),
        };
        Ok(outcome)
    }
    pub(in crate::process::windows::user32::common::tooltip) fn tooltip_timer(
        &mut self,
        window: WindowHandle32,
        id: u32,
        ram: &mut AddressSpace,
        gdi: &mut GdiRuntime32,
        call: User32CallContext32,
    ) -> Result<User32Outcome32, HostError> {
        if id != TIMER {
            return Ok(User32Outcome32::Return(0));
        }
        let tip = self.tooltip(window).map_err(HostError::User32)?;
        let target = tip.hover.filter(|&key| {
            tip.tools.iter().any(|tool| {
                tool.key == key
                    && WindowHandle32::try_from(if tool.flags & 1 != 0 {
                        tool.id
                    } else {
                        tool.window
                    })
                    .ok()
                    .is_some_and(|window| self.windows().get(window).is_ok())
            })
        });
        if target.is_none() || tip.suppressed || !tip.enabled {
            let outcome = if tip.automatic && tip.active.is_some() {
                self.begin_tooltip_tracking(window, None, ram, gdi, call)?
            } else {
                User32Outcome32::Return(0)
            };
            self.timers.kill(call.thread_id, window.get(), TIMER);
            let tip = self.tooltip_mut(window).expect("same tooltip");
            tip.hover = None;
            tip.automatic = false;
            return Ok(outcome);
        }
        if tip.automatic {
            let outcome = self.begin_tooltip_tracking(window, None, ram, gdi, call)?;
            self.timers.kill(call.thread_id, window.get(), TIMER);
            let tip = self.tooltip_mut(window).expect("same tooltip");
            tip.automatic = false;
            tip.suppressed = true;
            return Ok(outcome);
        }
        let delay = tip.delays[1];
        self.timers
            .check_window_set(call.thread_id, window.get(), TIMER, delay, call.now_ns)
            .map_err(HostError::User32)?;
        let outcome = self.begin_tooltip_tracking(window, target, ram, gdi, call)?;
        self.tooltip_set_delay(window, delay, call);
        self.tooltip_mut(window).expect("same tooltip").automatic = true;
        Ok(outcome)
    }
    pub(in crate::process::windows::user32) fn tooltip_subclass_call(
        &self,
        procedure: GuestAddress,
        arguments: [u32; 4],
    ) -> Result<Option<TooltipRelay32>, E> {
        if self.pending.last().is_some_and(|pending| matches!(pending.continuation,
            Continuation32::TooltipRelay(plan) if matches!(plan.stage,Stage32::Original) && plan.procedure==procedure &&
            [plan.message.window,plan.message.message,plan.message.wparam,plan.message.lparam]==arguments)) {
            return Ok(None);
        }
        if !self.has_tooltip_subclass(arguments[0], arguments[1]) {
            return Ok(None);
        }
        let window = WindowHandle32::try_from(arguments[0])?;
        let record = self.windows().get(window)?;
        if record.description.procedure != procedure {
            return Ok(None);
        }
        let message = Message32 {
            window: arguments[0],
            message: arguments[1],
            wparam: arguments[2],
            lparam: arguments[3],
            ..Default::default()
        };
        for tooltip in self
            .state
            .common_windows(CommonControlKind32::Tooltip)
            .filter(|&tooltip| self.tooltip_subclasses(tooltip, arguments[0]))
        {
            if self.tooltip_hit(tooltip, message)?.is_some() {
                let tip = self.tooltip(tooltip)?;
                self.timers.check_window_set(
                    record.description.thread_id,
                    tooltip.get(),
                    TIMER,
                    tip.delays[0],
                    self.timers.now,
                )?;
            }
        }
        if self.pending.len() + 3 > self.continuation_limit {
            return Err(E::Capacity);
        }
        self.next_token.checked_add(3).ok_or(E::Capacity)?;
        Ok(Some(self.tooltip_relay_plan(
            procedure,
            arguments,
            record.description.thread_id,
        )))
    }
    fn tooltip_relay_plan(
        &self,
        procedure: GuestAddress,
        args: [u32; 4],
        thread: u32,
    ) -> TooltipRelay32 {
        let call = User32CallContext32 {
            thread_id: thread,
            now_ns: self.timers.now,
            time_ms: self.timers.tick,
            ..self.runtime_input
        };
        TooltipRelay32 {
            message: Message32 {
                window: args[0],
                message: args[1],
                wparam: args[2],
                lparam: args[3],
                thread_id: thread,
                time_ms: call.time_ms,
                point: call.cursor,
            },
            procedure,
            call,
            after: 0,
            stage: Stage32::Relay,
            child: None,
            completed: None,
            result: 0,
        }
    }
    pub(in crate::process::windows::user32) fn begin_tooltip_foreign_relay(
        &mut self,
        args: &[u32],
        call: User32CallContext32,
    ) -> Result<User32Outcome32, E> {
        let window = self.owned(args[0], call.thread_id)?;
        let procedure = self.windows().get(window)?.description.procedure;
        if procedure.0 != args[4] {
            return Err(E::InvalidRequest);
        }
        let arguments = [args[0], args[1], args[2], args[3]];
        if let Some(plan) = self.tooltip_subclass_call(procedure, arguments)? {
            self.start(Continuation32::TooltipRelay(TooltipRelay32 {
                call,
                ..plan
            }))
        } else {
            self.start(Continuation32::Call {
                procedure,
                arguments,
            })
        }
    }
    fn tooltip_relay(&self, token: u64) -> Result<TooltipRelay32, E> {
        let pending = self
            .pending
            .last()
            .filter(|p| p.token == token)
            .ok_or(E::InvalidToken)?;
        let Continuation32::TooltipRelay(plan) = pending.continuation else {
            return Err(E::InvalidToken);
        };
        Ok(plan)
    }
    pub(in crate::process::windows::user32) fn tooltip_relay_wait(&self, token: u64) -> bool {
        self.pending.last().is_some_and(|p| {
            p.token == token && matches!(p.continuation, Continuation32::TooltipRelay(_))
        })
    }
    pub(in crate::process::windows::user32) fn resume_tooltip_relay(
        &mut self,
        token: u64,
        ram: &mut AddressSpace,
        gdi: &mut GdiRuntime32,
    ) -> Result<User32Outcome32, HostError> {
        let mut plan = self.tooltip_relay(token).map_err(HostError::User32)?;
        if plan.child.is_some() {
            return Err(HostError::User32(E::InvalidToken));
        }
        let outcome = match plan.stage {
            Stage32::Relay => {
                let next = self
                    .state
                    .common_windows(CommonControlKind32::Tooltip)
                    .filter(|tip| {
                        tip.get() > plan.after
                            && self.windows().get(*tip).is_ok_and(|record| {
                                record.description.thread_id == plan.call.thread_id
                            })
                            && self.tooltip_subclasses(*tip, plan.message.window)
                    })
                    .min_by_key(|tip| tip.get());
                if let Some(window) = next {
                    let outcome =
                        self.relay_tooltip_event(window, plan.message, ram, gdi, plan.call)?;
                    plan.after = window.get();
                    outcome
                } else {
                    plan.stage = Stage32::Original;
                    User32Outcome32::Return(0)
                }
            }
            Stage32::Original => {
                if self
                    .windows()
                    .get(
                        WindowHandle32::try_from(plan.message.window)
                            .map_err(E::from)
                            .map_err(HostError::User32)?,
                    )
                    .is_err()
                {
                    plan.result = 0;
                    plan.stage = Stage32::Finish;
                    User32Outcome32::Return(0)
                } else {
                    let activation = self
                        .pending
                        .last()
                        .expect("same relay owner")
                        .activation
                        .clone();
                    let outcome = self
                        .start(Continuation32::Call {
                            procedure: plan.procedure,
                            arguments: [
                                plan.message.window,
                                plan.message.message,
                                plan.message.wparam,
                                plan.message.lparam,
                            ],
                        })
                        .map_err(HostError::User32)?;
                    self.pending
                        .last_mut()
                        .expect("same original child")
                        .activation = activation;
                    outcome
                }
            }
            Stage32::Finish => {
                self.pending.pop();
                return Ok(User32Outcome32::Return(plan.result));
            }
        };
        let child = match outcome {
            User32Outcome32::Callback(callback) => Some(callback.continuation),
            User32Outcome32::Wait { continuation } => Some(continuation),
            _ => None,
        };
        if let Some(child) = child {
            plan.child = Some(child);
            plan.completed = None;
        }
        self.pending
            .iter_mut()
            .find(|p| p.token == token)
            .expect("same relay owner")
            .continuation = Continuation32::TooltipRelay(plan);
        if child.is_some() {
            Ok(outcome)
        } else {
            Ok(User32Outcome32::Wait {
                continuation: token,
            })
        }
    }
    pub(in crate::process::windows::user32) fn complete_tooltip_relay_child(
        &mut self,
        child: u64,
        outcome: User32Outcome32,
    ) -> Result<User32Outcome32, E> {
        let User32Outcome32::Return(result) = outcome else {
            return Ok(outcome);
        };
        let Some(pending) = self.pending.last_mut() else {
            return Ok(outcome);
        };
        let Continuation32::TooltipRelay(ref mut plan) = pending.continuation else {
            return Ok(outcome);
        };
        if plan.child != Some(child) {
            return Ok(outcome);
        }
        plan.child = None;
        plan.completed = Some(child);
        if matches!(plan.stage, Stage32::Original) {
            plan.result = result;
            plan.stage = Stage32::Finish;
        }
        Ok(User32Outcome32::Wait {
            continuation: pending.token,
        })
    }
    pub(in crate::process::windows::user32) fn retry_tooltip_relay_completion(
        &self,
        child: u64,
    ) -> Option<User32Outcome32> {
        let pending = self.pending.last()?;
        let Continuation32::TooltipRelay(plan) = pending.continuation else {
            return None;
        };
        (plan.child.is_none() && plan.completed == Some(child)).then_some(User32Outcome32::Wait {
            continuation: pending.token,
        })
    }
}

impl crate::process::EngineInstance {
    pub(in crate::process::windows) fn execute_tooltip_foreign_relay(
        &mut self,
        frame: &crate::windows::CallFrame32,
    ) -> Result<User32Outcome32, HostError> {
        self.check_tooltip_dispatch_invocation(frame)?;
        let call = self.user32_call_context()?;
        let queue = self.prepare_user32_queue_change_for(call.thread_id)?;
        let outcome = self
            .user32
            .as_mut()
            .ok_or(HostError::InvalidRequest)?
            .begin_tooltip_foreign_relay(frame.arguments(), call)
            .map_err(HostError::User32)?;
        self.commit_user32_queue_change(queue);
        Ok(outcome)
    }
    pub(in crate::process::windows::user32) fn prepare_user32_message_invocation(
        &self,
        target: u32,
        procedure: GuestAddress,
        args: &[u32],
        completion: crate::process::windows::threads::ThreadInvocationCompletion32,
    ) -> Result<crate::process::windows::threads::PreparedThreadInvocation32, HostError> {
        if self
            .user32
            .as_ref()
            .is_some_and(|context| context.has_tooltip_subclass(args[0], args[1]))
        {
            let gate = self
                .loaded_windows_modules()
                .flat_map(|module| module.gates())
                .find(|gate| gate.id == crate::windows::comctl32::TOOLTIP_RELAY_PROC)
                .ok_or(HostError::InvalidRequest)?;
            self.prepare_thread_invocation(
                target,
                gate.entry,
                &[args[0], args[1], args[2], args[3], procedure.0],
                completion,
            )
        } else {
            self.prepare_thread_invocation(target, procedure, args, completion)
        }
    }
}
