use super::*;
use crate::process::{FileId, FileReadRequest32, OverlayEntry32, OverlayKind32};

pub fn encode_file_request(
    request: Option<FileReadRequest32>,
    bytes: &mut [u8],
) -> Result<(), AbiError> {
    if bytes.len() != 80 {
        return Err(AbiError::Length);
    }
    initialize(bytes, *b"R3FQ");
    if let Some(request) = request {
        for (at, value) in [
            (16, request.epoch),
            (24, request.request),
            (32, request.file.0),
            (40, request.revision),
            (48, request.asset),
            (56, request.offset),
        ] {
            put_wide(bytes, at, value);
        }
        put(bytes, 64, request.length);
    }
    Ok(())
}
pub fn decode_file_request(bytes: &[u8]) -> Result<FileReadRequest32, AbiError> {
    header(bytes, *b"R3FQ", 80)?;
    if bytes[68..].iter().any(|byte| *byte != 0) {
        return Err(AbiError::Reserved);
    }
    let result = FileReadRequest32 {
        epoch: wide(bytes, 16),
        request: wide(bytes, 24),
        file: FileId(wide(bytes, 32)),
        revision: wide(bytes, 40),
        asset: wide(bytes, 48),
        offset: wide(bytes, 56),
        length: word(bytes, 64),
    };
    if result.epoch == 0
        || result.request == 0
        || result.file.0 == 0
        || result.asset == 0
        || result.length == 0
        || result.length > 65_536
    {
        return Err(AbiError::Flags);
    }
    Ok(result)
}
pub fn encode_file_overlay(
    entry: Option<&OverlayEntry32>,
    bytes: &mut [u8],
) -> Result<(), AbiError> {
    overlay(entry, bytes, false)
}
pub fn encode_file_overlay_metadata(
    entry: Option<&OverlayEntry32>,
    bytes: &mut [u8],
) -> Result<(), AbiError> {
    overlay(entry, bytes, true)
}
fn overlay(
    entry: Option<&OverlayEntry32>,
    bytes: &mut [u8],
    metadata: bool,
) -> Result<(), AbiError> {
    let count = entry.map_or(0, |entry| entry.path.len());
    if count > 32767 || bytes.len() != 96 + if metadata { 0 } else { count * 2 } {
        return Err(AbiError::Length);
    }
    initialize(bytes, *b"R3FO");
    if let Some(entry) = entry {
        put_wide(bytes, 16, entry.epoch);
        put_wide(bytes, 24, entry.revision);
        put(
            bytes,
            52,
            match entry.kind {
                OverlayKind32::File => 1,
                OverlayKind32::Directory => 2,
                OverlayKind32::Deleted => 3,
            },
        );
        put(bytes, 56, count as u32);
        put(bytes, 60, u32::from(metadata));
        if let Some(info) = entry.info {
            put_wide(bytes, 32, info.id.0);
            put_wide(bytes, 40, info.size);
            put(bytes, 48, info.attributes);
            put_wide(bytes, 64, info.times.created);
            put_wide(bytes, 72, info.times.accessed);
            put_wide(bytes, 80, info.times.written);
        }
        if !metadata {
            for (i, unit) in entry.path.iter().enumerate() {
                bytes[96 + i * 2..98 + i * 2].copy_from_slice(&unit.to_le_bytes());
            }
        }
    }
    Ok(())
}
