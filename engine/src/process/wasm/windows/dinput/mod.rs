mod configuration;
mod haptics;
mod input;

pub(crate) use configuration::*;
pub(crate) use haptics::*;
pub(crate) use input::*;

use crate::process::HostError;

fn header(bytes: &[u8], magic: &[u8; 4], minimum: usize) -> Result<(), HostError> {
    if bytes.len() < minimum
        || &bytes[..4] != magic
        || u16::from_le_bytes(bytes[4..6].try_into().unwrap()) != 1
        || u16::from_le_bytes(bytes[6..8].try_into().unwrap()) != 1
        || word(bytes, 8) as usize != bytes.len()
        || word(bytes, 12) != 0
    {
        return Err(HostError::InvalidRequest);
    }
    Ok(())
}

fn word(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}
