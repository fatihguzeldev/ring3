use super::{
    CodecError, PROFILE, VERSION,
    packet::{u16_at, u32_at, u64_at},
};
use crate::graphics::{
    ResourceId,
    d3d8::{
        commands::{ReadbackId, SurfaceVersion},
        resources::SurfaceRef,
    },
};

pub const HEADER_SIZE: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum ReadbackStatus {
    Complete = 0,
    DeviceLost = 1,
    DeviceRemoved = 2,
    Allocation = 3,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReadbackReceipt<'a> {
    pub id: ReadbackId,
    pub source: SurfaceVersion,
    pub status: ReadbackStatus,
    pub bytes: &'a [u8],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReadbackHeader {
    pub id: ReadbackId,
    pub source: SurfaceVersion,
    pub status: ReadbackStatus,
    pub payload_bytes: u32,
}

pub fn readback_header(bytes: &[u8]) -> Result<ReadbackHeader, CodecError> {
    if bytes.len() != HEADER_SIZE {
        return Err(CodecError::Length);
    }
    if &bytes[..4] != b"R3R8" {
        return Err(CodecError::Magic);
    }
    if u16_at(bytes, 4) != VERSION || u16_at(bytes, 6) != PROFILE {
        return Err(CodecError::Version);
    }
    let payload_bytes = u32_at(bytes, 12);
    if payload_bytes.checked_add(HEADER_SIZE as u32) != Some(u32_at(bytes, 8)) {
        return Err(CodecError::Length);
    }
    let header = ReadbackHeader {
        id: ReadbackId {
            epoch: u64_at(bytes, 16),
            sequence: u64_at(bytes, 24),
        },
        source: SurfaceVersion {
            surface: SurfaceRef {
                resource: ResourceId {
                    epoch: u64_at(bytes, 32),
                    slot: u32_at(bytes, 40),
                    generation: u32_at(bytes, 44),
                },
                subresource: u32_at(bytes, 56),
            },
            version: u64_at(bytes, 48),
        },
        status: match u32_at(bytes, 60) {
            0 => ReadbackStatus::Complete,
            1 => ReadbackStatus::DeviceLost,
            2 => ReadbackStatus::DeviceRemoved,
            3 => ReadbackStatus::Allocation,
            _ => return Err(CodecError::InvalidCommand),
        },
        payload_bytes,
    };
    validate(ReadbackReceipt {
        id: header.id,
        source: header.source,
        status: header.status,
        bytes: &[],
    })?;
    if header.status != ReadbackStatus::Complete && payload_bytes != 0 {
        return Err(CodecError::InvalidCommand);
    }
    Ok(header)
}

pub fn encode_readback_receipt(
    receipt: ReadbackReceipt<'_>,
    output: &mut [u8],
) -> Result<usize, CodecError> {
    let length = HEADER_SIZE
        .checked_add(receipt.bytes.len())
        .filter(|length| *length <= u32::MAX as usize)
        .ok_or(CodecError::Length)?;
    if output.len() < length {
        return Err(CodecError::Length);
    }
    validate(receipt)?;
    output[..HEADER_SIZE].fill(0);
    output[..4].copy_from_slice(b"R3R8");
    output[4..6].copy_from_slice(&VERSION.to_le_bytes());
    output[6..8].copy_from_slice(&PROFILE.to_le_bytes());
    output[8..12].copy_from_slice(&(length as u32).to_le_bytes());
    output[12..16].copy_from_slice(&(receipt.bytes.len() as u32).to_le_bytes());
    output[16..24].copy_from_slice(&receipt.id.epoch.to_le_bytes());
    output[24..32].copy_from_slice(&receipt.id.sequence.to_le_bytes());
    output[32..40].copy_from_slice(&receipt.source.surface.resource.epoch.to_le_bytes());
    output[40..44].copy_from_slice(&receipt.source.surface.resource.slot.to_le_bytes());
    output[44..48].copy_from_slice(&receipt.source.surface.resource.generation.to_le_bytes());
    output[48..56].copy_from_slice(&receipt.source.version.to_le_bytes());
    output[56..60].copy_from_slice(&receipt.source.surface.subresource.to_le_bytes());
    output[60..64].copy_from_slice(&(receipt.status as u32).to_le_bytes());
    output[HEADER_SIZE..length].copy_from_slice(receipt.bytes);
    Ok(length)
}

pub fn readback_receipt(bytes: &[u8]) -> Result<ReadbackReceipt<'_>, CodecError> {
    if bytes.len() < HEADER_SIZE {
        return Err(CodecError::Length);
    }
    if &bytes[..4] != b"R3R8" {
        return Err(CodecError::Magic);
    }
    if u16_at(bytes, 4) != VERSION || u16_at(bytes, 6) != PROFILE {
        return Err(CodecError::Version);
    }
    if u32_at(bytes, 8) as usize != bytes.len()
        || u32_at(bytes, 12) as usize != bytes.len() - HEADER_SIZE
    {
        return Err(CodecError::Length);
    }
    let receipt = ReadbackReceipt {
        id: ReadbackId {
            epoch: u64_at(bytes, 16),
            sequence: u64_at(bytes, 24),
        },
        source: SurfaceVersion {
            surface: SurfaceRef {
                resource: ResourceId {
                    epoch: u64_at(bytes, 32),
                    slot: u32_at(bytes, 40),
                    generation: u32_at(bytes, 44),
                },
                subresource: u32_at(bytes, 56),
            },
            version: u64_at(bytes, 48),
        },
        status: match u32_at(bytes, 60) {
            0 => ReadbackStatus::Complete,
            1 => ReadbackStatus::DeviceLost,
            2 => ReadbackStatus::DeviceRemoved,
            3 => ReadbackStatus::Allocation,
            _ => return Err(CodecError::InvalidCommand),
        },
        bytes: &bytes[HEADER_SIZE..],
    };
    validate(receipt)?;
    Ok(receipt)
}

fn validate(receipt: ReadbackReceipt<'_>) -> Result<(), CodecError> {
    if receipt.id.epoch == 0
        || receipt.id.sequence == 0
        || receipt.source.surface.resource.epoch == 0
        || receipt.source.surface.resource.generation == 0
        || receipt.source.version == 0
    {
        return Err(CodecError::Identity);
    }
    if receipt.status != ReadbackStatus::Complete && !receipt.bytes.is_empty() {
        return Err(CodecError::InvalidCommand);
    }
    Ok(())
}
