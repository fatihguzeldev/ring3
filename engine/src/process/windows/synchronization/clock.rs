use crate::{
    process::{
        EngineInstance, HostError,
        windows::{threads::PreparedThreadTimers32, user32::PreparedUser32TimerClock32},
    },
    windows::synchronization::PreparedSignals32,
};

pub(in crate::process) struct PreparedSynchronizationClock32 {
    synchronization: Option<(PreparedSignals32, PreparedThreadTimers32)>,
    user32: PreparedUser32TimerClock32,
}
impl EngineInstance {
    pub(in crate::process) fn prepare_synchronization_clock(
        &mut self,
        now: u64,
    ) -> Result<PreparedSynchronizationClock32, HostError> {
        let user32 = self.prepare_user32_timer_clock(now)?;
        let synchronization = if let Some(runtime) = self.synchronization.as_ref() {
            let signals = runtime
                .context
                .prepare_time_with_messages(now, user32.queue_rows())
                .map_err(HostError::Synchronization)?;
            let timers = self.prepare_thread_timer_apcs(signals.timer_apcs())?;
            Some((signals, timers))
        } else {
            None
        };
        Ok(PreparedSynchronizationClock32 {
            synchronization,
            user32,
        })
    }
    pub(in crate::process) fn commit_synchronization_clock(
        &mut self,
        prepared: PreparedSynchronizationClock32,
    ) {
        self.commit_user32_timer_clock(prepared.user32);
        if let Some((signals, timers)) = prepared.synchronization {
            self.synchronization
                .as_mut()
                .unwrap()
                .context
                .commit_signals(signals);
            self.commit_thread_timer_apcs(timers);
        }
    }
}
