use super::{header, word};
use crate::{
    abi::arena::TRANSFER_SIZE,
    input::source::ControllerState32,
    process::{
        HostError,
        wasm::{
            mutate,
            windows::{input, wide},
        },
    },
};

pub(crate) fn submit_windows_input(length: u32) -> u32 {
    mutate(|instance| {
        let mut bytes = [0; TRANSFER_SIZE];
        let source = input(instance, length)?;
        bytes[..source.len()].copy_from_slice(source);
        instance.submit_windows_input_encoded(&bytes[..length as usize])
    })
}

pub(crate) fn sync_windows_input() -> u32 {
    mutate(|instance| instance.sync_windows_input())
}

pub(crate) fn windows_input_time() -> u32 {
    mutate(|instance| {
        let (epoch, floor, observed) = instance.windows_input_time()?;
        super::super::record(
            instance,
            crate::abi::media::MediaRecordKind::InputTime,
            epoch,
            [floor, observed, 0, 0, 0],
        );
        Ok(())
    })
}

pub(crate) fn last_windows_input_delivery_error() -> u32 {
    mutate(|instance| {
        instance.memory()?;
        let error = instance
            .last_windows_input_delivery_error()
            .map_or(0, crate::process::wasm::status);
        let epoch = instance.media_epoch().unwrap_or(0);
        crate::process::wasm::windows::record(
            instance,
            crate::abi::media::MediaRecordKind::WindowsInputDelivery,
            epoch,
            [u64::from(error), 0, 0, 0, 0],
        );
        Ok(())
    })
}

pub(crate) fn submit_windows_pointer(
    epoch_low: u32,
    epoch_high: u32,
    time_low: u32,
    time_high: u32,
    x: u32,
    y: u32,
) -> u32 {
    mutate(|instance| {
        instance.submit_windows_pointer(
            wide(epoch_low, epoch_high),
            wide(time_low, time_high),
            x as i32,
            y as i32,
        )
    })
}

pub(crate) fn submit_controller_input(epoch_low: u32, epoch_high: u32, length: u32) -> u32 {
    mutate(|instance| {
        let bytes = input(instance, length)?;
        header(bytes, b"R3IS", 192)?;
        if bytes.len() != 192 || word(bytes, 20) > 1 {
            return Err(HostError::InvalidRequest);
        }
        let slot = u8::try_from(word(bytes, 16)).map_err(|_| HostError::InvalidRequest)?;
        let timestamp_ns = u64::from_le_bytes(bytes[24..32].try_into().unwrap());
        let state = ControllerState32 {
            connected: word(bytes, 20) != 0,
            axes: std::array::from_fn(|index| word(bytes, 32 + index * 4) as i32),
            pov: std::array::from_fn(|index| word(bytes, 160 + index * 4)),
            buttons: std::array::from_fn(|index| bytes[176 + index / 8] & (1 << (index % 8)) != 0),
        };
        instance.submit_controller_input(wide(epoch_low, epoch_high), slot, timestamp_ns, state)
    })
}
