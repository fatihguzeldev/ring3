use super::*;
use crate::process::{AssetRef32, FileTimes32};

pub struct FileMount32 {
    pub path: Vec<u16>,
    pub asset: Option<AssetRef32>,
    pub attributes: u32,
    pub times: FileTimes32,
}
pub fn decode_file_mount(bytes: &[u8]) -> Result<FileMount32, AbiError> {
    if bytes.len() < 64 {
        return Err(AbiError::Length);
    }
    header(bytes, *b"R3FM", bytes.len())?;
    let count = word(bytes, 36) as usize;
    if count == 0 || count > 260 || bytes.len() != 64 + count * 2 {
        return Err(AbiError::Length);
    }
    let id = wide(bytes, 16);
    let length = wide(bytes, 24);
    if id == 0 && length != 0 {
        return Err(AbiError::Flags);
    }
    let mut path = Vec::new();
    path.try_reserve_exact(count)
        .map_err(|_| AbiError::Length)?;
    path.extend(
        bytes[64..]
            .chunks_exact(2)
            .map(|unit| u16::from_le_bytes(unit.try_into().unwrap())),
    );
    Ok(FileMount32 {
        path,
        asset: (id != 0).then_some(AssetRef32 { id, length }),
        attributes: word(bytes, 32),
        times: FileTimes32 {
            created: wide(bytes, 40),
            accessed: wide(bytes, 48),
            written: wide(bytes, 56),
        },
    })
}
pub fn encode_file_mount(mount: &FileMount32, bytes: &mut [u8]) -> Result<(), AbiError> {
    if mount.path.is_empty() || mount.path.len() > 260 || bytes.len() != 64 + mount.path.len() * 2 {
        return Err(AbiError::Length);
    }
    if mount.asset.is_some_and(|asset| asset.id == 0) {
        return Err(AbiError::Flags);
    }
    initialize(bytes, *b"R3FM");
    let asset = mount.asset.unwrap_or(AssetRef32 { id: 0, length: 0 });
    put_wide(bytes, 16, asset.id);
    put_wide(bytes, 24, asset.length);
    put(bytes, 32, mount.attributes);
    put(bytes, 36, mount.path.len() as u32);
    put_wide(bytes, 40, mount.times.created);
    put_wide(bytes, 48, mount.times.accessed);
    put_wide(bytes, 56, mount.times.written);
    for (i, unit) in mount.path.iter().enumerate() {
        bytes[64 + i * 2..66 + i * 2].copy_from_slice(&unit.to_le_bytes());
    }
    Ok(())
}

pub fn decode_file_mount_start(bytes: &[u8]) -> Result<crate::process::FileMountStart32, AbiError> {
    header(bytes, *b"R3FB", 64)?;
    let units = word(bytes, 36);
    let id = wide(bytes, 16);
    let length = wide(bytes, 24);
    if units == 0 || units > 32767 || id == 0 && length != 0 {
        return Err(AbiError::Flags);
    }
    Ok(crate::process::FileMountStart32 {
        units,
        asset: (id != 0).then_some(AssetRef32 { id, length }),
        attributes: word(bytes, 32),
        times: FileTimes32 {
            created: wide(bytes, 40),
            accessed: wide(bytes, 48),
            written: wide(bytes, 56),
        },
    })
}
pub fn encode_file_mount_start(
    start: crate::process::FileMountStart32,
    bytes: &mut [u8],
) -> Result<(), AbiError> {
    if bytes.len() != 64 {
        return Err(AbiError::Length);
    }
    if start.units == 0 || start.units > 32767 || start.asset.is_some_and(|a| a.id == 0) {
        return Err(AbiError::Flags);
    }
    initialize(bytes, *b"R3FB");
    let asset = start.asset.unwrap_or(AssetRef32 { id: 0, length: 0 });
    put_wide(bytes, 16, asset.id);
    put_wide(bytes, 24, asset.length);
    put(bytes, 32, start.attributes);
    put(bytes, 36, start.units);
    put_wide(bytes, 40, start.times.created);
    put_wide(bytes, 48, start.times.accessed);
    put_wide(bytes, 56, start.times.written);
    Ok(())
}
