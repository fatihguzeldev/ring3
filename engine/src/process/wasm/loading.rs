use super::mutate;
use crate::abi::{arena::TRANSFER_OFFSET, image};
use crate::process::{EngineInstance, HostError};

pub(crate) fn load_pe32(length: u32) -> u32 {
    mutate(|instance| {
        let image = instance.load_pe32_transfer(length)?;
        image_record(instance, image)
    })
}

pub(crate) fn load_pe32_at(length: u32, actual_base: u32) -> u32 {
    mutate(|instance| {
        let image = instance.load_pe32_transfer_at(length, actual_base)?;
        image_record(instance, image)
    })
}

pub(crate) fn load_pe32_linked_at(length: u32, actual_base: u32, gate_base: u32) -> u32 {
    mutate(|instance| {
        let linked = instance.load_pe32_linked_transfer_at(length, actual_base, gate_base)?;
        image::encode_linked_image_v1(
            &linked,
            &mut instance.arena.as_mut().get_mut()
                [TRANSFER_OFFSET..TRANSFER_OFFSET + image::LINKED_IMAGE_V1_RECORD_SIZE],
        )
        .map_err(|_| HostError::Infrastructure)
    })
}

pub(crate) fn load_pe32_linked_v2_at(length: u32, actual_base: u32, gate_base: u32) -> u32 {
    mutate(|instance| {
        let linked = instance.load_pe32_linked_v2_transfer_at(length, actual_base, gate_base)?;
        linked_image_v2_record(instance, linked)
    })
}

pub(crate) fn begin_image_input(total: u32) -> u32 {
    mutate(|instance| instance.begin_image_input(total))
}

pub(crate) fn append_image_input(offset: u32, length: u32) -> u32 {
    mutate(|instance| instance.append_image_input_transfer(offset, length))
}

pub(crate) fn abort_image_input() -> u32 {
    mutate(EngineInstance::abort_image_input)
}

pub(crate) fn load_pe32_input_at(actual_base: u32) -> u32 {
    mutate(|instance| {
        let image = instance.load_pe32_input_at(actual_base)?;
        image_record(instance, image)
    })
}

pub(crate) fn load_pe32_linked_v2_input_at(actual_base: u32, gate_base: u32) -> u32 {
    mutate(|instance| {
        let linked = instance.load_pe32_linked_v2_input_at(actual_base, gate_base)?;
        linked_image_v2_record(instance, linked)
    })
}

pub(crate) fn load_pe32_linked_v3_input_at(actual_base: u32, gate_base: u32) -> u32 {
    mutate(|instance| {
        let linked = instance.load_pe32_linked_v3_input_at(actual_base, gate_base)?;
        image::encode_linked_image_v3(
            &linked,
            &mut instance.arena.as_mut().get_mut()
                [TRANSFER_OFFSET..TRANSFER_OFFSET + image::LINKED_IMAGE_V3_RECORD_SIZE],
        )
        .map_err(|_| HostError::Infrastructure)
    })
}

pub(crate) fn load_pe32_linked_v4_input_at(actual_base: u32, gate_base: u32) -> u32 {
    mutate(|instance| {
        let linked = instance.load_pe32_linked_v4_input_at(actual_base, gate_base)?;
        image::encode_linked_image_v4(
            &linked,
            &mut instance.arena.as_mut().get_mut()
                [TRANSFER_OFFSET..TRANSFER_OFFSET + image::LINKED_IMAGE_V4_RECORD_SIZE],
        )
        .map_err(|_| HostError::Infrastructure)
    })
}

fn linked_image_v2_record(
    instance: &mut EngineInstance,
    linked: crate::loader::LinkedImageMetadata32V2,
) -> Result<(), HostError> {
    image::encode_linked_image_v2(
        &linked,
        &mut instance.arena.as_mut().get_mut()
            [TRANSFER_OFFSET..TRANSFER_OFFSET + image::LINKED_IMAGE_V2_RECORD_SIZE],
    )
    .map_err(|_| HostError::Infrastructure)
}

pub(crate) fn load_pe32_linked_v5_input_at(actual_base: u32, gate_base: u32) -> u32 {
    mutate(|instance| {
        let linked = instance.load_pe32_linked_v5_input_at(actual_base, gate_base)?;
        image::encode_linked_image_v5(
            &linked,
            &mut instance.arena.as_mut().get_mut()
                [TRANSFER_OFFSET..TRANSFER_OFFSET + image::LINKED_IMAGE_V5_RECORD_SIZE],
        )
        .map_err(|_| HostError::Infrastructure)
    })
}

pub(crate) fn load_pe32_linked_v6_input_at(actual_base: u32, gate_base: u32) -> u32 {
    mutate(|instance| {
        let linked = instance.load_pe32_linked_v6_input_at(actual_base, gate_base)?;
        image::encode_linked_image_v6(
            &linked,
            &mut instance.arena.as_mut().get_mut()
                [TRANSFER_OFFSET..TRANSFER_OFFSET + image::LINKED_IMAGE_V6_RECORD_SIZE],
        )
        .map_err(|_| HostError::Infrastructure)
    })
}

pub(crate) fn load_pe32_linked_v7_input_at(actual_base: u32, gate_base: u32) -> u32 {
    mutate(|instance| {
        let linked = instance.load_pe32_linked_v7_input_at(actual_base, gate_base)?;
        image::encode_linked_image_v7(
            &linked,
            &mut instance.arena.as_mut().get_mut()
                [TRANSFER_OFFSET..TRANSFER_OFFSET + image::LINKED_IMAGE_V7_RECORD_SIZE],
        )
        .map_err(|_| HostError::Infrastructure)
    })
}

pub(crate) fn load_pe32_linked_v8_input_at(actual_base: u32, gate_base: u32) -> u32 {
    mutate(|instance| {
        let linked = instance.load_pe32_linked_v8_input_at(actual_base, gate_base)?;
        image::encode_linked_image_v8(
            &linked,
            &mut instance.arena.as_mut().get_mut()
                [TRANSFER_OFFSET..TRANSFER_OFFSET + image::LINKED_IMAGE_V8_RECORD_SIZE],
        )
        .map_err(|_| HostError::Infrastructure)
    })
}

pub(crate) fn start_loaded_image(stack_base: u32, pages: u32) -> u32 {
    mutate(|instance| instance.start_loaded_image(stack_base, pages))
}

fn image_record(
    instance: &mut EngineInstance,
    image: crate::loader::ImageMetadata32,
) -> Result<(), HostError> {
    crate::abi::image::encode_image(
        &image,
        &mut instance.arena.as_mut().get_mut()
            [TRANSFER_OFFSET..TRANSFER_OFFSET + image::IMAGE_RECORD_SIZE],
    )
    .map_err(|_| HostError::Infrastructure)
}
