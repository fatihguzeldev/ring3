use super::{input_bytes, record, wide};
use crate::abi::media::MediaRecordKind;
use crate::{
    abi::arena::{TRANSFER_OFFSET, TRANSFER_SIZE},
    graphics::{BatchId, ResourceId},
    process::{HostError, MediaError, wasm::mutate},
};

pub(crate) fn graphics_create_vertex_buffer(length: u32) -> u32 {
    mutate(|instance| {
        let mut bytes = [0; TRANSFER_SIZE];
        let input = input_bytes(instance, length)?;
        bytes[..input.len()].copy_from_slice(input);
        let id = instance.graphics_create_vertex_buffer(&bytes[..length as usize])?;
        record(
            instance,
            MediaRecordKind::GraphicsResource,
            id.epoch,
            [u64::from(id.slot), u64::from(id.generation), 0, 0, 0],
        );
        Ok(())
    })
}

pub(crate) fn graphics_update_vertex_buffer(
    epoch_low: u32,
    epoch_high: u32,
    slot: u32,
    generation: u32,
    first: u32,
    length: u32,
) -> u32 {
    mutate(|instance| {
        let mut bytes = [0; TRANSFER_SIZE];
        let input = input_bytes(instance, length)?;
        bytes[..input.len()].copy_from_slice(input);
        instance.graphics_update_vertex_buffer(
            ResourceId {
                epoch: wide(epoch_low, epoch_high),
                slot,
                generation,
            },
            first,
            &bytes[..length as usize],
        )
    })
}

pub(crate) fn graphics_destroy_vertex_buffer(
    epoch_low: u32,
    epoch_high: u32,
    slot: u32,
    generation: u32,
) -> u32 {
    mutate(|instance| {
        instance.graphics_destroy_vertex_buffer(ResourceId {
            epoch: wide(epoch_low, epoch_high),
            slot,
            generation,
        })
    })
}

pub(crate) fn graphics_draw_triangles(
    epoch_low: u32,
    epoch_high: u32,
    slot: u32,
    generation: u32,
    first: u32,
    count: u32,
) -> u32 {
    mutate(|instance| {
        instance.graphics_draw_triangles(
            ResourceId {
                epoch: wide(epoch_low, epoch_high),
                slot,
                generation,
            },
            first,
            count,
        )
    })
}

pub(crate) fn graphics_clear(r: u32, g: u32, b: u32, a: u32) -> u32 {
    mutate(|instance| instance.graphics_clear([r, g, b, a].map(f32::from_bits)))
}

pub(crate) fn graphics_present(surface: u32) -> u32 {
    mutate(|instance| instance.graphics_present(surface))
}

pub(crate) fn graphics_seal_batch() -> u32 {
    mutate(|instance| {
        let id = instance.graphics_seal_batch()?;
        let size = instance
            .graphics()?
            .peek_batch()
            .map_or(0, |(_, bytes)| bytes.len());
        record(
            instance,
            MediaRecordKind::GraphicsBatch,
            instance.media_epoch()?,
            [id.map_or(0, |id| id.sequence), size as u64, 0, 0, 0],
        );
        Ok(())
    })
}

pub(crate) fn graphics_read_batch(
    epoch_low: u32,
    epoch_high: u32,
    seq_low: u32,
    seq_high: u32,
    offset: u32,
    length: u32,
) -> u32 {
    mutate(|instance| {
        if length as usize > TRANSFER_SIZE {
            return Err(HostError::InvalidRequest);
        }
        let id = BatchId {
            epoch: wide(epoch_low, epoch_high),
            sequence: wide(seq_low, seq_high),
        };
        let graphics = instance.graphics()?;
        if graphics.peek_batch().map(|(pending, _)| pending) != Some(id) {
            return Err(HostError::Media(MediaError::Graphics(
                crate::graphics::GraphicsError::StaleBatch,
            )));
        }
        let mut bytes = [0; TRANSFER_SIZE];
        graphics
            .copy_batch_range(offset, &mut bytes[..length as usize])
            .map_err(|error| HostError::Media(MediaError::Graphics(error)))?;
        instance.arena.as_mut().get_mut()[TRANSFER_OFFSET..TRANSFER_OFFSET + length as usize]
            .copy_from_slice(&bytes[..length as usize]);
        Ok(())
    })
}

pub(crate) fn graphics_acknowledge_batch(
    epoch_low: u32,
    epoch_high: u32,
    seq_low: u32,
    seq_high: u32,
) -> u32 {
    mutate(|instance| {
        instance.graphics_acknowledge_batch(BatchId {
            epoch: wide(epoch_low, epoch_high),
            sequence: wide(seq_low, seq_high),
        })
    })
}
