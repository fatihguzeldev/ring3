use super::super::{DSoundError32, objects::ObjectKind32};
use crate::{
    audio::{AudioBufferId32, AudioContext, dsound::Buffer},
    memory::PageRange,
    process::{EngineInstance, HostError, windows::objects::PreparedGuestLifetime32},
    windows::synchronization::EventBinding32,
};

pub struct PreparedDSoundMediaReset32 {
    buffers: Vec<(usize, Buffer)>,
    captures: Vec<usize>,
    guest: Option<PreparedGuestLifetime32>,
    notifications: Vec<(EventBinding32, u64)>,
}
impl PreparedDSoundMediaReset32 {
    pub fn notifications(&self) -> &[(EventBinding32, u64)] {
        &self.notifications
    }
}
impl EngineInstance {
    pub(in crate::process) fn prepare_dsound_media_reset(
        &self,
        replacement: &mut AudioContext,
    ) -> Result<PreparedDSoundMediaReset32, HostError> {
        let mut prepared = PreparedDSoundMediaReset32 {
            buffers: Vec::new(),
            captures: Vec::new(),
            guest: None,
            notifications: Vec::new(),
        };
        let Some(runtime) = self.dsound.as_ref() else {
            return Ok(prepared);
        };
        let original = self.audio()?;
        prepared
            .buffers
            .try_reserve_exact(runtime.records.len())
            .map_err(|_| HostError::DSound(DSoundError32::Allocation))?;
        prepared
            .captures
            .try_reserve_exact(runtime.records.len())
            .map_err(|_| HostError::DSound(DSoundError32::Allocation))?;
        prepared
            .notifications
            .try_reserve_exact(runtime.config.max_notifications as usize)
            .map_err(|_| HostError::DSound(DSoundError32::Allocation))?;
        let mut shared: Vec<(AudioBufferId32, AudioBufferId32)> = Vec::new();
        shared
            .try_reserve_exact(runtime.records.len())
            .map_err(|_| HostError::DSound(DSoundError32::Allocation))?;
        for (index, object) in runtime.records.iter().enumerate() {
            let Some(object) = object else {
                continue;
            };
            match &object.kind {
                ObjectKind32::Buffer(buffer) => {
                    let storage = if let Some((_, storage)) =
                        shared.iter().find(|(old, _)| *old == buffer.native.storage)
                    {
                        *storage
                    } else {
                        let length = original
                            .stream_bytes(buffer.native.storage)
                            .map_err(audio_error)?
                            .len() as u32;
                        let storage = replacement
                            .create_stream_buffer(
                                buffer.native.format.pcm,
                                buffer.native.format.encoding,
                                length,
                            )
                            .map_err(audio_error)?;
                        shared.push((buffer.native.storage, storage));
                        storage
                    };
                    let voice = replacement
                        .create_stream_voice(storage)
                        .map_err(audio_error)?;
                    let mut native = buffer.native;
                    native.storage = storage;
                    native.voice = voice;
                    native.lost = true;
                    native
                        .apply_controls(
                            replacement,
                            &runtime
                                .device(buffer.device)
                                .map_err(|_| HostError::DSound(DSoundError32::InvalidRequest))?
                                .native,
                        )
                        .map_err(|_| HostError::DSound(DSoundError32::InvalidRequest))?;
                    if original
                        .voice_state(buffer.native.voice)
                        .map_err(audio_error)?
                        .playing
                    {
                        prepared.notifications.extend(
                            buffer
                                .notifications
                                .iter()
                                .filter(|notification| notification.offset == u32::MAX)
                                .map(|notification| (notification.binding, 1)),
                        );
                    }
                    prepared.buffers.push((index, native));
                }
                ObjectKind32::CaptureBuffer(buffer) => {
                    buffer
                        .native
                        .can_invalidate()
                        .map_err(|_| HostError::DSound(DSoundError32::InvalidRequest))?;
                    if buffer.native.status() & 1 != 0 {
                        prepared.notifications.extend(
                            buffer
                                .notifications
                                .iter()
                                .filter(|notification| notification.offset == u32::MAX)
                                .map(|notification| (notification.binding, 1)),
                        );
                    }
                    prepared.captures.push(index);
                }
                _ => {}
            }
        }
        let memory = self.memory.as_ref().ok_or(HostError::Closed)?;
        let mut ranges: Vec<PageRange> = Vec::new();
        ranges
            .try_reserve_exact(runtime.locks.entries.len())
            .map_err(|_| HostError::DSound(DSoundError32::Allocation))?;
        for lock in &runtime.locks.entries {
            runtime
                .locks
                .validate(memory, lock)
                .map_err(|_| HostError::DSound(DSoundError32::InvalidRequest))?;
            ranges.push(lock.range);
        }
        ranges.sort_unstable_by_key(|range| range.first);
        ranges.dedup();
        prepared.guest = Some(
            runtime
                .com
                .prepare_lifetime_changes_with_unmaps(memory, &[], &[], &[], &[], &[], &ranges)
                .map_err(|_| HostError::DSound(DSoundError32::InvalidRequest))?,
        );
        Ok(prepared)
    }
    pub(in crate::process) fn commit_dsound_media_reset(
        &mut self,
        prepared: PreparedDSoundMediaReset32,
    ) {
        let Some(runtime) = self.dsound.as_mut() else {
            assert!(prepared.guest.is_none());
            return;
        };
        runtime
            .com
            .commit_lifetime_changes(self.memory.as_mut().unwrap(), prepared.guest.unwrap())
            .expect("sound reset guest mappings were preflighted");
        runtime.locks.entries.clear();
        for (index, native) in prepared.buffers {
            let ObjectKind32::Buffer(buffer) = &mut runtime.records[index].as_mut().unwrap().kind
            else {
                unreachable!()
            };
            buffer.native = native;
        }
        for index in prepared.captures {
            let ObjectKind32::CaptureBuffer(buffer) =
                &mut runtime.records[index].as_mut().unwrap().kind
            else {
                unreachable!()
            };
            buffer
                .native
                .invalidate()
                .expect("capture reset was preflighted");
        }
    }
}
fn audio_error(value: crate::audio::AudioError32) -> HostError {
    HostError::Media(crate::process::MediaError::Audio(value))
}
