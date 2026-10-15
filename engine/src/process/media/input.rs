use super::{EngineInstance, HostError, MediaError};
use crate::input::{
    InputContext, InputError, InputLease, InputRecord, QueueRead, RelativeTotals, TimedInputEvent,
    wire,
};

impl EngineInstance {
    pub fn input(&self) -> Result<&InputContext, HostError> {
        Ok(&self.media_contexts()?.input)
    }

    pub fn submit_input(
        &mut self,
        epoch: u64,
        sequence: u64,
        events: &[TimedInputEvent],
    ) -> Result<(), HostError> {
        self.media_contexts()?;
        let (_, observed_ns) = self.cpu_clock.monotonic_bounds()?;
        if events.iter().any(|event| event.timestamp_ns > observed_ns) {
            return Err(HostError::Media(MediaError::Input(InputError::TimeOrder)));
        }
        self.media_contexts_mut()?
            .input
            .submit_batch(epoch, sequence, events)
            .map_err(|error| HostError::Media(MediaError::Input(error)))
    }

    pub fn submit_input_encoded(&mut self, bytes: &[u8]) -> Result<(), HostError> {
        self.media_contexts()?;
        let batch = wire::inspect_batch(bytes)
            .map_err(|error| HostError::Media(MediaError::Input(error)))?;
        let (_, observed_ns) = self.cpu_clock.monotonic_bounds()?;
        if batch.latest_timestamp_ns() > observed_ns {
            return Err(HostError::Media(MediaError::Input(InputError::TimeOrder)));
        }
        self.media_contexts_mut()?
            .input
            .submit_encoded(&batch)
            .map_err(|error| HostError::Media(MediaError::Input(error)))
    }

    pub fn input_acquire(&mut self, require_capture: bool) -> Result<InputLease, HostError> {
        self.media_contexts_mut()?
            .input
            .acquire(require_capture)
            .map_err(|error| HostError::Media(MediaError::Input(error)))
    }

    pub fn input_read(
        &mut self,
        max: usize,
        output: &mut [InputRecord],
    ) -> Result<usize, HostError> {
        self.media_contexts_mut()?
            .input
            .read(max, output)
            .map_err(|error| HostError::Media(MediaError::Input(error)))
    }

    pub fn input_consume(&mut self, count: usize) -> Result<(), HostError> {
        self.media_contexts_mut()?
            .input
            .consume(count)
            .map_err(|error| HostError::Media(MediaError::Input(error)))
    }

    pub fn input_read_encoded(
        &mut self,
        max: usize,
        output: &mut [u8],
    ) -> Result<QueueRead, HostError> {
        self.media_contexts_mut()?
            .input
            .read_encoded(max, output)
            .map_err(|error| HostError::Media(MediaError::Input(error)))
    }

    pub fn input_drain_relative(&mut self) -> Result<RelativeTotals, HostError> {
        Ok(self.media_contexts_mut()?.input.drain_relative())
    }
}
