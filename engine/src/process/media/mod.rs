use super::{EngineInstance, HostError};
use crate::{
    audio::{AudioConfig32, AudioContext, AudioError32},
    graphics::{GraphicsContext, GraphicsError, GraphicsLimits},
    input::{InputContext, InputError, InputLimits},
};
use std::sync::atomic::{AtomicU64, Ordering};

mod audio;
mod graphics;
mod input;

static NEXT_SESSION: AtomicU64 = AtomicU64::new(1);

pub(in crate::process) fn next_session() -> Result<u64, HostError> {
    NEXT_SESSION
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |epoch| {
            epoch.checked_add(1)
        })
        .map_err(|_| HostError::GenerationExhausted)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MediaConfig {
    pub graphics: GraphicsLimits,
    pub audio: AudioConfig32,
    pub input: InputLimits,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaError {
    NotConfigured,
    AlreadyConfigured,
    Graphics(GraphicsError),
    Audio(AudioError32),
    Input(InputError),
}

pub(in crate::process) struct MediaContexts {
    config: MediaConfig,
    pub(in crate::process) graphics: GraphicsContext,
    pub(in crate::process) audio: AudioContext,
    pub(in crate::process) input: InputContext,
}

impl MediaContexts {
    fn new(epoch: u64, clock_floor_ns: u64, config: MediaConfig) -> Result<Self, HostError> {
        Ok(Self {
            graphics: GraphicsContext::new(epoch, config.graphics)
                .map_err(|error| HostError::Media(MediaError::Graphics(error)))?,
            audio: AudioContext::new(epoch, config.audio)
                .map_err(|error| HostError::Media(MediaError::Audio(error)))?,
            input: InputContext::new(epoch, clock_floor_ns, config.input)
                .map_err(|error| HostError::Media(MediaError::Input(error)))?,
            config,
        })
    }
}

impl EngineInstance {
    pub fn configure_media(&mut self, config: MediaConfig) -> Result<u64, HostError> {
        self.memory()?;
        if self.media.is_some() {
            return Err(HostError::Media(MediaError::AlreadyConfigured));
        }
        let (origin_ns, _) = self.cpu_clock.monotonic_bounds()?;
        let epoch = next_session()?;
        let mut media = MediaContexts::new(epoch, origin_ns, config)?;
        self.initialize_windows_input(&mut media.input, origin_ns)?;
        self.media = Some(media);
        self.media_epoch = epoch;
        Ok(epoch)
    }

    pub fn reset_media(&mut self) -> Result<u64, HostError> {
        let config = self.media_contexts()?.config;
        let (_, now_ns) = self.cpu_clock.monotonic_bounds()?;
        let epoch = next_session()?;
        // prepare every owner before replacing any live context.
        let mut replacement = MediaContexts::new(epoch, now_ns, config)?;
        self.initialize_windows_input(&mut replacement.input, now_ns)?;
        self.media = Some(replacement);
        self.media_epoch = epoch;
        Ok(epoch)
    }

    pub fn media_epoch(&self) -> Result<u64, HostError> {
        self.media_contexts()?;
        Ok(self.media_epoch)
    }

    pub(in crate::process) fn media_contexts(&self) -> Result<&MediaContexts, HostError> {
        self.memory()?;
        self.media
            .as_ref()
            .ok_or(HostError::Media(MediaError::NotConfigured))
    }

    pub(in crate::process) fn media_contexts_mut(
        &mut self,
    ) -> Result<&mut MediaContexts, HostError> {
        self.memory()?;
        self.media
            .as_mut()
            .ok_or(HostError::Media(MediaError::NotConfigured))
    }
}
