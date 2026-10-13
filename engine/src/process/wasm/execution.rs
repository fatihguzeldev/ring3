use super::{REGISTRY, inspect, mutate, status};
use crate::abi::arena::{CANCEL_OFFSET, EXIT_OFFSET, STATE_OFFSET, TRANSFER_OFFSET};
use crate::abi::x86::{STATE_SIZE, decode_state};
use crate::cpu::dbt::RegistryError;
use crate::process::{EngineInstance, HostError, ResidentInstallation};

pub(crate) fn compile(count: u32) -> u32 {
    mutate(|instance| instance.compile(count).map(|_| ()))
}

pub(crate) fn compile_with_gates(count: u32, gate_count: u32) -> u32 {
    mutate(|instance| instance.compile_with_gates(count, gate_count).map(|_| ()))
}

pub(crate) fn compile_entries(count: u32, gate_count: u32) -> u32 {
    mutate(|instance| instance.compile_entries(count, gate_count).map(|_| ()))
}

pub(crate) fn compile_stopped_entry(key_low: u32, key_high: u32, generation: u32) -> u32 {
    let key = u64::from(key_low) | (u64::from(key_high) << 32);
    mutate(|instance| instance.compile_stopped_entry(key, generation).map(|_| ()))
}

pub(crate) fn compile_stopped_resident_entry(key_low: u32, key_high: u32) -> u32 {
    let key = u64::from(key_low) | (u64::from(key_high) << 32);
    mutate(|instance| {
        let id = instance.compile_stopped_resident_entry(key)?;
        resident_record(instance, id.get())
    })
}

pub(crate) fn compile_resident(count: u32) -> u32 {
    compile_resident_with_gates(count, 0)
}

pub(crate) fn compile_resident_with_gates(count: u32, gate_count: u32) -> u32 {
    mutate(|instance| {
        let id = instance.compile_resident_with_gates(count, gate_count)?;
        resident_record(instance, id.get())
    })
}

pub(crate) fn compile_resident_entries(count: u32, gate_count: u32) -> u32 {
    mutate(|instance| {
        let id = instance.compile_resident_entries(count, gate_count)?;
        resident_record(instance, id.get())
    })
}

pub(crate) fn compile_resident_callback_unit(
    key_low: u32,
    key_high: u32,
    home_low: u32,
    home_high: u32,
    callback_token: u32,
    count: u32,
    gate_count: u32,
) -> u32 {
    let key = u64::from(key_low) | (u64::from(key_high) << 32);
    let home_id = u64::from(home_low) | (u64::from(home_high) << 32);
    mutate(|instance| {
        let id = instance.compile_resident_callback_unit(
            key,
            home_id,
            callback_token,
            count,
            gate_count,
        )?;
        resident_record(instance, id.get())
    })
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn acknowledge_resident_callback_installation(
    key_low: u32,
    key_high: u32,
    home_low: u32,
    home_high: u32,
    callback_token: u32,
    unit_low: u32,
    unit_high: u32,
    slot: u32,
) -> u32 {
    let key = u64::from(key_low) | (u64::from(key_high) << 32);
    let home_id = u64::from(home_low) | (u64::from(home_high) << 32);
    let unit_id = u64::from(unit_low) | (u64::from(unit_high) << 32);
    mutate(|instance| {
        let installed = instance.acknowledge_resident_callback_installation(
            key,
            home_id,
            callback_token,
            unit_id,
            slot,
        )?;
        installation_record(instance, installed);
        Ok(())
    })
}

pub(crate) fn find_resident(pc: u32) -> u32 {
    REGISTRY.with(|registry| {
        let Ok(mut registry) = registry.try_borrow_mut() else {
            return 9;
        };
        let Some(instance) = registry.instance.as_mut() else {
            return 5;
        };
        let id = match instance.lookup_resident(pc) {
            Ok(id) => id,
            Err(HostError::Resident(RegistryError::NotFound { .. })) => return 17,
            Err(error) => return status(error),
        };
        resident_record(instance, id.get()).map_or_else(status, |()| 0)
    })
}

pub(crate) fn discard_unacknowledged_resident(
    key_low: u32,
    key_high: u32,
    id_low: u32,
    id_high: u32,
) -> u32 {
    let key = u64::from(key_low) | (u64::from(key_high) << 32);
    let id = u64::from(id_low) | (u64::from(id_high) << 32);
    mutate(|instance| instance.discard_unacknowledged_resident(key, id))
}

pub(crate) fn discard_installed_resident(
    key_low: u32,
    key_high: u32,
    id_low: u32,
    id_high: u32,
    expected_slot: u32,
) -> u32 {
    let key = u64::from(key_low) | (u64::from(key_high) << 32);
    let id = u64::from(id_low) | (u64::from(id_high) << 32);
    mutate(|instance| instance.discard_installed_resident(key, id, expected_slot))
}

pub(crate) fn retire_stale_resident(key_low: u32, key_high: u32, id_low: u32, id_high: u32) -> u32 {
    let key = u64::from(key_low) | (u64::from(key_high) << 32);
    let id = u64::from(id_low) | (u64::from(id_high) << 32);
    mutate(|instance| instance.retire_stale_resident(key, id))
}

pub(crate) fn acknowledge_resident_installation(
    key_low: u32,
    key_high: u32,
    id_low: u32,
    id_high: u32,
    slot: u32,
) -> u32 {
    let key = u64::from(key_low) | (u64::from(key_high) << 32);
    let id = u64::from(id_low) | (u64::from(id_high) << 32);
    mutate(|instance| {
        let installed = instance.acknowledge_resident_installation(key, id, slot)?;
        installation_record(instance, installed);
        Ok(())
    })
}

pub(crate) fn find_installed_resident(key_low: u32, key_high: u32, pc: u32) -> u32 {
    REGISTRY.with(|registry| {
        let Ok(mut registry) = registry.try_borrow_mut() else {
            return 9;
        };
        let Some(instance) = registry.instance.as_mut() else {
            return 5;
        };
        let key = u64::from(key_low) | (u64::from(key_high) << 32);
        let installed = match instance.lookup_installed_resident(key, pc) {
            Ok(installed) => installed,
            Err(HostError::Resident(RegistryError::NotFound { .. })) => return 17,
            Err(error) => return status(error),
        };
        installation_record(instance, installed);
        0
    })
}

fn installation_record(instance: &mut EngineInstance, installed: ResidentInstallation) {
    let fields = [
        u32::from_le_bytes(*b"R3IN"),
        0x10001,
        32,
        0,
        installed.unit_id as u32,
        (installed.unit_id >> 32) as u32,
        installed.slot,
        0,
    ];
    let output = &mut instance.arena.as_mut().get_mut()[TRANSFER_OFFSET..TRANSFER_OFFSET + 32];
    for (word, value) in output.chunks_exact_mut(4).zip(fields) {
        word.copy_from_slice(&value.to_le_bytes());
    }
}

pub(crate) fn dispatcher_module(low: u32, high: u32) -> u32 {
    let key = u64::from(low) | (u64::from(high) << 32);
    mutate(|instance| {
        let bytes = instance.dispatcher_bytes(key)?;
        let fields = [
            u32::from_le_bytes(*b"R3DP"),
            0x10001,
            32,
            0,
            low,
            high,
            bytes.as_ptr() as u32,
            bytes.len() as u32,
        ];
        let output = &mut instance.arena.as_mut().get_mut()[TRANSFER_OFFSET..TRANSFER_OFFSET + 32];
        for (word, value) in output.chunks_exact_mut(4).zip(fields) {
            word.copy_from_slice(&value.to_le_bytes());
        }
        Ok(())
    })
}

pub(crate) fn resident_module(low: u32, high: u32) -> u32 {
    let id = u64::from(low) | (u64::from(high) << 32);
    mutate(|instance| resident_record(instance, id))
}

fn resident_record(instance: &mut EngineInstance, id: u64) -> Result<(), HostError> {
    let bytes = instance.resident_bytes(id)?;
    let fields = [
        1,
        24,
        id as u32,
        (id >> 32) as u32,
        bytes.as_ptr() as u32,
        bytes.len() as u32,
    ];
    let transfer = &mut instance.arena.as_mut().get_mut()[TRANSFER_OFFSET..TRANSFER_OFFSET + 24];
    for (output, value) in transfer.chunks_exact_mut(4).zip(fields) {
        output.copy_from_slice(&value.to_le_bytes());
    }
    Ok(())
}

pub(crate) fn guard_dispatch_entry(
    key_low: u32,
    key_high: u32,
    state: u32,
    exit: u32,
    cancel: u32,
) -> u32 {
    REGISTRY.with(|registry| {
        let Ok(registry) = registry.try_borrow() else {
            return 9;
        };
        let Some(instance) = registry.instance.as_ref() else {
            return 5;
        };
        let key = u64::from(key_low) | (u64::from(key_high) << 32);
        if let Err(error) = instance.guard_dispatch_entry(key) {
            return status(error);
        }
        let base = instance.arena_address() as u32;
        if state != base + STATE_OFFSET as u32
            || exit != base + EXIT_OFFSET as u32
            || cancel != base + CANCEL_OFFSET as u32
        {
            return 1;
        }
        if decode_state(&instance.arena()[STATE_OFFSET..STATE_OFFSET + STATE_SIZE]).is_err()
            || instance.segment_context().is_err()
        {
            return 2;
        }
        0
    })
}

pub(crate) fn guard_resident(
    key_low: u32,
    key_high: u32,
    id_low: u32,
    id_high: u32,
    state: u32,
    exit: u32,
    cancel: u32,
) -> u32 {
    REGISTRY.with(|registry| {
        let Ok(registry) = registry.try_borrow() else {
            return 9;
        };
        let Some(instance) = registry.instance.as_ref() else {
            return 5;
        };
        let key = u64::from(key_low) | (u64::from(key_high) << 32);
        let id = u64::from(id_low) | (u64::from(id_high) << 32);
        if let Err(error) = instance.guard_resident(key, id) {
            return status(error);
        }
        let base = instance.arena_address() as u32;
        if state != base + STATE_OFFSET as u32
            || exit != base + EXIT_OFFSET as u32
            || cancel != base + CANCEL_OFFSET as u32
        {
            return 1;
        }
        0
    })
}

pub(crate) fn generation() -> u32 {
    inspect(EngineInstance::generation)
}

pub(crate) fn module_ptr() -> u32 {
    inspect(|instance| {
        instance
            .artifact_bytes()
            .map_or(0, |bytes| bytes.as_ptr() as usize as u32)
    })
}

pub(crate) fn module_len() -> u32 {
    inspect(|instance| {
        instance
            .artifact_bytes()
            .map_or(0, |bytes| bytes.len() as u32)
    })
}

pub(crate) fn guard(
    low: u32,
    high: u32,
    generation: u32,
    state: u32,
    exit: u32,
    cancel: u32,
) -> u32 {
    REGISTRY.with(|registry| {
        let Ok(registry) = registry.try_borrow() else {
            return 9;
        };
        let Some(instance) = registry.instance.as_ref() else {
            return 5;
        };
        let key = u64::from(low) | (u64::from(high) << 32);
        if let Err(error) = instance.guard(key, generation) {
            return status(error);
        }
        let base = instance.arena_address() as u32;
        if state != base + STATE_OFFSET as u32
            || exit != base + EXIT_OFFSET as u32
            || cancel != base + CANCEL_OFFSET as u32
        {
            return 1;
        }
        0
    })
}
