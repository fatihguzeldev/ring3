#![forbid(unsafe_code)]

use std::cell::RefCell;

use super::{CallError, EngineInstance, HostError, ResidentInstallation, StoreCompletion};
use crate::{
    abi::{
        arena::{CANCEL_OFFSET, EXIT_OFFSET, STATE_OFFSET, TRANSFER_OFFSET},
        x86::{STATE_SIZE, decode_state},
    },
    cpu::dbt::RegistryError,
};

struct Registry {
    opened: bool,
    instance: Option<EngineInstance>,
}

thread_local! {
    static REGISTRY: RefCell<Registry> = const {
        RefCell::new(Registry { opened: false, instance: None })
    };
}

pub(crate) fn open(pages: u32, low: u32, high: u32) -> u32 {
    REGISTRY.with(|registry| {
        let Ok(mut registry) = registry.try_borrow_mut() else {
            return 9;
        };
        if registry.opened {
            return 6;
        }
        let key = u64::from(low) | (u64::from(high) << 32);
        match EngineInstance::new(pages, key) {
            Ok(instance) => {
                registry.instance = Some(instance);
                registry.opened = true;
                0
            }
            Err(error) => status(error),
        }
    })
}

pub(crate) fn close() -> u32 {
    REGISTRY.with(|registry| {
        let Ok(mut registry) = registry.try_borrow_mut() else {
            return 9;
        };
        registry.opened = true;
        if let Some(instance) = registry.instance.as_mut() {
            instance.close();
        }
        0
    })
}

pub(crate) fn arena_ptr() -> u32 {
    inspect(|instance| {
        if instance.is_open() {
            instance.arena_address() as u32
        } else {
            0
        }
    })
}

pub(crate) fn map(address: u32, pages: u32, permissions: u32) -> u32 {
    mutate(|instance| instance.map(address, pages, permissions))
}

pub(crate) fn protect(address: u32, pages: u32, permissions: u32) -> u32 {
    mutate(|instance| instance.protect(address, pages, permissions))
}

pub(crate) fn unmap(address: u32, pages: u32) -> u32 {
    mutate(|instance| instance.unmap(address, pages))
}

pub(crate) fn upload(address: u32, length: u32) -> u32 {
    mutate(|instance| instance.upload(address, length))
}

pub(crate) fn compile(count: u32) -> u32 {
    mutate(|instance| instance.compile(count).map(|_| ()))
}

pub(crate) fn compile_with_gates(count: u32, gate_count: u32) -> u32 {
    mutate(|instance| instance.compile_with_gates(count, gate_count).map(|_| ()))
}

pub(crate) fn compile_entries(count: u32, gate_count: u32) -> u32 {
    mutate(|instance| instance.compile_entries(count, gate_count).map(|_| ()))
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
        if decode_state(&instance.arena()[STATE_OFFSET..STATE_OFFSET + STATE_SIZE]).is_err() {
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

pub(crate) fn capture_call(
    low: u32,
    high: u32,
    generation: u32,
    convention: u32,
    words: u32,
) -> u32 {
    let key = u64::from(low) | (u64::from(high) << 32);
    mutate(|instance| {
        instance
            .capture_call_raw(key, generation, convention, words)
            .map(|_| ())
    })
}

pub(crate) fn capture_resident_call(
    key_low: u32,
    key_high: u32,
    id_low: u32,
    id_high: u32,
    convention: u32,
    words: u32,
) -> u32 {
    let key = u64::from(key_low) | (u64::from(key_high) << 32);
    let id = u64::from(id_low) | (u64::from(id_high) << 32);
    mutate(|instance| {
        instance
            .capture_resident_call_raw(key, id, convention, words)
            .map(|_| ())
    })
}

pub(crate) fn complete_call(low: u32, high: u32, generation: u32, token: u32, result: u32) -> u32 {
    let key = u64::from(low) | (u64::from(high) << 32);
    mutate(|instance| instance.complete_call(key, generation, token, result))
}

pub(crate) fn complete_resident_call(
    key_low: u32,
    key_high: u32,
    id_low: u32,
    id_high: u32,
    token: u32,
    result: u32,
) -> u32 {
    let key = u64::from(key_low) | (u64::from(key_high) << 32);
    let id = u64::from(id_low) | (u64::from(id_high) << 32);
    mutate(|instance| instance.complete_resident_call(key, id, token, result))
}

pub(crate) fn abandon_call(low: u32, high: u32, token: u32) -> u32 {
    let key = u64::from(low) | (u64::from(high) << 32);
    mutate(|instance| instance.abandon_call(key, token))
}

pub(crate) fn capture_resident_callback_call(
    key_low: u32,
    key_high: u32,
    id_low: u32,
    id_high: u32,
    callback_token: u32,
    convention: u32,
    words: u32,
) -> u32 {
    let key = u64::from(key_low) | (u64::from(key_high) << 32);
    let id = u64::from(id_low) | (u64::from(id_high) << 32);
    mutate(|instance| {
        instance
            .capture_resident_callback_call_raw(key, id, callback_token, convention, words)
            .map(|_| ())
    })
}

pub(crate) fn complete_resident_callback_call(
    key_low: u32,
    key_high: u32,
    id_low: u32,
    id_high: u32,
    callback_token: u32,
    inner_token: u32,
    result: u32,
) -> u32 {
    let key = u64::from(key_low) | (u64::from(key_high) << 32);
    let id = u64::from(id_low) | (u64::from(id_high) << 32);
    mutate(|instance| {
        instance.complete_resident_callback_call(key, id, callback_token, inner_token, result)
    })
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn begin_callback(
    low: u32,
    high: u32,
    generation: u32,
    outer_token: u32,
    entry_pc: u32,
    return_pc: u32,
    return_id: u32,
    count: u32,
) -> u32 {
    let key = u64::from(low) | (u64::from(high) << 32);
    REGISTRY.with(|registry| {
        let Ok(mut registry) = registry.try_borrow_mut() else {
            return 9;
        };
        let Some(instance) = registry.instance.as_mut() else {
            return 5;
        };
        match instance.begin_callback_from_transfer(
            key,
            generation,
            outer_token,
            entry_pc,
            return_pc,
            return_id,
            count,
        ) {
            Ok(record) => {
                if record.outcome == 1 {
                    17
                } else {
                    0
                }
            }
            Err(error) => status(error),
        }
    })
}

pub(crate) fn finish_callback(low: u32, high: u32, generation: u32, token: u32) -> u32 {
    let key = u64::from(low) | (u64::from(high) << 32);
    mutate(|instance| instance.finish_callback(key, generation, token).map(|_| ()))
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn begin_resident_callback(
    key_low: u32,
    key_high: u32,
    outer_low: u32,
    outer_high: u32,
    callback_low: u32,
    callback_high: u32,
    outer_token: u32,
    entry_pc: u32,
    return_pc: u32,
    return_id: u32,
    count: u32,
) -> u32 {
    let key = u64::from(key_low) | (u64::from(key_high) << 32);
    let outer_id = u64::from(outer_low) | (u64::from(outer_high) << 32);
    let callback_id = u64::from(callback_low) | (u64::from(callback_high) << 32);
    REGISTRY.with(|registry| {
        let Ok(mut registry) = registry.try_borrow_mut() else {
            return 9;
        };
        let Some(instance) = registry.instance.as_mut() else {
            return 5;
        };
        match instance.begin_resident_callback_from_transfer(
            key,
            outer_id,
            callback_id,
            outer_token,
            entry_pc,
            return_pc,
            return_id,
            count,
        ) {
            Ok(record) => {
                if record.outcome == 0 {
                    0
                } else {
                    17
                }
            }
            Err(error) => status(error),
        }
    })
}

pub(crate) fn finish_resident_callback(
    key_low: u32,
    key_high: u32,
    callback_low: u32,
    callback_high: u32,
    token: u32,
) -> u32 {
    let key = u64::from(key_low) | (u64::from(key_high) << 32);
    let callback_id = u64::from(callback_low) | (u64::from(callback_high) << 32);
    mutate(|instance| {
        instance
            .finish_resident_callback(key, callback_id, token)
            .map(|_| ())
    })
}

pub(crate) fn authorize_resident_callback(
    key_low: u32,
    key_high: u32,
    callback_low: u32,
    callback_high: u32,
    token: u32,
) -> u32 {
    let key = u64::from(key_low) | (u64::from(key_high) << 32);
    let callback_id = u64::from(callback_low) | (u64::from(callback_high) << 32);
    mutate(|instance| instance.authorize_resident_callback(key, callback_id, token))
}

pub(crate) fn abort_callback(low: u32, high: u32, token: u32) -> u32 {
    let key = u64::from(low) | (u64::from(high) << 32);
    mutate(|instance| instance.abort_callback(key, token))
}

pub(crate) fn resume_callback_code(
    low: u32,
    high: u32,
    generation: u32,
    callback_token: u32,
    count: u32,
    gate_count: u32,
) -> u32 {
    let key = u64::from(low) | (u64::from(high) << 32);
    mutate(|instance| {
        instance
            .resume_callback_code(key, generation, callback_token, count, gate_count)
            .map(|_| ())
    })
}

pub(crate) fn resume_callback_entries(
    low: u32,
    high: u32,
    generation: u32,
    callback_token: u32,
    count: u32,
    gate_count: u32,
) -> u32 {
    let key = u64::from(low) | (u64::from(high) << 32);
    mutate(|instance| {
        instance
            .resume_callback_entries(key, generation, callback_token, count, gate_count)
            .map(|_| ())
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

pub(crate) fn read8(address: u32) -> u32 {
    mutate(|instance| instance.read8(address))
}

pub(crate) fn read16(address: u32) -> u32 {
    mutate(|instance| instance.read16(address))
}

pub(crate) fn read32(address: u32) -> u32 {
    mutate(|instance| instance.read32(address))
}

pub(crate) fn write32(address: u32, value: u32) -> u32 {
    mutate(|instance| instance.write32(address, value))
}

pub(crate) fn write_words32(count: u32) -> u32 {
    mutate(|instance| instance.write_words32(count))
}

pub(crate) fn store32(address: u32, value: u32) -> u32 {
    REGISTRY.with(|registry| {
        let Ok(mut registry) = registry.try_borrow_mut() else {
            return 9;
        };
        let Some(instance) = registry.instance.as_mut() else {
            return 5;
        };
        match instance.store32(address, value) {
            Ok(StoreCompletion::Complete) => 0,
            Ok(StoreCompletion::CodeInvalidated) => 11,
            Err(error) => status(error),
        }
    })
}

pub(crate) fn store_resident32(
    key_low: u32,
    key_high: u32,
    id_low: u32,
    id_high: u32,
    address: u32,
    value: u32,
) -> u32 {
    REGISTRY.with(|registry| {
        let Ok(mut registry) = registry.try_borrow_mut() else {
            return 9;
        };
        let Some(instance) = registry.instance.as_mut() else {
            return 5;
        };
        let key = u64::from(key_low) | (u64::from(key_high) << 32);
        let id = u64::from(id_low) | (u64::from(id_high) << 32);
        match instance.store_resident32(key, id, address, value) {
            Ok(StoreCompletion::Complete) => 0,
            Ok(StoreCompletion::CodeInvalidated) => 11,
            Err(error) => status(error),
        }
    })
}

fn inspect(operation: impl FnOnce(&EngineInstance) -> u32) -> u32 {
    REGISTRY.with(|registry| {
        let Ok(registry) = registry.try_borrow() else {
            return 0;
        };
        registry.instance.as_ref().map_or(0, operation)
    })
}

fn mutate(operation: impl FnOnce(&mut EngineInstance) -> Result<(), HostError>) -> u32 {
    REGISTRY.with(|registry| {
        let Ok(mut registry) = registry.try_borrow_mut() else {
            return 9;
        };
        let Some(instance) = registry.instance.as_mut() else {
            return 5;
        };
        operation(instance).map_or_else(status, |()| 0)
    })
}

fn status(error: HostError) -> u32 {
    match error {
        HostError::Closed => 5,
        HostError::InvalidRequest => 7,
        HostError::InvalidArtifact => 3,
        HostError::CodeInvalidated => 4,
        HostError::Memory(_) => 8,
        HostError::Compile(_) => 10,
        HostError::Resident(error) => match error {
            RegistryError::InvalidLimits | RegistryError::InstructionOverlap { .. } => 7,
            RegistryError::Allocation | RegistryError::IdentityExhausted => 9,
            RegistryError::WrongAddressSpace
            | RegistryError::InvalidUnit
            | RegistryError::NotFound { .. } => 3,
            RegistryError::UnitCapacity | RegistryError::ByteCapacity => 18,
            RegistryError::Compile(_) => 10,
            RegistryError::CodeInvalidated => 4,
        },
        HostError::GenerationExhausted | HostError::Infrastructure => 9,
        HostError::Call(error) => match error {
            CallError::Busy => 12,
            CallError::InvalidStop => 13,
            CallError::InvalidToken => 14,
            CallError::StateChanged => 15,
            CallError::Cancelled => 16,
            CallError::InvalidRequest => 7,
            CallError::TokenExhausted => 9,
            CallError::Memory(_) => 8,
        },
    }
}
