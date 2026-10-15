use super::*;
use crate::process::wasm::{mutate, windows::input};

pub(crate) fn append_file_input(
    epoch_low: u32,
    epoch_high: u32,
    token_low: u32,
    token_high: u32,
    offset: u32,
    length: u32,
) -> u32 {
    mutate(|instance| {
        if length == 0 || length > 2048 {
            return Err(HostError::InvalidRequest);
        }
        let bytes = input(instance, length * 2)?;
        let mut units = [0u16; 2048];
        for (index, pair) in bytes.chunks_exact(2).enumerate() {
            units[index] = u16::from_le_bytes(pair.try_into().unwrap());
        }
        let pending = instance.append_file_input(
            wide(epoch_low, epoch_high),
            wide(token_low, token_high),
            offset,
            &units[..length as usize],
        )?;
        input_status(instance, Some(pending))
    })
}
pub(crate) fn abort_file_input(
    epoch_low: u32,
    epoch_high: u32,
    token_low: u32,
    token_high: u32,
) -> u32 {
    mutate(|instance| {
        instance.abort_file_input(wide(epoch_low, epoch_high), wide(token_low, token_high))
    })
}
pub(crate) fn pending_file_input() -> u32 {
    mutate(|instance| {
        let pending = instance.pending_file_input()?;
        input_status(instance, pending)
    })
}
pub(super) fn input_status(
    instance: &mut EngineInstance,
    input: Option<crate::process::FileInput32>,
) -> Result<(), HostError> {
    status(
        instance,
        input.map_or(0, |p| p.epoch),
        input.map_or([0; 5], |p| {
            [
                p.token,
                u64::from(p.kind),
                u64::from(p.received),
                u64::from(p.total),
                0,
            ]
        }),
    )
}
