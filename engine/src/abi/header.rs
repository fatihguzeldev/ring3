use super::{ABI_VERSION, AbiError, X86_INTEGER_PROFILE};

pub(crate) fn validate(bytes: &[u8], magic: [u8; 4], size: usize) -> Result<(), AbiError> {
    if bytes.len() != size {
        return Err(AbiError::Length);
    }
    if bytes[..4] != magic {
        return Err(AbiError::Magic);
    }
    if u16::from_le_bytes([bytes[4], bytes[5]]) != ABI_VERSION {
        return Err(AbiError::Version);
    }
    if u16::from_le_bytes([bytes[6], bytes[7]]) != X86_INTEGER_PROFILE {
        return Err(AbiError::Profile);
    }
    if read_u32(bytes, 8) != size as u32 {
        return Err(AbiError::Length);
    }
    if read_u32(bytes, 12) != 0 {
        return Err(AbiError::Reserved);
    }
    Ok(())
}

pub(crate) fn write(bytes: &mut [u8], magic: [u8; 4]) {
    bytes.fill(0);
    bytes[..4].copy_from_slice(&magic);
    bytes[4..6].copy_from_slice(&ABI_VERSION.to_le_bytes());
    bytes[6..8].copy_from_slice(&X86_INTEGER_PROFILE.to_le_bytes());
    write_u32(bytes, 8, bytes.len() as u32);
}

pub(crate) fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

pub(crate) fn write_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}
