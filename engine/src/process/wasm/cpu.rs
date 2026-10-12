use super::{cpu_store, mutate};
use crate::process::HostError;

pub(crate) fn cpu_read(address: u32, length: u32, alignment: u32) -> u32 {
    mutate(|instance| instance.cpu_read(address, length, alignment))
}

pub(crate) fn cpu_write(address: u32, length: u32, alignment: u32) -> u32 {
    cpu_store(|instance| instance.cpu_write(address, length, alignment))
}

pub(crate) fn cpu_write_resident(
    key_low: u32,
    key_high: u32,
    id_low: u32,
    id_high: u32,
    address: u32,
    length: u32,
    alignment: u32,
) -> u32 {
    cpu_store(|instance| {
        instance.cpu_write_resident(
            u64::from(key_low) | (u64::from(key_high) << 32),
            u64::from(id_low) | (u64::from(id_high) << 32),
            address,
            length,
            alignment,
        )
    })
}

pub(crate) fn cpu_fxsave(address: u32) -> u32 {
    cpu_store(|instance| instance.cpu_fxsave(address))
}

pub(crate) fn cpu_fxsave_resident(
    key_low: u32,
    key_high: u32,
    id_low: u32,
    id_high: u32,
    address: u32,
) -> u32 {
    cpu_store(|instance| {
        instance.cpu_fxsave_resident(
            u64::from(key_low) | (u64::from(key_high) << 32),
            u64::from(id_low) | (u64::from(id_high) << 32),
            address,
        )
    })
}

pub(crate) fn cpu_mask_write(address: u32, length: u32, mask: u32) -> u32 {
    cpu_store(|instance| instance.cpu_mask_write(address, length, mask))
}

pub(crate) fn cpu_mask_write_resident(
    key_low: u32,
    key_high: u32,
    id_low: u32,
    id_high: u32,
    address: u32,
    length: u32,
    mask: u32,
) -> u32 {
    cpu_store(|instance| {
        instance.cpu_mask_write_resident(
            u64::from(key_low) | (u64::from(key_high) << 32),
            u64::from(id_low) | (u64::from(id_high) << 32),
            address,
            length,
            mask,
        )
    })
}

pub(crate) fn x87_op(
    operation: u32,
    address: u32,
    flags: u32,
    pc: u32,
    opcode: u32,
    waiting: u32,
) -> u32 {
    cpu_store(|instance| instance.x87_op(operation, address, flags, pc, opcode, waiting))
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn x87_resident_op(
    key_low: u32,
    key_high: u32,
    id_low: u32,
    id_high: u32,
    operation: u32,
    address: u32,
    flags: u32,
    pc: u32,
    opcode: u32,
    waiting: u32,
) -> u32 {
    cpu_store(|instance| {
        instance.x87_resident_op(
            u64::from(key_low) | (u64::from(key_high) << 32),
            u64::from(id_low) | (u64::from(id_high) << 32),
            operation,
            address,
            flags,
            pc,
            opcode,
            waiting,
        )
    })
}

pub(crate) fn simd_execute(
    descriptor: u32,
    operands: u32,
    immediate: u32,
    source_gpr: u32,
    destination_gpr: u32,
    flags: u32,
) -> u32 {
    mutate(|instance| {
        instance.simd_execute(
            descriptor,
            operands,
            immediate,
            source_gpr,
            destination_gpr,
            flags,
        )
    })
}

pub(crate) fn segment_load(target: u32, selector: u32) -> u32 {
    mutate(|instance| instance.segment_load(target, selector))
}

pub(crate) fn segment_load_resident(
    key_low: u32,
    key_high: u32,
    id_low: u32,
    id_high: u32,
    target: u32,
    selector: u32,
) -> u32 {
    mutate(|instance| {
        instance.segment_load_resident(
            u64::from(key_low) | (u64::from(key_high) << 32),
            u64::from(id_low) | (u64::from(id_high) << 32),
            target,
            selector,
        )
    })
}

pub(crate) fn segment_load_far(target: u32, address: u32, word: u32) -> u32 {
    mutate(|instance| instance.segment_load_far(target, address, word))
}

pub(crate) fn segment_load_far_resident(
    key_low: u32,
    key_high: u32,
    id_low: u32,
    id_high: u32,
    target: u32,
    address: u32,
    word: u32,
) -> u32 {
    mutate(|instance| {
        instance.segment_load_far_resident(
            u64::from(key_low) | (u64::from(key_high) << 32),
            u64::from(id_low) | (u64::from(id_high) << 32),
            target,
            address,
            word,
        )
    })
}

pub(crate) fn set_segment_base(segment: u32, base: u32) -> u32 {
    mutate(|instance| {
        use crate::cpu::x86::SegmentRegister::*;
        let segment = match segment {
            0 => Es,
            1 => Cs,
            2 => Ss,
            3 => Ds,
            4 => Fs,
            5 => Gs,
            _ => return Err(HostError::InvalidRequest),
        };
        instance.set_segment_base(segment, base)
    })
}

pub(crate) fn configure_segment_descriptor(
    selector: u32,
    base: u32,
    dpl: u32,
    present: u32,
    kind: u32,
) -> u32 {
    mutate(|instance| {
        use crate::cpu::x86::{SegmentDescriptor, SegmentDescriptorKind};
        if selector > 0xffff || dpl > 3 || present > 1 {
            return Err(HostError::InvalidRequest);
        }
        let kind = match kind {
            0 => SegmentDescriptorKind::Data { writable: false },
            1 => SegmentDescriptorKind::Data { writable: true },
            2..=5 => SegmentDescriptorKind::Code {
                readable: kind & 1 != 0,
                conforming: kind >= 4,
            },
            6 => SegmentDescriptorKind::System,
            _ => return Err(HostError::InvalidRequest),
        };
        instance.configure_segment_descriptor(SegmentDescriptor {
            selector: selector as u16,
            base,
            dpl: dpl as u8,
            present: present != 0,
            kind,
        })
    })
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn simd_execute_resident(
    key_low: u32,
    key_high: u32,
    id_low: u32,
    id_high: u32,
    descriptor: u32,
    operands: u32,
    immediate: u32,
    source_gpr: u32,
    destination_gpr: u32,
    flags: u32,
) -> u32 {
    mutate(|instance| {
        instance.simd_execute_resident(
            u64::from(key_low) | (u64::from(key_high) << 32),
            u64::from(id_low) | (u64::from(id_high) << 32),
            descriptor,
            operands,
            immediate,
            source_gpr,
            destination_gpr,
            flags,
        )
    })
}

pub(crate) fn cpu_integer(descriptor: u32, address: u32) -> u32 {
    cpu_store(|instance| instance.cpu_integer(descriptor, address))
}

pub(crate) fn cpu_integer_resident(
    key_low: u32,
    key_high: u32,
    id_low: u32,
    id_high: u32,
    descriptor: u32,
    address: u32,
) -> u32 {
    cpu_store(|instance| {
        instance.cpu_integer_resident(
            u64::from(key_low) | (u64::from(key_high) << 32),
            u64::from(id_low) | (u64::from(id_high) << 32),
            descriptor,
            address,
        )
    })
}

pub(crate) fn stack_execute(control: u32) -> u32 {
    cpu_store(|instance| instance.stack_execute(control))
}

pub(crate) fn stack_execute_resident(
    key_low: u32,
    key_high: u32,
    id_low: u32,
    id_high: u32,
    control: u32,
) -> u32 {
    cpu_store(|instance| {
        instance.stack_execute_resident(
            u64::from(key_low) | (u64::from(key_high) << 32),
            u64::from(id_low) | (u64::from(id_high) << 32),
            control,
        )
    })
}
