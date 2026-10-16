use super::wide;
use crate::{
    abi::{
        arena::{TRANSFER_OFFSET, TRANSFER_SIZE},
        windows::dialogs::{
            WindowsDialogIcon32, WindowsDialogRecord32, encode_windows_dialog_page,
        },
    },
    process::{EngineInstance, HostError, User32DialogIcon32, User32DialogRequest32, wasm::mutate},
};

fn record(request: User32DialogRequest32<'_>) -> WindowsDialogRecord32<'_> {
    WindowsDialogRecord32 {
        epoch: request.epoch,
        id: request.id,
        thread: request.thread,
        owner: request.owner,
        style: request.style,
        language: request.language,
        help_context: request.help_context,
        caption: request.caption,
        text: request.text,
        icon: match request.icon {
            User32DialogIcon32::Standard(value) => WindowsDialogIcon32::Standard(value),
            User32DialogIcon32::Resource {
                instance,
                id: Some(id),
                ..
            } => WindowsDialogIcon32::Resource { instance, id },
            User32DialogIcon32::Resource {
                instance,
                id: None,
                name,
            } => WindowsDialogIcon32::NamedResource { instance, name },
        },
    }
}
fn publish(instance: &mut EngineInstance, bytes: &[u8]) {
    let output =
        &mut instance.arena.as_mut().get_mut()[TRANSFER_OFFSET..TRANSFER_OFFSET + TRANSFER_SIZE];
    output.fill(0);
    output[..bytes.len()].copy_from_slice(bytes);
}
pub(crate) fn pending_user32_dialog(index: u32) -> u32 {
    mutate(|instance| {
        let mut bytes = [0; TRANSFER_SIZE];
        let length = encode_windows_dialog_page(
            instance.user32_dialog_request(index)?.map(record),
            0,
            0,
            0,
            &mut bytes,
        )
        .map_err(|_| HostError::InvalidRequest)?;
        publish(instance, &bytes[..length]);
        Ok(())
    })
}
pub(crate) fn read_user32_dialog_text(
    epoch_low: u32,
    epoch_high: u32,
    id_low: u32,
    id_high: u32,
    field: u32,
    offset: u32,
    units: u32,
) -> u32 {
    mutate(|instance| {
        if !matches!(field, 1..=3) {
            return Err(HostError::InvalidRequest);
        }
        let epoch = wide(epoch_low, epoch_high);
        let id = wide(id_low, id_high);
        let mut index = 0;
        let mut bytes = [0; TRANSFER_SIZE];
        let length = loop {
            let Some(request) = instance.user32_dialog_request(index)? else {
                return Err(HostError::InvalidRequest);
            };
            if request.epoch == epoch && request.id == id {
                break encode_windows_dialog_page(
                    Some(record(request)),
                    field,
                    offset,
                    units,
                    &mut bytes,
                )
                .map_err(|_| HostError::InvalidRequest)?;
            }
            index = index.checked_add(1).ok_or(HostError::InvalidRequest)?;
        };
        publish(instance, &bytes[..length]);
        Ok(())
    })
}
pub(crate) fn complete_user32_dialog(
    epoch_low: u32,
    epoch_high: u32,
    id_low: u32,
    id_high: u32,
    button: u32,
    error: u32,
) -> u32 {
    mutate(|instance| {
        instance.complete_user32_dialog(
            wide(epoch_low, epoch_high),
            wide(id_low, id_high),
            button,
            error,
        )
    })
}
pub(crate) fn request_user32_dialog_help(
    epoch_low: u32,
    epoch_high: u32,
    id_low: u32,
    id_high: u32,
) -> u32 {
    mutate(|instance| {
        instance.request_user32_dialog_help(wide(epoch_low, epoch_high), wide(id_low, id_high))
    })
}
