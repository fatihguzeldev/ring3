mod configuration;
mod environment;
mod mounting;
mod overlay;
mod progress;
mod uploads;
pub(crate) use configuration::*;
pub(crate) use environment::*;
pub(crate) use mounting::*;
pub(crate) use overlay::*;
pub(crate) use progress::*;
use uploads::input_status;
pub(crate) use uploads::*;

use crate::{
    abi::arena::{TRANSFER_OFFSET, TRANSFER_SIZE},
    process::{EngineInstance, HostError},
};
fn wide(low: u32, high: u32) -> u64 {
    u64::from(low) | (u64::from(high) << 32)
}
fn output(instance: &mut EngineInstance, bytes: &[u8]) -> Result<(), HostError> {
    if bytes.len() > TRANSFER_SIZE {
        return Err(HostError::InvalidRequest);
    }
    instance.arena_mut()?[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()].copy_from_slice(bytes);
    Ok(())
}
fn status(instance: &mut EngineInstance, epoch: u64, values: [u64; 5]) -> Result<(), HostError> {
    let mut bytes = [0; 64];
    bytes[..4].copy_from_slice(b"R3FS");
    bytes[4..6].copy_from_slice(&1u16.to_le_bytes());
    bytes[6..8].copy_from_slice(&1u16.to_le_bytes());
    bytes[8..12].copy_from_slice(&64u32.to_le_bytes());
    bytes[16..24].copy_from_slice(&epoch.to_le_bytes());
    for (i, value) in values.iter().enumerate() {
        bytes[24 + i * 8..32 + i * 8].copy_from_slice(&value.to_le_bytes());
    }
    output(instance, &bytes)
}
