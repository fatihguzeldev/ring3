use super::{input, record, wide};
use crate::{
    abi::{
        arena::{TRANSFER_OFFSET, TRANSFER_SIZE},
        media::MediaRecordKind,
    },
    graphics::{BatchId, d3d8::device::DeviceStatus},
    process::{EngineInstance, HostError, wasm::mutate},
};

pub(crate) fn register_d3d8_module(base: u32) -> u32 {
    mutate(|instance| instance.register_d3d8_module(base))
}
pub(crate) fn d3d8_device(index: u32) -> u32 {
    mutate(|instance| {
        let device = instance.d3d8_device(index as usize)?;
        record(
            instance,
            MediaRecordKind::D3D8Device,
            device.map_or(0, |device| device.epoch),
            device.map_or([0; 5], |device| {
                [
                    u64::from(device.pointer.0),
                    device.generation,
                    u64::from(device.retired),
                    device.status as u64,
                    device.binding_lifetime_skips,
                ]
            }),
        );
        Ok(())
    })
}
pub(crate) fn d3d8_seal_batch(low: u32, high: u32) -> u32 {
    mutate(|instance| {
        let epoch = wide(low, high);
        let batch = instance.d3d8_seal_batch(epoch)?;
        let values = if batch.is_some() {
            batch_values(instance, epoch)?
        } else {
            [0; 5]
        };
        record(instance, MediaRecordKind::D3D8Batch, epoch, values);
        Ok(())
    })
}
pub(crate) fn d3d8_batch_info(low: u32, high: u32) -> u32 {
    mutate(|instance| {
        let epoch = wide(low, high);
        let values = batch_values(instance, epoch)?;
        record(instance, MediaRecordKind::D3D8Batch, epoch, values);
        Ok(())
    })
}
fn batch_values(instance: &EngineInstance, epoch: u64) -> Result<[u64; 5], HostError> {
    Ok(instance
        .d3d8_batch_bytes(epoch)?
        .map_or([0; 5], |(id, bytes)| {
            // the sealed bytes are borrowed until the next engine call or acknowledgement.
            #[cfg(target_arch = "wasm32")]
            let borrowed = [u64::from(bytes.as_ptr() as u32), 1];
            #[cfg(not(target_arch = "wasm32"))]
            let borrowed = [0, 0];
            [id.sequence, bytes.len() as u64, borrowed[0], borrowed[1], 0]
        }))
}
pub(crate) fn d3d8_read_batch(
    low: u32,
    high: u32,
    sequence_low: u32,
    sequence_high: u32,
    offset: u32,
    length: u32,
) -> u32 {
    mutate(|instance| {
        if length as usize > TRANSFER_SIZE {
            return Err(HostError::InvalidRequest);
        }
        let epoch = wide(low, high);
        let (batch, size) = instance
            .d3d8_batch_info(epoch)?
            .ok_or(HostError::D3D8(crate::process::D3D8Error32::StaleReply))?;
        if batch.sequence != wide(sequence_low, sequence_high) {
            return Err(HostError::D3D8(crate::process::D3D8Error32::StaleReply));
        }
        if offset
            .checked_add(length)
            .is_none_or(|end| end as usize > size)
        {
            return Err(HostError::InvalidRequest);
        }
        let mut bytes = [0; TRANSFER_SIZE];
        let copied = instance.d3d8_copy_batch(epoch, offset, &mut bytes[..length as usize])?;
        if copied != length as usize {
            return Err(HostError::Infrastructure);
        }
        instance.arena.as_mut().get_mut()[TRANSFER_OFFSET..TRANSFER_OFFSET + copied]
            .copy_from_slice(&bytes[..copied]);
        Ok(())
    })
}
pub(crate) fn d3d8_acknowledge_batch(
    low: u32,
    high: u32,
    sequence_low: u32,
    sequence_high: u32,
) -> u32 {
    mutate(|instance| {
        instance.d3d8_acknowledge_batch(BatchId {
            epoch: wide(low, high),
            sequence: wide(sequence_low, sequence_high),
        })?;
        Ok(())
    })
}
pub(crate) fn d3d8_observe_device(
    low: u32,
    high: u32,
    generation_low: u32,
    generation_high: u32,
    status: u32,
) -> u32 {
    mutate(|instance| {
        let status = match status {
            1 => DeviceStatus::Lost,
            2 => DeviceStatus::Resettable,
            3 => DeviceStatus::Removed,
            _ => return Err(HostError::InvalidRequest),
        };
        instance.d3d8_observe_device(
            wide(low, high),
            wide(generation_low, generation_high),
            status,
        )?;
        Ok(())
    })
}
pub(crate) fn begin_d3d8_readback_input() -> u32 {
    mutate(|instance| {
        let mut header = [0; 64];
        header.copy_from_slice(input(instance, 64)?);
        instance.begin_d3d8_readback_input(&header)
    })
}
pub(crate) fn append_d3d8_readback_input(offset: u32, length: u32) -> u32 {
    mutate(|instance| {
        let mut bytes = [0; TRANSFER_SIZE];
        let source = input(instance, length)?;
        bytes[..source.len()].copy_from_slice(source);
        instance.append_d3d8_readback_input(offset, &bytes[..length as usize])
    })
}
pub(crate) fn finish_d3d8_readback_input() -> u32 {
    mutate(EngineInstance::finish_d3d8_readback_input)
}
pub(crate) fn abort_d3d8_readback_input() -> u32 {
    mutate(EngineInstance::abort_d3d8_readback_input)
}
