#![forbid(unsafe_code)]

use std::cell::RefCell;

use super::{CallError, EngineInstance, HostError, StoreCompletion};
use crate::{
    abi::arena::{CANCEL_OFFSET, EXIT_OFFSET, STATE_OFFSET, TRANSFER_OFFSET},
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
    mutate(|instance| {
        let id = instance.compile_resident(count)?;
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

pub(crate) fn complete_call(low: u32, high: u32, generation: u32, token: u32, result: u32) -> u32 {
    let key = u64::from(low) | (u64::from(high) << 32);
    mutate(|instance| instance.complete_call(key, generation, token, result))
}

pub(crate) fn abandon_call(low: u32, high: u32, token: u32) -> u32 {
    let key = u64::from(low) | (u64::from(high) << 32);
    mutate(|instance| instance.abandon_call(key, token))
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
