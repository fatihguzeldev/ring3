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
        self.sync_windows_input()?;
        self.media_contexts()?
            .input
            .source()
            .check_delivery_capacity(events.iter().copied())
            .map_err(|error| HostError::Media(MediaError::Input(error)))?;
        let limit = self.media_contexts()?.config.input.batch_events;
        if events.len() > limit {
            return Err(HostError::Media(MediaError::Input(
                InputError::InvalidCount,
            )));
        }
        let (_, observed_ns) = self.cpu_clock.monotonic_bounds()?;
        if events.iter().any(|event| event.timestamp_ns > observed_ns) {
            return Err(HostError::Media(MediaError::Input(InputError::TimeOrder)));
        }
        self.media_contexts_mut()?
            .input
            .submit_batch(epoch, sequence, events)
            .map_err(|error| HostError::Media(MediaError::Input(error)))?;
        let _ = self.sync_windows_input();
        Ok(())
    }

    pub fn submit_input_encoded(&mut self, bytes: &[u8]) -> Result<(), HostError> {
        self.sync_windows_input()?;
        self.media_contexts()?;
        let batch = wire::inspect_batch(bytes)
            .map_err(|error| HostError::Media(MediaError::Input(error)))?;
        self.media_contexts()?
            .input
            .source()
            .check_delivery_capacity(batch.events())
            .map_err(|error| HostError::Media(MediaError::Input(error)))?;
        let (_, observed_ns) = self.cpu_clock.monotonic_bounds()?;
        if batch.latest_timestamp_ns() > observed_ns {
            return Err(HostError::Media(MediaError::Input(InputError::TimeOrder)));
        }
        self.media_contexts_mut()?
            .input
            .submit_encoded(&batch)
            .map_err(|error| HostError::Media(MediaError::Input(error)))?;
        let _ = self.sync_windows_input();
        Ok(())
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

impl EngineInstance {
    pub(in crate::process) fn prepare_windows_input_flags(
        &self,
        focused: bool,
        captured: bool,
    ) -> Result<Option<crate::input::PreparedWindowInput32>, HostError> {
        let Some(media) = self.media.as_ref() else {
            return Ok(None);
        };
        let (_, timestamp) = self.cpu_clock.monotonic_bounds()?;
        media
            .input
            .prepare_window_flags(focused, captured, timestamp)
            .map(Some)
            .map_err(|error| HostError::Media(MediaError::Input(error)))
    }

    pub(in crate::process) fn commit_windows_input_flags(
        &mut self,
        prepared: Option<crate::input::PreparedWindowInput32>,
    ) {
        if let Some(prepared) = prepared {
            self.media
                .as_mut()
                .expect("prepared input context exists")
                .input
                .commit_window_flags(prepared);
        }
    }
}

impl EngineInstance {
    pub(in crate::process) fn initialize_windows_input(
        &self,
        input: &mut InputContext,
        timestamp_ns: u64,
    ) -> Result<(), HostError> {
        if let Some(windows) = self.user32_windows() {
            let prepared = input
                .prepare_window_flags(
                    windows.focus().is_some(),
                    windows.capture().is_some(),
                    timestamp_ns,
                )
                .map_err(|error| HostError::Media(MediaError::Input(error)))?;
            input.commit_window_flags(prepared);
        }
        self.initialize_user32_cursor_source(input, timestamp_ns)?;
        Ok(())
    }
}

impl EngineInstance {
    pub fn sync_windows_input(&mut self) -> Result<(), HostError> {
        self.memory()?;
        let result = (|| {
            self.refresh_dsound_focus()?;
            self.sync_dinput_input()?;
            self.sync_user32_input()
        })();
        self.windows_input_delivery_error = result.as_ref().err().copied();
        result
    }
    pub fn last_windows_input_delivery_error(&self) -> Option<HostError> {
        self.windows_input_delivery_error
    }
    pub(in crate::process) fn prepare_dinput_policy(
        &self,
    ) -> Result<Option<crate::input::source::PreparedSourceWindow32>, HostError> {
        let Some(media) = self.media.as_ref() else {
            return Ok(None);
        };
        let source = media.input.source();
        if let Some(user) = self.user32.as_ref() {
            let cursor = user
                .source_cursor()
                .unwrap_or(crate::input::source::InputCursor32 {
                    epoch: source.tail().epoch,
                    sequence: 0,
                });
            if !source.can_retain_from(cursor, 1) {
                return Err(HostError::Call(super::super::CallError::Busy));
            }
        }
        let (_, now) = self.cpu_clock.monotonic_bounds()?;
        source
            .prepare_window(now)
            .map(Some)
            .map_err(|error| HostError::Media(MediaError::Input(error)))
    }
    pub(in crate::process) fn commit_dinput_policy(
        &mut self,
        prepared: Option<crate::input::source::PreparedSourceWindow32>,
        policy: (bool, bool),
    ) {
        if let Some(prepared) = prepared {
            self.media
                .as_mut()
                .unwrap()
                .input
                .source_mut()
                .commit_policy(prepared, policy.0, policy.1);
        }
    }
    pub fn submit_windows_input(
        &mut self,
        epoch: u64,
        sequence: u64,
        events: &[TimedInputEvent],
    ) -> Result<(), HostError> {
        self.sync_windows_input()?;
        let media = self.media_contexts()?;
        if events.len() > media.config.input.batch_events {
            return Err(HostError::Media(MediaError::Input(
                InputError::InvalidCount,
            )));
        }
        media
            .input
            .source()
            .check_delivery_capacity(events.iter().copied())
            .map_err(|error| HostError::Media(MediaError::Input(error)))?;
        let (_, observed) = self.cpu_clock.monotonic_bounds()?;
        if events.iter().any(|event| event.timestamp_ns > observed) {
            return Err(HostError::Media(MediaError::Input(InputError::TimeOrder)));
        }
        self.media_contexts_mut()?
            .input
            .submit_windows_batch(epoch, sequence, events)
            .map_err(|error| HostError::Media(MediaError::Input(error)))?;
        let _ = self.sync_windows_input();
        Ok(())
    }
    pub fn submit_windows_input_encoded(&mut self, bytes: &[u8]) -> Result<(), HostError> {
        self.sync_windows_input()?;
        let batch = wire::inspect_batch(bytes)
            .map_err(|error| HostError::Media(MediaError::Input(error)))?;
        self.media_contexts()?
            .input
            .source()
            .check_delivery_capacity(batch.events())
            .map_err(|error| HostError::Media(MediaError::Input(error)))?;
        let (_, observed) = self.cpu_clock.monotonic_bounds()?;
        if batch.latest_timestamp_ns() > observed {
            return Err(HostError::Media(MediaError::Input(InputError::TimeOrder)));
        }
        self.media_contexts_mut()?
            .input
            .submit_windows_encoded(&batch)
            .map_err(|error| HostError::Media(MediaError::Input(error)))?;
        let _ = self.sync_windows_input();
        Ok(())
    }
}
