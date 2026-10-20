use super::{REGISTRY, status};
use crate::abi::arena::TRANSFER_OFFSET;

pub(crate) fn component_memory_lease(key_low: u32, key_high: u32) -> u32 {
    let key = u64::from(key_low) | (u64::from(key_high) << 32);
    REGISTRY.with(|registry| {
        let Ok(mut registry) = registry.try_borrow_mut() else {
            return 9;
        };
        if registry.core_active() {
            return 12;
        }
        let Some(instance) = registry.instance.as_ref() else {
            return 5;
        };
        if !instance.is_open() {
            return 5;
        }
        if instance.key() != key {
            return 3;
        }
        if instance.external_cpu_memory_pinned
            || instance.image_input.is_some()
            || instance.pending_call.is_some()
            || instance.callback.is_some()
            || instance.has_active_module_operation()
            || instance.execution_frame.is_some()
        {
            return 12;
        }
        let memory = match instance.memory() {
            Ok(memory) => memory,
            Err(error) => return status(error),
        };
        let (mappings, permissions, backing) = memory.cpu_read_arrays();
        if memory.mapped_pages() == 0
            || backing.is_empty()
            || backing.as_ptr() as usize % 4096 != 0
            || backing.len() % 4096 != 0
            || mappings.is_empty()
            || mappings.len() != permissions.len()
            || std::mem::size_of_val(&permissions[0]) != 1
        {
            return 7;
        }
        let Ok(backing_ptr) = u32::try_from(backing.as_ptr() as usize) else {
            return 7;
        };
        let Ok(backing_len) = u32::try_from(backing.len()) else {
            return 7;
        };
        let Ok(mappings_ptr) = u32::try_from(mappings.as_ptr() as usize) else {
            return 7;
        };
        let Ok(permissions_ptr) = u32::try_from(permissions.as_ptr() as usize) else {
            return 7;
        };
        let Ok(guest_pages) = u32::try_from(mappings.len()) else {
            return 7;
        };
        let fields = [
            backing_ptr,
            backing_len,
            mappings_ptr,
            permissions_ptr,
            guest_pages,
            memory.capacity_pages(),
            memory.mapped_pages(),
            1,
            1,
        ];
        let mut packet = [0u8; 64];
        packet[..4].copy_from_slice(b"R3CM");
        packet[4..6].copy_from_slice(&1u16.to_le_bytes());
        packet[6..8].copy_from_slice(&1u16.to_le_bytes());
        packet[8..12].copy_from_slice(&64u32.to_le_bytes());
        for (index, value) in fields.into_iter().enumerate() {
            packet[16 + index * 4..20 + index * 4].copy_from_slice(&value.to_le_bytes());
        }
        let instance = registry.instance.as_mut().unwrap();
        instance.external_cpu_memory_pinned = true;
        instance.arena.as_mut().get_mut()[TRANSFER_OFFSET..TRANSFER_OFFSET + 64]
            .copy_from_slice(&packet);
        0
    })
}
