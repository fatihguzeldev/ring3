use crate::{
    abi::arena::{TRANSFER_OFFSET, TRANSFER_SIZE},
    abi::media::{MEDIA_RECORD_SIZE, MediaRecordKind, encode_media_record},
    process::{EngineInstance, HostError, MediaConfig},
};

mod audio;
mod graphics;
mod input;

pub(crate) use audio::*;
pub(crate) use graphics::*;
pub(crate) use input::*;

const RECORD_SIZE: usize = MEDIA_RECORD_SIZE;

fn wide(low: u32, high: u32) -> u64 {
    u64::from(low) | (u64::from(high) << 32)
}

fn input_bytes(instance: &EngineInstance, length: u32) -> Result<&[u8], HostError> {
    instance.memory()?;
    if length as usize > TRANSFER_SIZE {
        return Err(HostError::InvalidRequest);
    }
    Ok(&instance.arena()[TRANSFER_OFFSET..TRANSFER_OFFSET + length as usize])
}

fn record(instance: &mut EngineInstance, kind: MediaRecordKind, epoch: u64, values: [u64; 5]) {
    let bytes =
        &mut instance.arena.as_mut().get_mut()[TRANSFER_OFFSET..TRANSFER_OFFSET + RECORD_SIZE];
    encode_media_record(
        kind,
        epoch,
        values,
        bytes.try_into().expect("fixed transfer record span"),
    );
}

pub(crate) fn configure_media(sample_rate: u32) -> u32 {
    super::mutate(|instance| {
        let mut config = MediaConfig::default();
        config.audio.sample_rate = sample_rate;
        let epoch = instance.configure_media(config)?;
        record(instance, MediaRecordKind::Session, epoch, [0; 5]);
        Ok(())
    })
}

pub(crate) fn reset_media() -> u32 {
    super::mutate(|instance| {
        let epoch = instance.reset_media()?;
        record(instance, MediaRecordKind::Session, epoch, [0; 5]);
        Ok(())
    })
}

pub(super) fn status(error: crate::process::MediaError) -> u32 {
    use crate::{
        audio::AudioError32 as A, graphics::GraphicsError as G, input::InputError as I,
        process::MediaError as M,
    };
    match error {
        M::NotConfigured => 38,
        M::AlreadyConfigured => 39,
        M::Graphics(error) => match error {
            G::InvalidLimits
            | G::InvalidEpoch
            | G::InvalidVertexData
            | G::InvalidColor
            | G::InvalidSurface
            | G::InvalidRange
            | G::EmptyBatch => 7,
            G::Busy => 12,
            G::StaleResource | G::StaleBatch => 40,
            G::IdentityExhausted | G::Allocation => 9,
            G::ResourceCapacity | G::ByteCapacity | G::BatchCapacity | G::OutputTooSmall => 18,
        },
        M::Audio(error) => match error {
            A::InvalidConfig | A::InvalidRequest => 7,
            A::UnsupportedFormat => 20,
            A::InvalidBuffer | A::InvalidVoice | A::InvalidSubmission => 40,
            A::Busy | A::BufferInUse => 12,
            A::Capacity => 18,
            A::Allocation | A::Exhausted => 9,
        },
        M::Input(error) => match error {
            I::InvalidLimits | I::InvalidEvent | I::InvalidCount => 7,
            I::InvalidWire => 19,
            I::Allocation | I::Exhausted => 9,
            I::StaleSession => 40,
            I::SequenceOrder | I::TimeOrder => 41,
            I::QueueFull | I::OutputCapacity => 18,
            I::NotFocused => 42,
            I::NotCaptured => 43,
        },
    }
}
