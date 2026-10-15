use super::{EngineInstance, HostError, MediaError};
use crate::audio::{AudioBufferId32, AudioContext, AudioSubmission32, AudioVoiceId32, PcmFormat32};

impl EngineInstance {
    pub fn audio(&self) -> Result<&AudioContext, HostError> {
        Ok(&self.media_contexts()?.audio)
    }

    pub fn audio_create_buffer(
        &mut self,
        format: PcmFormat32,
        byte_length: u32,
    ) -> Result<AudioBufferId32, HostError> {
        self.media_contexts_mut()?
            .audio
            .create_buffer(format, byte_length)
            .map_err(|error| HostError::Media(MediaError::Audio(error)))
    }

    pub fn audio_write_buffer(
        &mut self,
        id: AudioBufferId32,
        byte_offset: u32,
        bytes: &[u8],
    ) -> Result<(), HostError> {
        self.media_contexts_mut()?
            .audio
            .write_buffer(id, byte_offset, bytes)
            .map_err(|error| HostError::Media(MediaError::Audio(error)))
    }

    pub fn audio_release_buffer(&mut self, id: AudioBufferId32) -> Result<(), HostError> {
        self.media_contexts_mut()?
            .audio
            .release_buffer(id)
            .map_err(|error| HostError::Media(MediaError::Audio(error)))
    }

    pub fn audio_create_voice(
        &mut self,
        buffer: AudioBufferId32,
    ) -> Result<AudioVoiceId32, HostError> {
        self.media_contexts_mut()?
            .audio
            .create_voice(buffer)
            .map_err(|error| HostError::Media(MediaError::Audio(error)))
    }

    pub fn audio_release_voice(&mut self, id: AudioVoiceId32) -> Result<(), HostError> {
        self.media_contexts_mut()?
            .audio
            .release_voice(id)
            .map_err(|error| HostError::Media(MediaError::Audio(error)))
    }

    pub fn audio_play(&mut self, id: AudioVoiceId32, looping: bool) -> Result<(), HostError> {
        self.media_contexts_mut()?
            .audio
            .play(id, looping)
            .map_err(|error| HostError::Media(MediaError::Audio(error)))
    }

    pub fn audio_stop(&mut self, id: AudioVoiceId32) -> Result<(), HostError> {
        self.media_contexts_mut()?
            .audio
            .stop(id)
            .map_err(|error| HostError::Media(MediaError::Audio(error)))
    }

    pub fn audio_seek(&mut self, id: AudioVoiceId32, frame: u32) -> Result<(), HostError> {
        self.media_contexts_mut()?
            .audio
            .seek(id, frame)
            .map_err(|error| HostError::Media(MediaError::Audio(error)))
    }

    pub fn audio_submit(&mut self, frames: u32) -> Result<AudioSubmission32, HostError> {
        self.media_contexts_mut()?
            .audio
            .submit(frames)
            .map_err(|error| HostError::Media(MediaError::Audio(error)))
    }

    pub fn audio_acknowledge(
        &mut self,
        submission: AudioSubmission32,
        cumulative_consumed_frames: u32,
    ) -> Result<(), HostError> {
        self.media_contexts_mut()?
            .audio
            .acknowledge(submission, cumulative_consumed_frames)
            .map_err(|error| HostError::Media(MediaError::Audio(error)))
    }
}
