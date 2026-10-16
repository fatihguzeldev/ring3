use super::replies::TimerReply32;
use crate::{
    process::{EngineInstance, HostError, calls::call::PendingOwner},
    windows::{CallFrame32, winmm::timers::MultimediaTimerTarget32},
};
impl EngineInstance {
    pub(in crate::process::windows::winmm) fn set_multimedia_timer(
        &mut self,
        args: &[u32],
    ) -> Result<u32, HostError> {
        let (_, now) = self.cpu_clock.monotonic_bounds()?;
        let kind = args[4] & 0x30;
        let bindings = if matches!(kind, 0x10 | 0x20) {
            let Some(sync) = self.synchronization.as_ref() else {
                return Ok(0);
            };
            match sync.context().prepare_event_bindings(&[args[2]], &[]) {
                Ok(prepared) => Some(prepared),
                Err(_) => return Ok(0),
            }
        } else {
            None
        };
        let target = match kind {
            0 => MultimediaTimerTarget32::Function {
                procedure: args[2],
                user: args[3],
            },
            0x10 => MultimediaTimerTarget32::SetEvent(bindings.as_ref().unwrap().bindings()[0]),
            0x20 => MultimediaTimerTarget32::PulseEvent(bindings.as_ref().unwrap().bindings()[0]),
            _ => return Ok(0),
        };
        let prepared = match self
            .winmm
            .timers
            .prepare_create(now, args[0], args[1], args[4], target)
        {
            Ok(prepared) => prepared,
            Err(_) => return Ok(0),
        };
        if kind == 0 {
            self.winmm
                .captures
                .try_reserve(1)
                .map_err(|_| HostError::Infrastructure)?;
            match self.ensure_multimedia_timer_worker() {
                Ok(_) => {}
                Err(HostError::Thread(
                    crate::windows::thread::ThreadError32::Capacity
                    | crate::windows::thread::ThreadError32::Allocation
                    | crate::windows::thread::ThreadError32::Exhausted,
                )) => return Ok(0),
                Err(error) => return Err(error),
            }
        }
        let activation = (kind == 0)
            .then(|| self.prepare_activation_capture())
            .transpose()?;
        if let Some(bindings) = bindings {
            self.synchronization
                .as_mut()
                .unwrap()
                .context_mut()
                .commit_event_bindings(bindings);
        }
        let activation = activation.map(|prepared| self.commit_activation_capture(prepared));
        let id = self.winmm.timers.commit_create(prepared);
        if let Some(activation) = activation {
            self.winmm.captures.push((id, activation));
        }
        Ok(id)
    }
    pub(in crate::process::windows::winmm) fn kill_multimedia_timer(
        &mut self,
        owner: PendingOwner,
        token: u32,
        frame: CallFrame32,
    ) -> Result<Option<u32>, HostError> {
        let id = frame.arguments()[0];
        let (callback, binding) = match self.winmm.timers.can_kill(id) {
            Ok(plan) => plan,
            Err(_) => return Ok(Some(11)),
        };
        let release = binding
            .map(|binding| {
                self.synchronization
                    .as_ref()
                    .ok_or(HostError::Infrastructure)?
                    .context()
                    .prepare_event_bindings(&[], &[binding])
                    .map_err(HostError::Synchronization)
            })
            .transpose()?;
        let capture = self
            .winmm
            .captures
            .iter()
            .find(|(timer, _)| *timer == id)
            .map(|(_, handle)| *handle);
        let activation = capture
            .map(|handle| self.prepare_activation_hold_release(&[handle]))
            .transpose()?;
        if let Some(activation) = activation {
            self.commit_activation_hold_release(activation);
        }
        self.winmm.captures.retain(|(timer, _)| *timer != id);
        self.winmm.timers.commit_kill(id);
        if let Some(release) = release {
            self.synchronization
                .as_mut()
                .unwrap()
                .context_mut()
                .commit_event_bindings(release);
        }
        if let Some(callback) = callback {
            self.winmm.reply = Some(TimerReply32 {
                owner,
                token,
                frame,
                callback,
            });
            Ok(None)
        } else {
            Ok(Some(0))
        }
    }
}
