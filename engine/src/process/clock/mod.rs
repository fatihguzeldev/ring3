use super::{EngineInstance, HostError};

const NANOSECONDS_PER_SECOND: u128 = 1_000_000_000;

#[derive(Default)]
pub(in crate::process) struct CpuClock {
    state: Option<ClockState>,
}

struct ClockState {
    frequency_hz: u64,
    epoch_ns: u64,
    observed_ns: u64,
    timestamp: u64,
}

impl CpuClock {
    fn configure(&mut self, frequency_hz: u64, epoch_ns: u64) -> Result<(), HostError> {
        if frequency_hz == 0 || self.state.is_some() {
            return Err(HostError::InvalidRequest);
        }
        self.state = Some(ClockState {
            frequency_hz,
            epoch_ns,
            observed_ns: epoch_ns,
            timestamp: 0,
        });
        Ok(())
    }

    fn observe(&mut self, now_ns: u64) -> Result<(), HostError> {
        let state = self.state.as_mut().ok_or(HostError::Infrastructure)?;
        if now_ns < state.observed_ns {
            return Err(HostError::InvalidRequest);
        }
        let elapsed_ns = now_ns - state.epoch_ns;
        let timestamp = u64::try_from(
            u128::from(elapsed_ns) * u128::from(state.frequency_hz) / NANOSECONDS_PER_SECOND,
        )
        .map_err(|_| HostError::InvalidRequest)?;
        state.observed_ns = now_ns;
        state.timestamp = timestamp;
        Ok(())
    }

    fn advance(&mut self, delta_ns: u64) -> Result<(), HostError> {
        let state = self.state.as_ref().ok_or(HostError::Infrastructure)?;
        let now_ns = state
            .observed_ns
            .checked_add(delta_ns)
            .ok_or(HostError::InvalidRequest)?;
        self.observe(now_ns)
    }

    pub(in crate::process) fn frequency(&self) -> Result<u64, HostError> {
        self.state
            .as_ref()
            .map(|state| state.frequency_hz)
            .ok_or(HostError::Infrastructure)
    }

    pub(in crate::process) fn timestamp(&self) -> Result<u64, HostError> {
        self.state
            .as_ref()
            .map(|state| state.timestamp)
            .ok_or(HostError::Infrastructure)
    }

    pub(in crate::process) fn monotonic_bounds(&self) -> Result<(u64, u64), HostError> {
        self.state
            .as_ref()
            .map(|state| (state.epoch_ns, state.observed_ns))
            .ok_or(HostError::Infrastructure)
    }
}

impl EngineInstance {
    /// fixes the virtual counter frequency and host monotonic epoch for this instance.
    pub fn configure_cpu_clock(
        &mut self,
        frequency_hz: u64,
        epoch_ns: u64,
    ) -> Result<(), HostError> {
        self.memory()?;
        self.cpu_clock.configure(frequency_hz, epoch_ns)
    }

    /// observes host monotonic nanoseconds; equal samples do not advance the counter.
    pub fn observe_cpu_clock(&mut self, now_ns: u64) -> Result<(), HostError> {
        self.memory()?;
        let prepared = self.prepare_synchronization_clock(now_ns)?;
        self.cpu_clock.observe(now_ns)?;
        self.commit_synchronization_clock(prepared);
        Ok(())
    }

    /// advances the same clock explicitly for deterministic execution.
    pub fn advance_cpu_clock(&mut self, delta_ns: u64) -> Result<(), HostError> {
        self.memory()?;
        let (_, previous_ns) = self.cpu_clock.monotonic_bounds()?;
        let now_ns = previous_ns
            .checked_add(delta_ns)
            .ok_or(HostError::InvalidRequest)?;
        let prepared = self.prepare_synchronization_clock(now_ns)?;
        self.cpu_clock.advance(delta_ns)?;
        self.commit_synchronization_clock(prepared);
        Ok(())
    }
}

#[cfg(test)]
mod tests;
