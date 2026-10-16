use super::decoding;
use crate::{
    memory::GuestAddress,
    process::{EngineInstance, HostError},
    windows::synchronization::{
        SynchronizationApi32 as Api, SynchronizationError32 as Error, WaitOutcome32, WaitResult32,
    },
};

pub(super) struct TimerCapture32 {
    object: u64,
    generation: u64,
    thread: u32,
    activation: crate::windows::activation::CapturedActivation32,
}
impl super::SynchronizationRuntime32 {
    pub(in crate::process) fn retire_stale_timer_captures(&mut self) {
        self.timer_captures.retain(|capture| {
            self.context
                .timer_callback_identity(capture.object)
                .is_some_and(|(generation, thread, procedure)| {
                    generation == capture.generation && thread == capture.thread && procedure != 0
                })
        });
    }
}

impl EngineInstance {
    pub(in crate::process) fn waitable_timer_activation(
        &self,
        object: u64,
        generation: u64,
    ) -> Option<crate::windows::activation::CapturedActivation32> {
        self.synchronization
            .as_ref()?
            .timer_captures
            .iter()
            .find(|capture| capture.object == object && capture.generation == generation)
            .map(|capture| capture.activation.clone())
    }
    pub(in crate::process::windows) fn execute_timer_call(
        &mut self,
        api: Api,
        args: &[u32],
    ) -> Result<WaitOutcome32, HostError> {
        let ready =
            |value, last_error| Ok(WaitOutcome32::Ready(WaitResult32 { value, last_error }));
        match api {
            Api::CreateWaitableTimerA | Api::CreateWaitableTimerW => {
                let inherit = decoding::inherit(self.memory()?, args[0])?;
                let name = if args[2] == 0 {
                    None
                } else {
                    Some(decoding::name(
                        self.memory()?,
                        args[2],
                        api == Api::CreateWaitableTimerW,
                    )?)
                };
                let (prepared, existed) = self
                    .synchronization()?
                    .prepare_timer(args[1] != 0, inherit, name.as_deref())
                    .map_err(HostError::Synchronization)?;
                let error = if existed { 183 } else { 0 };
                if self.module_teb_address().is_some()
                    && self.current_windows_thread()?.last_error() != error
                {
                    self.memory()?
                        .ensure_transaction_versions(1)
                        .map_err(HostError::Memory)?;
                }
                let handle = self.synchronization_mut()?.commit_object_handle(prepared);
                ready(handle, Some(error))
            }
            Api::OpenWaitableTimerA | Api::OpenWaitableTimerW => {
                let name = decoding::name(self.memory()?, args[2], api == Api::OpenWaitableTimerW)?;
                let handle = self
                    .synchronization_mut()?
                    .open_timer(args[0], args[1] != 0, &name)
                    .map_err(HostError::Synchronization)?;
                ready(handle, None)
            }
            Api::SetWaitableTimer | Api::CancelWaitableTimer => {
                let (object, _, owner) = self
                    .synchronization()?
                    .timer_identity(args[0])
                    .map_err(HostError::Synchronization)?;
                let mut prepared = if api == Api::SetWaitableTimer {
                    let mut bytes = [0; 8];
                    self.memory()?
                        .read(GuestAddress(args[1]), &mut bytes)
                        .map_err(HostError::Memory)?;
                    let due = i64::from_le_bytes(bytes);
                    if (args[2] as i32) < 0 {
                        return Err(HostError::Synchronization(Error::InvalidWait));
                    }
                    let now = self.cpu_clock.monotonic_bounds()?.1;
                    let delay = if due < 0 {
                        due.unsigned_abs().checked_mul(100)
                    } else {
                        (due as u64)
                            .saturating_sub(self.windows_filetime()?)
                            .checked_mul(100)
                    }
                    .ok_or(HostError::Synchronization(Error::Exhausted))?;
                    let due_ns = now
                        .checked_add(delay)
                        .ok_or(HostError::Synchronization(Error::Exhausted))?;
                    self.synchronization()?.prepare_set_timer(
                        args[0],
                        due_ns,
                        u64::from(args[2]) * 1_000_000,
                        args[3],
                        args[4],
                    )
                } else {
                    self.synchronization()?.prepare_cancel_timer(args[0])
                }
                .map_err(HostError::Synchronization)?;
                if owner != 0 {
                    let remaining = self.pending_apcs_without_timer(owner, object);
                    let context = self.synchronization()?;
                    if self.thread_apc_active(owner) {
                        context.project_apc_state(&mut prepared, owner, remaining)
                    } else {
                        context.project_cancel_apc_state(&mut prepared, owner, remaining)
                    }
                    .map_err(HostError::Synchronization)?;
                }
                if api == Api::SetWaitableTimer && args[5] != 0 {
                    self.check_windows_thread_write()?;
                    if self.module_teb_address().is_some()
                        && self.current_windows_thread()?.last_error() != 50
                    {
                        self.memory()?
                            .ensure_transaction_versions(1)
                            .map_err(HostError::Memory)?;
                    }
                }
                let capture = if api == Api::SetWaitableTimer && args[3] != 0 {
                    Some(self.prepare_managed_activation_capture()?)
                } else {
                    None
                };
                let token = capture.as_ref().map(|capture| capture.token());
                self.cancel_thread_timer_apcs(object);
                self.synchronization_mut()?.commit_signals(prepared);
                if let Some(capture) = capture {
                    self.commit_managed_activation_capture(capture);
                }
                let runtime = self.synchronization.as_mut().unwrap();
                runtime
                    .timer_captures
                    .retain(|capture| capture.object != object);
                if let Some(activation) = token {
                    let (_, generation, thread) = runtime
                        .context
                        .timer_identity(args[0])
                        .expect("timer identity was preflighted before publication");
                    assert!(runtime.timer_captures.len() < runtime.timer_captures.capacity());
                    runtime.timer_captures.push(TimerCapture32 {
                        object,
                        generation,
                        thread,
                        activation,
                    });
                }
                ready(
                    1,
                    if api == Api::SetWaitableTimer && args[5] != 0 {
                        Some(50)
                    } else {
                        None
                    },
                )
            }
            _ => Err(HostError::Synchronization(Error::Unsupported)),
        }
    }
}
