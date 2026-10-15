use crate::files::{ATTR_DIRECTORY, FileError32, FileInfo32};

pub(super) fn basic(info: FileInfo32, changed: u64) -> Vec<u8> {
    let mut bytes = vec![0; 40];
    for (index, value) in [
        info.times.created,
        info.times.accessed,
        info.times.written,
        changed,
    ]
    .into_iter()
    .enumerate()
    {
        bytes[index * 8..index * 8 + 8].copy_from_slice(&value.to_le_bytes());
    }
    bytes[32..36].copy_from_slice(&info.attributes.to_le_bytes());
    bytes
}
pub(super) fn standard(info: FileInfo32, allocation: u64) -> Vec<u8> {
    let mut bytes = vec![0; 24];
    bytes[..8].copy_from_slice(&allocation.to_le_bytes());
    bytes[8..16].copy_from_slice(&info.size.to_le_bytes());
    bytes[16..20].copy_from_slice(&info.links.to_le_bytes());
    bytes[20] = u8::from(info.delete_pending);
    bytes[21] = u8::from(info.attributes & ATTR_DIRECTORY != 0);
    bytes
}
pub(super) fn name(path: &[u16]) -> Result<Vec<u8>, FileError32> {
    let length = path.len().checked_mul(2).ok_or(FileError32::Capacity)?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(4 + length)
        .map_err(|_| FileError32::Allocation)?;
    bytes.extend_from_slice(&(length as u32).to_le_bytes());
    for unit in path {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    Ok(bytes)
}
pub(super) fn stream(size: u64, allocation: u64) -> Result<Vec<u8>, FileError32> {
    let name: Vec<u16> = "::$DATA".encode_utf16().collect();
    let mut bytes = vec![0; 24 + name.len() * 2];
    bytes[4..8].copy_from_slice(&((name.len() * 2) as u32).to_le_bytes());
    bytes[8..16].copy_from_slice(&size.to_le_bytes());
    bytes[16..24].copy_from_slice(&allocation.to_le_bytes());
    for (index, unit) in name.into_iter().enumerate() {
        bytes[24 + index * 2..26 + index * 2].copy_from_slice(&unit.to_le_bytes());
    }
    Ok(bytes)
}
pub(super) fn compression(size: u64) -> Vec<u8> {
    let mut bytes = vec![0; 16];
    bytes[..8].copy_from_slice(&size.to_le_bytes());
    bytes
}
pub(super) fn storage() -> Vec<u8> {
    let mut bytes = vec![0; 28];
    for index in 0..4 {
        bytes[index * 4..index * 4 + 4].copy_from_slice(&512u32.to_le_bytes());
    }
    bytes[16..20].copy_from_slice(&3u32.to_le_bytes());
    bytes
}
