#![forbid(unsafe_code)]

use std::cell::RefCell;

use super::{EngineInstance, HostError};
use crate::abi::arena::{CANCEL_OFFSET, EXIT_OFFSET, STATE_OFFSET};

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

pub(crate) fn read32(address: u32) -> u32 {
    mutate(|instance| instance.read32(address))
}

pub(crate) fn write32(address: u32, value: u32) -> u32 {
    mutate(|instance| instance.write32(address, value))
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
        HostError::GenerationExhausted | HostError::Infrastructure => 9,
    }
}
