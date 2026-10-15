use super::*;
use crate::abi::files::decode_file_config;
use crate::process::wasm::{mutate, windows::input};

pub(crate) fn configure_files(length: u32) -> u32 {
    mutate(|instance| {
        let config =
            decode_file_config(input(instance, length)?).map_err(|_| HostError::InvalidRequest)?;
        instance.configure_files(config)?;
        status(instance, instance.files_epoch()?, [0; 5])
    })
}
pub(crate) fn files_info() -> u32 {
    mutate(|instance| storage_status(instance, instance.files_epoch()?, [0; 5]))
}
pub(crate) fn set_files_time(
    epoch_low: u32,
    epoch_high: u32,
    time_low: u32,
    time_high: u32,
) -> u32 {
    mutate(|instance| {
        instance.set_files_time(wide(epoch_low, epoch_high), wide(time_low, time_high))
    })
}
