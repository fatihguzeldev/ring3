use super::*;
use crate::process::wasm::{mutate, windows::input};

pub(crate) fn begin_file_environment(epoch_low: u32, epoch_high: u32, length: u32) -> u32 {
    mutate(|instance| {
        let lengths = crate::abi::files::decode_file_environment_start(input(instance, length)?)
            .map_err(|_| HostError::InvalidRequest)?;
        let pending = instance.begin_file_environment(wide(epoch_low, epoch_high), &lengths)?;
        input_status(instance, Some(pending))
    })
}
pub(crate) fn set_files_environment(epoch_low: u32, epoch_high: u32, length: u32) -> u32 {
    mutate(|instance| {
        let environment = crate::abi::files::decode_file_environment(input(instance, length)?)
            .map_err(|_| HostError::InvalidRequest)?;
        instance.set_files_environment(wide(epoch_low, epoch_high), environment)
    })
}
pub(crate) fn finish_file_environment(
    epoch_low: u32,
    epoch_high: u32,
    token_low: u32,
    token_high: u32,
) -> u32 {
    mutate(|instance| {
        instance.finish_file_environment(wide(epoch_low, epoch_high), wide(token_low, token_high))
    })
}
