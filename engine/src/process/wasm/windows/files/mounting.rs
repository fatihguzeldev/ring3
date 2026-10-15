use super::*;
use crate::abi::files::decode_file_mount;
use crate::process::wasm::{mutate, windows::input};

pub(crate) fn mount_file(length: u32) -> u32 {
    mutate(|instance| {
        let mount =
            decode_file_mount(input(instance, length)?).map_err(|_| HostError::InvalidRequest)?;
        let file = if let Some(asset) = mount.asset {
            instance.mount_file(&mount.path, asset, mount.attributes, mount.times)?
        } else {
            instance.mount_directory(&mount.path, mount.attributes, mount.times)?
        };
        status(instance, instance.files_epoch()?, [file.0, 0, 0, 0, 0])
    })
}
pub(crate) fn begin_file_mount(epoch_low: u32, epoch_high: u32, length: u32) -> u32 {
    mutate(|instance| {
        let start = crate::abi::files::decode_file_mount_start(input(instance, length)?)
            .map_err(|_| HostError::InvalidRequest)?;
        let pending = instance.begin_file_mount(wide(epoch_low, epoch_high), start)?;
        input_status(instance, Some(pending))
    })
}
pub(crate) fn finish_file_mount(
    epoch_low: u32,
    epoch_high: u32,
    token_low: u32,
    token_high: u32,
) -> u32 {
    mutate(|instance| {
        let epoch = wide(epoch_low, epoch_high);
        let file = instance.finish_file_mount(epoch, wide(token_low, token_high))?;
        status(instance, epoch, [file.0, 0, 0, 0, 0])
    })
}
