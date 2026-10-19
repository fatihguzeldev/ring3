use super::paging::{cpu_mutate, cpu_paging_store};
use super::{REGISTRY, mutate, status};
use crate::process::{HostError, StoreCompletion};

pub(crate) fn cpu_read8(address: u32) -> u32 {
    cpu_mutate(|instance| instance.cpu_read8(address))
}

pub(crate) fn cpu_read16(address: u32) -> u32 {
    cpu_mutate(|instance| instance.cpu_read16(address))
}

pub(crate) fn cpu_read32(address: u32) -> u32 {
    cpu_mutate(|instance| instance.cpu_read32(address))
}

pub(crate) fn cpu_read32_view(epoch: i64) -> u32 {
    REGISTRY.with(|registry| {
        let Ok(registry) = registry.try_borrow() else {
            return 0;
        };
        let Some(instance) = registry.instance.as_ref() else {
            return 0;
        };
        let Ok(epoch) = u64::try_from(epoch) else {
            return 0;
        };
        instance.cpu_read32_view_address(epoch) as u32
    })
}

pub(crate) fn cpu_read(address: u32, length: u32, alignment: u32) -> u32 {
    cpu_mutate(|instance| instance.cpu_read(address, length, alignment))
}

pub(crate) fn cpu_write(address: u32, length: u32, alignment: u32) -> u32 {
    cpu_paging_store(|instance| instance.cpu_write(address, length, alignment))
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
    cpu_paging_store(|instance| {
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
    cpu_paging_store(|instance| instance.cpu_fxsave(address))
}

pub(crate) fn cpu_fxsave_resident(
    key_low: u32,
    key_high: u32,
    id_low: u32,
    id_high: u32,
    address: u32,
) -> u32 {
    cpu_paging_store(|instance| {
        instance.cpu_fxsave_resident(
            u64::from(key_low) | (u64::from(key_high) << 32),
            u64::from(id_low) | (u64::from(id_high) << 32),
            address,
        )
    })
}

pub(crate) fn cpu_mask_write(address: u32, length: u32, mask: u32) -> u32 {
    cpu_paging_store(|instance| instance.cpu_mask_write(address, length, mask))
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
    cpu_paging_store(|instance| {
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
    cpu_paging_store(|instance| instance.x87_op(operation, address, flags, pc, opcode, waiting))
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
    cpu_paging_store(|instance| {
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

#[allow(clippy::too_many_arguments)]
pub(crate) fn x87_resident_pair(
    key_low: u32,
    key_high: u32,
    id_low: u32,
    id_high: u32,
    first_pc: u32,
    address0: u32,
    address1: u32,
    flags: u32,
    max_steps: u32,
) -> i64 {
    use crate::cpu::dbt::X87PairProgress32;
    REGISTRY.with(|registry| {
        let Ok(mut registry) = registry.try_borrow_mut() else {
            return X87PairProgress32::encode(9, 0, 0) as i64;
        };
        let Some(instance) = registry.instance.as_mut() else {
            return X87PairProgress32::encode(5, 0, 0) as i64;
        };
        let before = instance.cpu_page_fault_sequence32();
        let outcome = instance.x87_resident_pair(
            u64::from(key_low) | (u64::from(key_high) << 32),
            u64::from(id_low) | (u64::from(id_high) << 32),
            first_pc,
            [address0, address1],
            flags,
            max_steps,
        );
        let transport = match outcome.result {
            Ok(StoreCompletion::Complete) if instance.cpu_page_fault_sequence32() != before => {
                crate::abi::cpu_helper::CPU_PAGE_FAULT_TRANSPORT
            }
            Ok(StoreCompletion::Complete) => 0,
            Ok(StoreCompletion::CodeInvalidated) => 11,
            Err(error) => status(error),
        };
        X87PairProgress32::encode(transport, outcome.completed, outcome.reported_step) as i64
    })
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn x87_resident_group(
    key_low: u32,
    key_high: u32,
    id_low: u32,
    id_high: u32,
    ordinal: u32,
    address0: u32,
    address1: u32,
    address2: u32,
    address3: u32,
    address4: u32,
    address5: u32,
    address6: u32,
    address7: u32,
    flags: u32,
    max_steps: u32,
) -> i64 {
    use crate::cpu::dbt::X87GroupProgress32;
    REGISTRY.with(|registry| {
        let Ok(mut registry) = registry.try_borrow_mut() else {
            return X87GroupProgress32::encode(9, 0, 0) as i64;
        };
        let Some(instance) = registry.instance.as_mut() else {
            return X87GroupProgress32::encode(5, 0, 0) as i64;
        };
        let before = instance.cpu_page_fault_sequence32();
        let outcome = instance.x87_resident_group(
            u64::from(key_low) | (u64::from(key_high) << 32),
            u64::from(id_low) | (u64::from(id_high) << 32),
            ordinal,
            [
                address0, address1, address2, address3, address4, address5, address6, address7,
            ],
            flags,
            max_steps,
        );
        let transport = match outcome.result {
            Ok(StoreCompletion::Complete) if instance.cpu_page_fault_sequence32() != before => {
                crate::abi::cpu_helper::CPU_PAGE_FAULT_TRANSPORT
            }
            Ok(StoreCompletion::Complete) => 0,
            Ok(StoreCompletion::CodeInvalidated) => 11,
            Err(error) => status(error),
        };
        X87GroupProgress32::encode(transport, outcome.completed, outcome.reported_step) as i64
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
    cpu_mutate(|instance| {
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

pub(crate) fn far_call(offset: u32, selector: u32, word: u32, return_eip: u32, esp: u32) -> u32 {
    cpu_paging_store(|instance| instance.far_call(offset, selector, word, return_eip, esp))
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn far_call_resident(
    key_low: u32,
    key_high: u32,
    id_low: u32,
    id_high: u32,
    offset: u32,
    selector: u32,
    word: u32,
    return_eip: u32,
    esp: u32,
) -> u32 {
    cpu_paging_store(|instance| {
        instance.far_call_resident(
            u64::from(key_low) | (u64::from(key_high) << 32),
            u64::from(id_low) | (u64::from(id_high) << 32),
            offset,
            selector,
            word,
            return_eip,
            esp,
        )
    })
}

pub(crate) fn far_return(offset: u32, selector: u32, word: u32) -> u32 {
    mutate(|instance| instance.far_return(offset, selector, word))
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn far_return_resident(
    key_low: u32,
    key_high: u32,
    id_low: u32,
    id_high: u32,
    offset: u32,
    selector: u32,
    word: u32,
) -> u32 {
    mutate(|instance| {
        instance.far_return_resident(
            u64::from(key_low) | (u64::from(key_high) << 32),
            u64::from(id_low) | (u64::from(id_high) << 32),
            offset,
            selector,
            word,
        )
    })
}

pub(crate) fn far_jump(offset: u32, selector: u32, word: u32) -> u32 {
    mutate(|instance| instance.far_jump(offset, selector, word))
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn far_jump_resident(
    key_low: u32,
    key_high: u32,
    id_low: u32,
    id_high: u32,
    offset: u32,
    selector: u32,
    word: u32,
) -> u32 {
    mutate(|instance| {
        instance.far_jump_resident(
            u64::from(key_low) | (u64::from(key_high) << 32),
            u64::from(id_low) | (u64::from(id_high) << 32),
            offset,
            selector,
            word,
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
    cpu_mutate(|instance| instance.segment_load_far(target, address, word))
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
    cpu_mutate(|instance| {
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

pub(crate) fn configure_segment_descriptor_raw(selector: u32, low: u32, high: u32) -> u32 {
    mutate(|instance| {
        if selector > u32::from(u16::MAX) {
            return Err(HostError::InvalidRequest);
        }
        instance.configure_segment_descriptor_raw(
            selector as u16,
            crate::cpu::x86::SegmentDescriptorRecord::from_words(low, high),
        )
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
    cpu_mutate(|instance| {
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
    cpu_paging_store(|instance| instance.cpu_integer(descriptor, address))
}

pub(crate) fn cpu_integer_resident(
    key_low: u32,
    key_high: u32,
    id_low: u32,
    id_high: u32,
    descriptor: u32,
    address: u32,
) -> u32 {
    cpu_paging_store(|instance| {
        instance.cpu_integer_resident(
            u64::from(key_low) | (u64::from(key_high) << 32),
            u64::from(id_low) | (u64::from(id_high) << 32),
            descriptor,
            address,
        )
    })
}

pub(crate) fn stack_execute(control: u32) -> u32 {
    cpu_paging_store(|instance| instance.stack_execute(control))
}

pub(crate) fn stack_execute_resident(
    key_low: u32,
    key_high: u32,
    id_low: u32,
    id_high: u32,
    control: u32,
) -> u32 {
    cpu_paging_store(|instance| {
        instance.stack_execute_resident(
            u64::from(key_low) | (u64::from(key_high) << 32),
            u64::from(id_low) | (u64::from(id_high) << 32),
            control,
        )
    })
}

pub(crate) fn configure_system_register_bank() -> u32 {
    mutate(|instance| instance.configure_system_register_bank_request())
}
