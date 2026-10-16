use crate::{
    memory::GuestAddress,
    process::{EngineInstance, HostError},
    windows::winmm::timers::{MultimediaTimerCallback32, MultimediaTimerTarget32},
};
impl EngineInstance {
    pub(in crate::process) fn multimedia_timer_deadline(&self) -> Option<u64> {
        self.winmm.timers.deadline()
    }
    pub(in crate::process) fn service_multimedia_timers(
        &mut self,
        now: u64,
    ) -> Result<(), HostError> {
        loop {
            let Some(due) = self
                .winmm
                .timers
                .prepare_due(now)
                .map_err(|_| HostError::Infrastructure)?
            else {
                return Ok(());
            };
            match due.target {
                MultimediaTimerTarget32::Function { procedure, user } => {
                    let worker = self.ensure_multimedia_timer_worker()?;
                    let capture = self
                        .winmm
                        .captures
                        .iter()
                        .find(|(timer, _)| *timer == due.identity.timer)
                        .map_or(0, |(_, handle)| *handle);
                    let invocation = match self.prepare_multimedia_timer_callback(
                        worker,
                        GuestAddress(procedure),
                        [due.identity.timer, 0, user, 0, 0],
                        due.identity,
                        capture,
                        !due.periodic,
                    ) {
                        Ok(invocation) => invocation,
                        Err(HostError::Thread(crate::windows::thread::ThreadError32::Capacity)) => {
                            return Ok(());
                        }
                        Err(error) => return Err(error),
                    };
                    self.winmm.timers.commit_due(due);
                    self.commit_thread_invocation(invocation);
                    if !due.periodic {
                        self.winmm
                            .captures
                            .retain(|(timer, _)| *timer != due.identity.timer);
                    }
                }
                MultimediaTimerTarget32::SetEvent(binding)
                | MultimediaTimerTarget32::PulseEvent(binding) => {
                    let release = !due.periodic;
                    let signals = self
                        .synchronization
                        .as_ref()
                        .ok_or(HostError::Infrastructure)?
                        .context()
                        .prepare_timer_event(
                            binding,
                            due.occurrences,
                            matches!(due.target, MultimediaTimerTarget32::PulseEvent(_)),
                            release,
                        )
                        .map_err(HostError::Synchronization)?;
                    self.winmm.timers.commit_due(due);
                    self.synchronization
                        .as_mut()
                        .unwrap()
                        .context_mut()
                        .commit_timer_event(signals);
                }
            }
        }
    }
    pub(in crate::process) fn finish_multimedia_timer_callback(
        &mut self,
        identity: MultimediaTimerCallback32,
    ) -> Result<(), HostError> {
        self.winmm
            .timers
            .finish_callback(identity)
            .map_err(|_| HostError::Infrastructure)
    }
    pub(in crate::process) fn append_multimedia_timer_exit_bindings(
        &self,
        output: &mut Vec<crate::windows::synchronization::EventBinding32>,
    ) -> Result<(), HostError> {
        self.winmm
            .timers
            .append_bindings(output)
            .map_err(|_| HostError::Infrastructure)
    }
}
