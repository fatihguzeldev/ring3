use crate::{
    abi::{
        arena::{TRANSFER_OFFSET, TRANSFER_SIZE},
        media::{MEDIA_RECORD_SIZE, MediaRecordKind, encode_media_record},
    },
    process::{EngineInstance, HostError},
};

mod activation;
mod callbacks;
mod configuration;
mod d3d8;
mod dinput;
mod dsound;
mod exceptions;
mod files;
mod nls;
mod process;
mod registry;
mod synchronization;
mod threads;
mod user32;

pub(crate) use activation::*;
pub(crate) use callbacks::*;
pub(crate) use configuration::*;
pub(crate) use d3d8::*;
pub(crate) use dinput::*;
pub(crate) use dsound::*;
pub(crate) use exceptions::*;
pub(crate) use files::*;
pub(crate) use nls::*;
pub(crate) use process::*;
pub(crate) use registry::*;
pub(crate) use synchronization::*;
pub(crate) use threads::*;
pub(crate) use user32::*;

fn wide(low: u32, high: u32) -> u64 {
    u64::from(low) | (u64::from(high) << 32)
}
fn input(instance: &EngineInstance, length: u32) -> Result<&[u8], HostError> {
    instance.memory()?;
    if length as usize > TRANSFER_SIZE {
        return Err(HostError::InvalidRequest);
    }
    Ok(&instance.arena()[TRANSFER_OFFSET..TRANSFER_OFFSET + length as usize])
}
fn record(instance: &mut EngineInstance, kind: MediaRecordKind, epoch: u64, values: [u64; 5]) {
    encode_media_record(
        kind,
        epoch,
        values,
        (&mut instance.arena.as_mut().get_mut()
            [TRANSFER_OFFSET..TRANSFER_OFFSET + MEDIA_RECORD_SIZE])
            .try_into()
            .unwrap(),
    );
}
