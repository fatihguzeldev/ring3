use super::{RECORD_SIZE, input_bytes, record, wide};
use crate::abi::media::MediaRecordKind;
use crate::{
    abi::arena::{TRANSFER_OFFSET, TRANSFER_SIZE},
    audio::{AudioBufferId32, AudioError32, AudioVoiceId32, PcmFormat32},
    process::{HostError, MediaError, wasm::mutate},
};

pub(crate) fn audio_create_buffer(sample_rate: u32, channels: u32, bits: u32, length: u32) -> u32 {
    mutate(|instance| {
        let channels = u16::try_from(channels).map_err(|_| HostError::InvalidRequest)?;
        let bits_per_sample = u16::try_from(bits).map_err(|_| HostError::InvalidRequest)?;
        let id = instance.audio_create_buffer(
            PcmFormat32 {
                sample_rate,
                channels,
                bits_per_sample,
            },
            length,
        )?;
        record(
            instance,
            MediaRecordKind::AudioBuffer,
            id.epoch,
            [u64::from(id.slot), u64::from(id.generation), 0, 0, 0],
        );
        Ok(())
    })
}

pub(crate) fn audio_write_buffer(
    epoch_low: u32,
    epoch_high: u32,
    slot: u32,
    generation: u32,
    offset: u32,
    length: u32,
) -> u32 {
    mutate(|instance| {
        let mut bytes = [0; TRANSFER_SIZE];
        let input = input_bytes(instance, length)?;
        bytes[..input.len()].copy_from_slice(input);
        instance.audio_write_buffer(
            AudioBufferId32 {
                epoch: wide(epoch_low, epoch_high),
                slot,
                generation,
            },
            offset,
            &bytes[..length as usize],
        )
    })
}

pub(crate) fn audio_release_buffer(
    epoch_low: u32,
    epoch_high: u32,
    slot: u32,
    generation: u32,
) -> u32 {
    mutate(|instance| {
        instance.audio_release_buffer(AudioBufferId32 {
            epoch: wide(epoch_low, epoch_high),
            slot,
            generation,
        })
    })
}

pub(crate) fn audio_create_voice(
    epoch_low: u32,
    epoch_high: u32,
    slot: u32,
    generation: u32,
) -> u32 {
    mutate(|instance| {
        let id = instance.audio_create_voice(AudioBufferId32 {
            epoch: wide(epoch_low, epoch_high),
            slot,
            generation,
        })?;
        record(
            instance,
            MediaRecordKind::AudioVoice,
            id.epoch,
            [u64::from(id.slot), u64::from(id.generation), 0, 0, 0],
        );
        Ok(())
    })
}

pub(crate) fn audio_release_voice(
    epoch_low: u32,
    epoch_high: u32,
    slot: u32,
    generation: u32,
) -> u32 {
    mutate(|instance| {
        instance.audio_release_voice(AudioVoiceId32 {
            epoch: wide(epoch_low, epoch_high),
            slot,
            generation,
        })
    })
}

pub(crate) fn audio_play(
    epoch_low: u32,
    epoch_high: u32,
    slot: u32,
    generation: u32,
    looping: u32,
) -> u32 {
    mutate(|instance| {
        if looping > 1 {
            return Err(HostError::InvalidRequest);
        }
        instance.audio_play(
            AudioVoiceId32 {
                epoch: wide(epoch_low, epoch_high),
                slot,
                generation,
            },
            looping != 0,
        )
    })
}

pub(crate) fn audio_stop(epoch_low: u32, epoch_high: u32, slot: u32, generation: u32) -> u32 {
    mutate(|instance| {
        instance.audio_stop(AudioVoiceId32 {
            epoch: wide(epoch_low, epoch_high),
            slot,
            generation,
        })
    })
}

pub(crate) fn audio_seek(
    epoch_low: u32,
    epoch_high: u32,
    slot: u32,
    generation: u32,
    frame: u32,
) -> u32 {
    mutate(|instance| {
        instance.audio_seek(
            AudioVoiceId32 {
                epoch: wide(epoch_low, epoch_high),
                slot,
                generation,
            },
            frame,
        )
    })
}

pub(crate) fn audio_submit(frames: u32) -> u32 {
    mutate(|instance| {
        if frames as usize > (TRANSFER_SIZE - RECORD_SIZE) / 8 {
            return Err(HostError::InvalidRequest);
        }
        let id = instance.audio_submit(frames)?;
        let mut bytes = [0; TRANSFER_SIZE - RECORD_SIZE];
        let pcm = instance
            .audio()?
            .submission_pcm(id)
            .map_err(|error| HostError::Media(MediaError::Audio(error)))?;
        for (chunk, sample) in bytes.chunks_exact_mut(4).zip(pcm) {
            chunk.copy_from_slice(&sample.to_le_bytes());
        }
        record(
            instance,
            MediaRecordKind::AudioSubmission,
            id.epoch,
            [id.sequence, id.first_frame, u64::from(id.frames), 0, 0],
        );
        let length = frames as usize * 8;
        instance.arena.as_mut().get_mut()
            [TRANSFER_OFFSET + RECORD_SIZE..TRANSFER_OFFSET + RECORD_SIZE + length]
            .copy_from_slice(&bytes[..length]);
        Ok(())
    })
}

pub(crate) fn audio_acknowledge(
    epoch_low: u32,
    epoch_high: u32,
    seq_low: u32,
    seq_high: u32,
    consumed: u32,
) -> u32 {
    mutate(|instance| {
        let id = instance
            .audio()?
            .state()
            .pending
            .filter(|id| {
                id.epoch == wide(epoch_low, epoch_high) && id.sequence == wide(seq_low, seq_high)
            })
            .ok_or(HostError::Media(MediaError::Audio(
                AudioError32::InvalidSubmission,
            )))?;
        instance.audio_acknowledge(id, consumed)
    })
}

pub(crate) fn audio_state() -> u32 {
    mutate(|instance| {
        let state = instance.audio()?.state();
        record(
            instance,
            MediaRecordKind::AudioState,
            state.epoch,
            [
                state.submitted_frames,
                state.consumed_frames,
                state.pending.map_or(0, |id| id.sequence),
                state.pending.map_or(0, |id| u64::from(id.frames)),
                0,
            ],
        );
        Ok(())
    })
}

pub(crate) fn audio_read_submission(
    epoch_low: u32,
    epoch_high: u32,
    seq_low: u32,
    seq_high: u32,
    first: u32,
    frames: u32,
) -> u32 {
    mutate(|instance| {
        if frames as usize > TRANSFER_SIZE / 8 {
            return Err(HostError::InvalidRequest);
        }
        let audio = instance.audio()?;
        let id = audio
            .state()
            .pending
            .filter(|id| {
                id.epoch == wide(epoch_low, epoch_high) && id.sequence == wide(seq_low, seq_high)
            })
            .ok_or(HostError::Media(MediaError::Audio(
                AudioError32::InvalidSubmission,
            )))?;
        let end = first
            .checked_add(frames)
            .filter(|end| *end <= id.frames)
            .ok_or(HostError::InvalidRequest)?;
        let pcm = audio
            .submission_pcm(id)
            .map_err(|error| HostError::Media(MediaError::Audio(error)))?;
        let mut bytes = [0; TRANSFER_SIZE];
        let length = frames as usize * 8;
        for (chunk, sample) in bytes[..length]
            .chunks_exact_mut(4)
            .zip(&pcm[first as usize * 2..end as usize * 2])
        {
            chunk.copy_from_slice(&sample.to_le_bytes());
        }
        instance.arena.as_mut().get_mut()[TRANSFER_OFFSET..TRANSFER_OFFSET + length]
            .copy_from_slice(&bytes[..length]);
        Ok(())
    })
}

pub(crate) fn audio_voice_state(
    epoch_low: u32,
    epoch_high: u32,
    slot: u32,
    generation: u32,
) -> u32 {
    mutate(|instance| {
        let id = AudioVoiceId32 {
            epoch: wide(epoch_low, epoch_high),
            slot,
            generation,
        };
        let state = instance
            .audio()?
            .voice_state(id)
            .map_err(|error| HostError::Media(MediaError::Audio(error)))?;
        let flags = u64::from(state.playing)
            | u64::from(state.producing) << 1
            | u64::from(state.looping) << 2;
        record(
            instance,
            MediaRecordKind::AudioVoiceState,
            id.epoch,
            [
                u64::from(state.position_frames),
                u64::from(state.produced_position_frames),
                flags,
                u64::from(state.buffer.slot),
                u64::from(state.buffer.generation),
            ],
        );
        Ok(())
    })
}
