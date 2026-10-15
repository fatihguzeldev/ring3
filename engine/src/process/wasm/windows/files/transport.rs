use super::*;

pub(super) fn storage_guard(instance: &EngineInstance) -> Result<(), HostError> {
    if instance.is_open() {
        Ok(())
    } else {
        Err(HostError::Closed)
    }
}
pub(super) fn storage_input(instance: &EngineInstance, length: u32) -> Result<&[u8], HostError> {
    storage_guard(instance)?;
    if length as usize > TRANSFER_SIZE {
        return Err(HostError::InvalidRequest);
    }
    Ok(&instance.arena()[TRANSFER_OFFSET..TRANSFER_OFFSET + length as usize])
}
pub(super) fn storage_output(instance: &mut EngineInstance, bytes: &[u8]) -> Result<(), HostError> {
    storage_guard(instance)?;
    if bytes.len() > TRANSFER_SIZE {
        return Err(HostError::InvalidRequest);
    }
    instance.arena.as_mut().get_mut()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(bytes);
    Ok(())
}
pub(super) fn output(instance: &mut EngineInstance, bytes: &[u8]) -> Result<(), HostError> {
    if bytes.len() > TRANSFER_SIZE {
        return Err(HostError::InvalidRequest);
    }
    instance.arena_mut()?[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()].copy_from_slice(bytes);
    Ok(())
}
fn status_bytes(epoch: u64, values: [u64; 5]) -> [u8; 64] {
    let mut bytes = [0; 64];
    bytes[..4].copy_from_slice(b"R3FS");
    bytes[4..6].copy_from_slice(&1u16.to_le_bytes());
    bytes[6..8].copy_from_slice(&1u16.to_le_bytes());
    bytes[8..12].copy_from_slice(&64u32.to_le_bytes());
    bytes[16..24].copy_from_slice(&epoch.to_le_bytes());
    for (index, value) in values.iter().enumerate() {
        bytes[24 + index * 8..32 + index * 8].copy_from_slice(&value.to_le_bytes());
    }
    bytes
}
pub(super) fn status(
    instance: &mut EngineInstance,
    epoch: u64,
    values: [u64; 5],
) -> Result<(), HostError> {
    output(instance, &status_bytes(epoch, values))
}
pub(super) fn storage_status(
    instance: &mut EngineInstance,
    epoch: u64,
    values: [u64; 5],
) -> Result<(), HostError> {
    storage_output(instance, &status_bytes(epoch, values))
}
