#![forbid(unsafe_code)]

use super::{AbiError, header};
use crate::{
    cpu::dbt::GateSpec,
    loader::{
        ImageMetadata32, LinkedImageMetadata32, LinkedImageMetadata32V2, LinkedImageMetadata32V3,
        LinkedImageMetadata32V4, LinkedImageMetadata32V5, LinkedImageMetadata32V6,
        LinkedImageMetadata32V7, LinkedImageMetadata32V8,
    },
};

pub const IMAGE_RECORD_SIZE: usize = 32;
pub const LINKED_IMAGE_V1_RECORD_SIZE: usize = 64;
pub const LINKED_IMAGE_V2_RECORD_SIZE: usize = 72;
pub const LINKED_IMAGE_V3_RECORD_SIZE: usize = 80;
pub const LINKED_IMAGE_V4_RECORD_SIZE: usize = 88;
pub const LINKED_IMAGE_V5_RECORD_SIZE: usize = 96;
pub const LINKED_IMAGE_V6_RECORD_SIZE: usize = 104;
pub const LINKED_IMAGE_V7_RECORD_SIZE: usize = 112;
pub const LINKED_IMAGE_V8_RECORD_SIZE: usize = 112;

pub fn encode_image(image: &ImageMetadata32, output: &mut [u8]) -> Result<(), AbiError> {
    if output.len() != IMAGE_RECORD_SIZE {
        return Err(AbiError::Length);
    }
    header::write(output, *b"R3PE");
    write_image_fields(image, output);
    Ok(())
}

pub fn encode_linked_image_v1(
    linked: &LinkedImageMetadata32,
    output: &mut [u8],
) -> Result<(), AbiError> {
    encode_linked_image_record(
        1,
        &linked.image,
        linked.gate_base,
        linked.gate_count,
        &linked.gates,
        output,
    )
}

pub fn encode_linked_image_v2(
    linked: &LinkedImageMetadata32V2,
    output: &mut [u8],
) -> Result<(), AbiError> {
    encode_linked_image_record(
        2,
        &linked.image,
        linked.gate_base,
        linked.gate_count,
        &linked.gates,
        output,
    )
}

pub fn encode_linked_image_v3(
    linked: &LinkedImageMetadata32V3,
    output: &mut [u8],
) -> Result<(), AbiError> {
    encode_linked_image_record(
        3,
        &linked.image,
        linked.gate_base,
        linked.gate_count,
        &linked.gates,
        output,
    )
}

pub fn encode_linked_image_v4(
    linked: &LinkedImageMetadata32V4,
    output: &mut [u8],
) -> Result<(), AbiError> {
    encode_linked_image_record(
        4,
        &linked.image,
        linked.gate_base,
        linked.gate_count,
        &linked.gates,
        output,
    )
}

pub fn encode_linked_image_v5(
    linked: &LinkedImageMetadata32V5,
    output: &mut [u8],
) -> Result<(), AbiError> {
    encode_linked_image_record(
        5,
        &linked.image,
        linked.gate_base,
        linked.gate_count,
        &linked.gates,
        output,
    )
}

pub fn encode_linked_image_v6(
    linked: &LinkedImageMetadata32V6,
    output: &mut [u8],
) -> Result<(), AbiError> {
    encode_linked_image_record(
        6,
        &linked.image,
        linked.gate_base,
        linked.gate_count,
        &linked.gates,
        output,
    )
}

pub fn encode_linked_image_v7(
    linked: &LinkedImageMetadata32V7,
    output: &mut [u8],
) -> Result<(), AbiError> {
    encode_linked_image_record(
        7,
        &linked.image,
        linked.gate_base,
        linked.gate_count,
        &linked.gates,
        output,
    )
}

pub fn encode_linked_image_v8(
    linked: &LinkedImageMetadata32V8,
    output: &mut [u8],
) -> Result<(), AbiError> {
    encode_linked_image_record(
        8,
        &linked.image,
        linked.gate_base,
        linked.gate_count,
        &linked.gates,
        output,
    )
}

fn encode_linked_image_record(
    version: u16,
    image: &ImageMetadata32,
    gate_base: u32,
    gate_count: u32,
    gates: &[GateSpec],
    output: &mut [u8],
) -> Result<(), AbiError> {
    if output.len() != 48 + 8 * gates.len() {
        return Err(AbiError::Length);
    }
    header::write_version(output, *b"R3LI", version);
    write_image_fields(image, output);
    header::write_u32(output, 32, gate_base);
    header::write_u32(output, 36, gate_count);
    for (index, gate) in gates.iter().enumerate() {
        header::write_u32(output, 48 + 8 * index, gate.entry.0);
        header::write_u32(output, 52 + 8 * index, gate.id);
    }
    Ok(())
}

fn write_image_fields(image: &ImageMetadata32, output: &mut [u8]) {
    for (index, value) in [
        image.image_base,
        image.image_size,
        image.entry_point,
        image.mapped_pages,
    ]
    .into_iter()
    .enumerate()
    {
        header::write_u32(output, 16 + 4 * index, value);
    }
}

#[cfg(test)]
mod tests;
