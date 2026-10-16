use crate::{
    process::{EngineInstance, HostError, windows::threads::PreparedThreadTimers32},
    windows::synchronization::PreparedSignals32,
};

pub(in crate::process) struct PreparedSynchronizationClock32 {
    signals: PreparedSignals32,
    timers: PreparedThreadTimers32,
}
impl EngineInstance {
    pub(in crate::process) fn prepare_synchronization_clock(
        &self,
        now: u64,
    ) -> Result<Option<PreparedSynchronizationClock32>, HostError> {
        let Some(runtime) = self.synchronization.as_ref() else {
            return Ok(None);
        };
        let signals = runtime
            .context
            .prepare_time(now)
            .map_err(HostError::Synchronization)?;
        let timers = self.prepare_thread_timer_apcs(signals.timer_apcs())?;
        Ok(Some(PreparedSynchronizationClock32 { signals, timers }))
    }
    pub(in crate::process) fn commit_synchronization_clock(
        &mut self,
        prepared: Option<PreparedSynchronizationClock32>,
    ) {
        if let Some(prepared) = prepared {
            self.synchronization
                .as_mut()
                .unwrap()
                .context
                .commit_signals(prepared.signals);
            self.commit_thread_timer_apcs(prepared.timers);
        }
    }
}
