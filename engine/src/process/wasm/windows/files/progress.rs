use super::*;
use crate::{
    abi::files::{decode_file_request, encode_file_request},
    process::wasm::mutate,
};
pub(crate) fn pending_file_request() -> u32 {
    mutate(|instance| {
        let request = instance.pending_file_request()?;
        let mut bytes = [0; 80];
        encode_file_request(request, &mut bytes).map_err(|_| HostError::Infrastructure)?;
        storage_output(instance, &bytes)
    })
}
pub(crate) fn supply_file_bytes(offset: u32, length: u32) -> u32 {
    mutate(|instance| {
        storage_guard(instance)?;
        if length > 4016 || length == 0 {
            return Err(HostError::InvalidRequest);
        }
        let input = storage_input(instance, 80 + length)?;
        let request = decode_file_request(&input[..80]).map_err(|_| HostError::InvalidRequest)?;
        let mut bytes = [0; 4016];
        bytes[..length as usize].copy_from_slice(&input[80..]);
        instance.supply_file_bytes(request, offset, &bytes[..length as usize])
    })
}
pub(crate) fn pending_file_call() -> u32 {
    mutate(|instance| {
        let wait = instance.pending_file_call();
        status(
            instance,
            wait.map_or(0, |wait| wait.epoch),
            [wait.map_or(0, |wait| u64::from(wait.token)), 0, 0, 0, 0],
        )
    })
}
pub(crate) fn progress_file_io(epoch_low: u32, epoch_high: u32) -> u32 {
    mutate(|instance| {
        let epoch = wide(epoch_low, epoch_high);
        let progress = instance.progress_file_io(epoch)?;
        status(
            instance,
            epoch,
            [
                progress.operation,
                u64::from(progress.error),
                u64::from(progress.bytes),
                u64::from(progress.pending_bytes),
                0,
            ],
        )
    })
}
pub(crate) fn resume_file_call(low: u32, high: u32, token: u32) -> u32 {
    mutate(|instance| instance.resume_file_call(wide(low, high), token))
}
pub(crate) fn cancel_file_call(low: u32, high: u32, token: u32) -> u32 {
    mutate(|instance| instance.cancel_file_call(wide(low, high), token))
}

pub(crate) fn cancel_file_request(length: u32) -> u32 {
    mutate(|instance| {
        let request = decode_file_request(storage_input(instance, length)?)
            .map_err(|_| HostError::InvalidRequest)?;
        instance.cancel_file_request(request)
    })
}
