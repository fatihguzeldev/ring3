use super::mutate;
use crate::abi::arena::TRANSFER_OFFSET;
use crate::process::EngineInstance;

pub(crate) fn load_pe32(length: u32) -> u32 {
    mutate(|instance| {
        let image = instance.load_pe32_transfer(length)?;
        image_record(instance, image);
        Ok(())
    })
}

pub(crate) fn load_pe32_at(length: u32, actual_base: u32) -> u32 {
    mutate(|instance| {
        let image = instance.load_pe32_transfer_at(length, actual_base)?;
        image_record(instance, image);
        Ok(())
    })
}

pub(crate) fn load_pe32_linked_at(length: u32, actual_base: u32, gate_base: u32) -> u32 {
    mutate(|instance| {
        let linked = instance.load_pe32_linked_transfer_at(length, actual_base, gate_base)?;
        let fields = [
            u32::from_le_bytes(*b"R3LI"),
            0x10001,
            64,
            0,
            linked.image.image_base,
            linked.image.image_size,
            linked.image.entry_point,
            linked.image.mapped_pages,
            linked.gate_base,
            linked.gate_count,
            0,
            0,
            linked.gates[0].entry.0,
            linked.gates[0].id,
            linked.gates[1].entry.0,
            linked.gates[1].id,
        ];
        let output = &mut instance.arena.as_mut().get_mut()[TRANSFER_OFFSET..TRANSFER_OFFSET + 64];
        for (output, field) in output.chunks_exact_mut(4).zip(fields) {
            output.copy_from_slice(&field.to_le_bytes());
        }
        Ok(())
    })
}

pub(crate) fn load_pe32_linked_v2_at(length: u32, actual_base: u32, gate_base: u32) -> u32 {
    mutate(|instance| {
        let linked = instance.load_pe32_linked_v2_transfer_at(length, actual_base, gate_base)?;
        linked_image_v2_record(instance, linked);
        Ok(())
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
        image_record(instance, image);
        Ok(())
    })
}

pub(crate) fn load_pe32_linked_v2_input_at(actual_base: u32, gate_base: u32) -> u32 {
    mutate(|instance| {
        let linked = instance.load_pe32_linked_v2_input_at(actual_base, gate_base)?;
        linked_image_v2_record(instance, linked);
        Ok(())
    })
}

pub(crate) fn load_pe32_linked_v3_input_at(actual_base: u32, gate_base: u32) -> u32 {
    mutate(|instance| {
        let linked = instance.load_pe32_linked_v3_input_at(actual_base, gate_base)?;
        let fields = [
            u32::from_le_bytes(*b"R3LI"),
            0x10003,
            80,
            0,
            linked.image.image_base,
            linked.image.image_size,
            linked.image.entry_point,
            linked.image.mapped_pages,
            linked.gate_base,
            linked.gate_count,
            0,
            0,
            linked.gates[0].entry.0,
            linked.gates[0].id,
            linked.gates[1].entry.0,
            linked.gates[1].id,
            linked.gates[2].entry.0,
            linked.gates[2].id,
            linked.gates[3].entry.0,
            linked.gates[3].id,
        ];
        let output = &mut instance.arena.as_mut().get_mut()[TRANSFER_OFFSET..TRANSFER_OFFSET + 80];
        for (output, field) in output.chunks_exact_mut(4).zip(fields) {
            output.copy_from_slice(&field.to_le_bytes());
        }
        Ok(())
    })
}

pub(crate) fn load_pe32_linked_v4_input_at(actual_base: u32, gate_base: u32) -> u32 {
    mutate(|instance| {
        let linked = instance.load_pe32_linked_v4_input_at(actual_base, gate_base)?;
        let fields = [
            u32::from_le_bytes(*b"R3LI"),
            0x10004,
            88,
            0,
            linked.image.image_base,
            linked.image.image_size,
            linked.image.entry_point,
            linked.image.mapped_pages,
            linked.gate_base,
            linked.gate_count,
            0,
            0,
            linked.gates[0].entry.0,
            linked.gates[0].id,
            linked.gates[1].entry.0,
            linked.gates[1].id,
            linked.gates[2].entry.0,
            linked.gates[2].id,
            linked.gates[3].entry.0,
            linked.gates[3].id,
            linked.gates[4].entry.0,
            linked.gates[4].id,
        ];
        let output = &mut instance.arena.as_mut().get_mut()[TRANSFER_OFFSET..TRANSFER_OFFSET + 88];
        for (output, field) in output.chunks_exact_mut(4).zip(fields) {
            output.copy_from_slice(&field.to_le_bytes());
        }
        Ok(())
    })
}

fn linked_image_v2_record(
    instance: &mut EngineInstance,
    linked: crate::loader::LinkedImageMetadata32V2,
) {
    let fields = [
        u32::from_le_bytes(*b"R3LI"),
        0x10002,
        72,
        0,
        linked.image.image_base,
        linked.image.image_size,
        linked.image.entry_point,
        linked.image.mapped_pages,
        linked.gate_base,
        linked.gate_count,
        0,
        0,
        linked.gates[0].entry.0,
        linked.gates[0].id,
        linked.gates[1].entry.0,
        linked.gates[1].id,
        linked.gates[2].entry.0,
        linked.gates[2].id,
    ];
    let output = &mut instance.arena.as_mut().get_mut()[TRANSFER_OFFSET..TRANSFER_OFFSET + 72];
    for (output, field) in output.chunks_exact_mut(4).zip(fields) {
        output.copy_from_slice(&field.to_le_bytes());
    }
}

pub(crate) fn load_pe32_linked_v5_input_at(actual_base: u32, gate_base: u32) -> u32 {
    mutate(|instance| {
        let linked = instance.load_pe32_linked_v5_input_at(actual_base, gate_base)?;
        let fields = [
            u32::from_le_bytes(*b"R3LI"),
            0x10005,
            96,
            0,
            linked.image.image_base,
            linked.image.image_size,
            linked.image.entry_point,
            linked.image.mapped_pages,
            linked.gate_base,
            linked.gate_count,
            0,
            0,
            linked.gates[0].entry.0,
            linked.gates[0].id,
            linked.gates[1].entry.0,
            linked.gates[1].id,
            linked.gates[2].entry.0,
            linked.gates[2].id,
            linked.gates[3].entry.0,
            linked.gates[3].id,
            linked.gates[4].entry.0,
            linked.gates[4].id,
            linked.gates[5].entry.0,
            linked.gates[5].id,
        ];
        let output = &mut instance.arena.as_mut().get_mut()[TRANSFER_OFFSET..TRANSFER_OFFSET + 96];
        for (output, field) in output.chunks_exact_mut(4).zip(fields) {
            output.copy_from_slice(&field.to_le_bytes());
        }
        Ok(())
    })
}

pub(crate) fn load_pe32_linked_v6_input_at(actual_base: u32, gate_base: u32) -> u32 {
    mutate(|instance| {
        let linked = instance.load_pe32_linked_v6_input_at(actual_base, gate_base)?;
        let fields = [
            u32::from_le_bytes(*b"R3LI"),
            0x10006,
            104,
            0,
            linked.image.image_base,
            linked.image.image_size,
            linked.image.entry_point,
            linked.image.mapped_pages,
            linked.gate_base,
            linked.gate_count,
            0,
            0,
            linked.gates[0].entry.0,
            linked.gates[0].id,
            linked.gates[1].entry.0,
            linked.gates[1].id,
            linked.gates[2].entry.0,
            linked.gates[2].id,
            linked.gates[3].entry.0,
            linked.gates[3].id,
            linked.gates[4].entry.0,
            linked.gates[4].id,
            linked.gates[5].entry.0,
            linked.gates[5].id,
            linked.gates[6].entry.0,
            linked.gates[6].id,
        ];
        let output = &mut instance.arena.as_mut().get_mut()[TRANSFER_OFFSET..TRANSFER_OFFSET + 104];
        for (output, field) in output.chunks_exact_mut(4).zip(fields) {
            output.copy_from_slice(&field.to_le_bytes());
        }
        Ok(())
    })
}

pub(crate) fn load_pe32_linked_v7_input_at(actual_base: u32, gate_base: u32) -> u32 {
    mutate(|instance| {
        let linked = instance.load_pe32_linked_v7_input_at(actual_base, gate_base)?;
        let fields = [
            u32::from_le_bytes(*b"R3LI"),
            0x10007,
            112,
            0,
            linked.image.image_base,
            linked.image.image_size,
            linked.image.entry_point,
            linked.image.mapped_pages,
            linked.gate_base,
            linked.gate_count,
            0,
            0,
            linked.gates[0].entry.0,
            linked.gates[0].id,
            linked.gates[1].entry.0,
            linked.gates[1].id,
            linked.gates[2].entry.0,
            linked.gates[2].id,
            linked.gates[3].entry.0,
            linked.gates[3].id,
            linked.gates[4].entry.0,
            linked.gates[4].id,
            linked.gates[5].entry.0,
            linked.gates[5].id,
            linked.gates[6].entry.0,
            linked.gates[6].id,
            linked.gates[7].entry.0,
            linked.gates[7].id,
        ];
        let output = &mut instance.arena.as_mut().get_mut()[TRANSFER_OFFSET..TRANSFER_OFFSET + 112];
        for (output, field) in output.chunks_exact_mut(4).zip(fields) {
            output.copy_from_slice(&field.to_le_bytes());
        }
        Ok(())
    })
}

pub(crate) fn load_pe32_linked_v8_input_at(actual_base: u32, gate_base: u32) -> u32 {
    mutate(|instance| {
        let linked = instance.load_pe32_linked_v8_input_at(actual_base, gate_base)?;
        let fields = [
            u32::from_le_bytes(*b"R3LI"),
            0x10008,
            112,
            0,
            linked.image.image_base,
            linked.image.image_size,
            linked.image.entry_point,
            linked.image.mapped_pages,
            linked.gate_base,
            linked.gate_count,
            0,
            0,
            linked.gates[0].entry.0,
            linked.gates[0].id,
            linked.gates[1].entry.0,
            linked.gates[1].id,
            linked.gates[2].entry.0,
            linked.gates[2].id,
            linked.gates[3].entry.0,
            linked.gates[3].id,
            linked.gates[4].entry.0,
            linked.gates[4].id,
            linked.gates[5].entry.0,
            linked.gates[5].id,
            linked.gates[6].entry.0,
            linked.gates[6].id,
            linked.gates[7].entry.0,
            linked.gates[7].id,
        ];
        let output = &mut instance.arena.as_mut().get_mut()[TRANSFER_OFFSET..TRANSFER_OFFSET + 112];
        for (output, field) in output.chunks_exact_mut(4).zip(fields) {
            output.copy_from_slice(&field.to_le_bytes());
        }
        Ok(())
    })
}

pub(crate) fn start_loaded_image(stack_base: u32, pages: u32) -> u32 {
    mutate(|instance| instance.start_loaded_image(stack_base, pages))
}

fn image_record(instance: &mut EngineInstance, image: crate::loader::ImageMetadata32) {
    let fields = [
        u32::from_le_bytes(*b"R3PE"),
        0x10001,
        32,
        0,
        image.image_base,
        image.image_size,
        image.entry_point,
        image.mapped_pages,
    ];
    let output = &mut instance.arena.as_mut().get_mut()[TRANSFER_OFFSET..TRANSFER_OFFSET + 32];
    for (output, field) in output.chunks_exact_mut(4).zip(fields) {
        output.copy_from_slice(&field.to_le_bytes());
    }
}
