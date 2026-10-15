mod configuration;
mod environment;
mod mounting;
mod records;

use super::AbiError;
pub use configuration::{decode_file_config, encode_file_config};
pub use environment::{
    decode_file_environment, decode_file_environment_start, encode_file_environment,
    encode_file_environment_start,
};
pub use mounting::{
    FileMount32, decode_file_mount, decode_file_mount_start, encode_file_mount,
    encode_file_mount_start,
};
pub use records::{
    decode_file_request, encode_file_overlay, encode_file_overlay_metadata, encode_file_request,
};

fn header(bytes: &[u8], magic: [u8; 4], size: usize) -> Result<(), AbiError> {
    if bytes.len() != size {
        return Err(AbiError::Length);
    }
    if bytes[..4] != magic {
        return Err(AbiError::Magic);
    }
    if u16::from_le_bytes(bytes[4..6].try_into().unwrap()) != 1 {
        return Err(AbiError::Version);
    }
    if u16::from_le_bytes(bytes[6..8].try_into().unwrap()) != 1 {
        return Err(AbiError::Profile);
    }
    if word(bytes, 8) as usize != size {
        return Err(AbiError::Length);
    }
    if word(bytes, 12) != 0 {
        return Err(AbiError::Reserved);
    }
    Ok(())
}
fn initialize(bytes: &mut [u8], magic: [u8; 4]) {
    bytes.fill(0);
    let size = bytes.len() as u32;
    bytes[..4].copy_from_slice(&magic);
    bytes[4..6].copy_from_slice(&1u16.to_le_bytes());
    bytes[6..8].copy_from_slice(&1u16.to_le_bytes());
    put(bytes, 8, size);
}
fn word(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}
fn wide(bytes: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(bytes[at..at + 8].try_into().unwrap())
}
fn put(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}
fn put_wide(bytes: &mut [u8], at: usize, value: u64) {
    bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
}

#[cfg(test)]
mod tests;
