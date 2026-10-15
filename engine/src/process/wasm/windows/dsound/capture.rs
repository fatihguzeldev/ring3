use crate::{
    abi::{
        arena::TRANSFER_SIZE,
        media::{MEDIA_RECORD_SIZE, MediaRecordKind},
    },
    audio::{
        PcmFormat32, SampleEncoding32,
        dsound::{CaptureRequest, WaveFormat},
    },
    process::{
        HostError,
        wasm::{
            mutate,
            windows::{input, record, wide},
        },
    },
};

pub(crate) fn dsound_capture_request(index: u32) -> u32 {
    mutate(|instance| {
        let (epoch, values) = if let Some(request) = instance.dsound_capture_request(index)? {
            let format = request.format;
            let flags = u64::from(format.pcm.channels)
                | u64::from(format.pcm.bits_per_sample) << 8
                | u64::from(format.encoding == SampleEncoding32::Float) << 16
                | u64::from(format.extensible) << 17
                | u64::from(request.looping) << 18
                | u64::from(format.channel_mask) << 32;
            (
                request.epoch,
                [
                    request.object,
                    request.revision,
                    request.first_frame,
                    u64::from(request.maximum_frames) | u64::from(format.pcm.sample_rate) << 32,
                    flags,
                ],
            )
        } else {
            (0, [0; 5])
        };
        record(instance, MediaRecordKind::DSoundCapture, epoch, values);
        Ok(())
    })
}

pub(crate) fn dsound_receive_capture(length: u32) -> u32 {
    mutate(|instance| {
        let mut owned = [0; TRANSFER_SIZE];
        let bytes = input(instance, length)?;
        if bytes.len() <= MEDIA_RECORD_SIZE {
            return Err(HostError::InvalidRequest);
        }
        owned[..bytes.len()].copy_from_slice(bytes);
        let bytes = &owned[..length as usize];
        if &bytes[..4] != b"R3MR"
            || u16::from_le_bytes(bytes[4..6].try_into().unwrap()) != 1
            || u16::from_le_bytes(bytes[6..8].try_into().unwrap())
                != MediaRecordKind::DSoundCapture as u16
            || u32::from_le_bytes(bytes[8..12].try_into().unwrap()) != MEDIA_RECORD_SIZE as u32
            || bytes[12..16].iter().any(|value| *value != 0)
        {
            return Err(HostError::InvalidRequest);
        }
        let long = |at| u64::from_le_bytes(bytes[at..at + 8].try_into().unwrap());
        let packed = long(48);
        let flags = long(56);
        if flags & 0xfff8_0000 != 0 {
            return Err(HostError::InvalidRequest);
        }
        let request = CaptureRequest {
            epoch: long(16),
            object: long(24),
            revision: long(32),
            first_frame: long(40),
            maximum_frames: packed as u32,
            looping: flags & (1 << 18) != 0,
            format: WaveFormat {
                pcm: PcmFormat32 {
                    sample_rate: (packed >> 32) as u32,
                    channels: (flags & 0xff) as u16,
                    bits_per_sample: ((flags >> 8) & 0xff) as u16,
                },
                encoding: if flags & (1 << 16) != 0 {
                    SampleEncoding32::Float
                } else {
                    SampleEncoding32::Integer
                },
                extensible: flags & (1 << 17) != 0,
                channel_mask: (flags >> 32) as u32,
            },
        };
        instance.dsound_receive_capture(request, &bytes[MEDIA_RECORD_SIZE..])
    })
}

pub(crate) fn dsound_capture_availability(epoch_low: u32, epoch_high: u32, available: u32) -> u32 {
    mutate(|instance| {
        if available > 1 {
            return Err(HostError::InvalidRequest);
        }
        instance.dsound_capture_availability(wide(epoch_low, epoch_high), available != 0)
    })
}

pub(crate) fn dsound_observe_playback_loss(epoch_low: u32, epoch_high: u32, lost: u32) -> u32 {
    mutate(|instance| {
        if lost > 1 {
            return Err(HostError::InvalidRequest);
        }
        instance.dsound_observe_playback_loss(wide(epoch_low, epoch_high), lost != 0)
    })
}
