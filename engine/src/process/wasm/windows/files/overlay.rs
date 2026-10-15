use super::*;
use crate::{
    abi::files::encode_file_overlay,
    process::{FileId, wasm::mutate},
};
pub(crate) fn file_overlay_entry(epoch_low: u32, epoch_high: u32, index: u32) -> u32 {
    mutate(|instance| {
        let entry = instance.file_overlay_entry(wide(epoch_low, epoch_high), index as usize)?;
        let length = 96 + entry.as_ref().map_or(0, |entry| entry.path.len() * 2);
        let metadata = length > TRANSFER_SIZE;
        let mut bytes = vec![0; if metadata { 96 } else { length }];
        if metadata {
            crate::abi::files::encode_file_overlay_metadata(entry.as_ref(), &mut bytes)
        } else {
            encode_file_overlay(entry.as_ref(), &mut bytes)
        }
        .map_err(|_| HostError::Infrastructure)?;
        storage_output(instance, &bytes)
    })
}
#[allow(clippy::too_many_arguments)]
pub(crate) fn read_file_overlay(
    epoch_low: u32,
    epoch_high: u32,
    file_low: u32,
    file_high: u32,
    revision_low: u32,
    revision_high: u32,
    offset_low: u32,
    offset_high: u32,
    length: u32,
) -> u32 {
    mutate(|instance| {
        storage_guard(instance)?;
        if length > 4064 {
            return Err(HostError::InvalidRequest);
        }
        let mut bytes = [0; 4096];
        let count = instance.read_file_overlay(
            wide(epoch_low, epoch_high),
            FileId(wide(file_low, file_high)),
            wide(revision_low, revision_high),
            wide(offset_low, offset_high),
            &mut bytes[32..32 + length as usize],
        )?;
        let size = 32 + count.unwrap_or(0);
        bytes[..4].copy_from_slice(b"R3FD");
        bytes[4..6].copy_from_slice(&1u16.to_le_bytes());
        bytes[6..8].copy_from_slice(&1u16.to_le_bytes());
        bytes[8..12].copy_from_slice(&(size as u32).to_le_bytes());
        bytes[16..20].copy_from_slice(&(count.unwrap_or(0) as u32).to_le_bytes());
        bytes[20..24].copy_from_slice(&u32::from(count.is_none()).to_le_bytes());
        storage_output(instance, &bytes[..size])
    })
}
pub(crate) fn acknowledge_file_overlay(
    epoch_low: u32,
    epoch_high: u32,
    revision_low: u32,
    revision_high: u32,
) -> u32 {
    mutate(|instance| {
        instance.acknowledge_file_overlay(
            wide(epoch_low, epoch_high),
            wide(revision_low, revision_high),
        )
    })
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn read_file_overlay_path(
    epoch_low: u32,
    epoch_high: u32,
    revision_low: u32,
    revision_high: u32,
    index: u32,
    offset: u32,
    length: u32,
) -> u32 {
    mutate(|instance| {
        storage_guard(instance)?;
        if length > 2032 {
            return Err(HostError::InvalidRequest);
        }
        let mut units = [0u16; 2032];
        let count = instance.read_file_overlay_path(
            wide(epoch_low, epoch_high),
            wide(revision_low, revision_high),
            index as usize,
            offset,
            &mut units[..length as usize],
        )?;
        let mut bytes = [0u8; 4096];
        let size = 32 + count * 2;
        bytes[..4].copy_from_slice(b"R3FP");
        bytes[4..6].copy_from_slice(&1u16.to_le_bytes());
        bytes[6..8].copy_from_slice(&1u16.to_le_bytes());
        bytes[8..12].copy_from_slice(&(size as u32).to_le_bytes());
        bytes[16..20].copy_from_slice(&offset.to_le_bytes());
        bytes[20..24].copy_from_slice(&(count as u32).to_le_bytes());
        for (index, &unit) in units[..count].iter().enumerate() {
            bytes[32 + index * 2..34 + index * 2].copy_from_slice(&unit.to_le_bytes());
        }
        storage_output(instance, &bytes[..size])
    })
}
