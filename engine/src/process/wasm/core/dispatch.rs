use std::sync::OnceLock;

use super::{REGISTRY, Registry};

static DISPATCH_CODE: OnceLock<Vec<u8>> = OnceLock::new();

fn dispatch_code() -> Option<&'static [u8]> {
    let allowed = REGISTRY.with(|registry| {
        registry.try_borrow().is_ok_and(|registry| {
            if !registry.opened || registry.instance.is_none() || registry.core_active() {
                return false;
            }
            #[cfg(feature = "external-cpu")]
            if registry
                .instance
                .as_ref()
                .is_some_and(|instance| instance.external_cpu_memory_pinned)
            {
                return false;
            }
            true
        })
    });
    allowed.then(|| {
        DISPATCH_CODE
            .get_or_init(crate::cpu::core::emit_dispatch)
            .as_slice()
    })
}

pub(crate) fn core_dispatch_code_ptr() -> u32 {
    dispatch_code().map_or(0, |code| code.as_ptr() as usize as u32)
}

pub(crate) fn core_dispatch_code_len() -> u32 {
    dispatch_code().map_or(0, |code| code.len() as u32)
}

pub(crate) fn core_link(slot: u32) -> u32 {
    if !(1..=4096).contains(&slot) {
        return 7;
    }
    REGISTRY.with(|registry| {
        let Ok(mut registry) = registry.try_borrow_mut() else {
            return 9;
        };
        let Registry { instance, core, .. } = &mut *registry;
        let Some(core) = core.as_mut() else { return 7 };
        if core.active {
            return 12;
        }
        let Some(code) = core.code.as_ref() else {
            return 7;
        };
        let Some(memory) = instance
            .as_ref()
            .and_then(|instance| instance.memory.as_ref())
        else {
            return 5;
        };
        if !code.is_current(memory) {
            return 4;
        }
        match core.cache.install_link(code, slot) {
            Ok(()) => 0,
            Err(_) => 7,
        }
    })
}

pub(crate) fn core_unlink(slot: u32, low: u32, high: u32) -> u32 {
    if !(1..=4096).contains(&slot) {
        return 7;
    }
    REGISTRY.with(|registry| {
        let Ok(mut registry) = registry.try_borrow_mut() else {
            return 9;
        };
        let Some(core) = registry.core.as_mut() else {
            return 7;
        };
        if core.active {
            return 12;
        }
        match core
            .cache
            .unlink(slot, u64::from(low) | u64::from(high) << 32)
        {
            Ok(()) => 0,
            Err(_) => 4,
        }
    })
}

pub(crate) fn core_chain_next(context: u32) -> u32 {
    REGISTRY.with(|registry| {
        let Ok(mut registry) = registry.try_borrow_mut() else {
            return u32::MAX;
        };
        let Registry { instance, core, .. } = &mut *registry;
        let Some(core) = core.as_mut() else {
            return u32::MAX;
        };
        if !core.active || core.context_address() as u32 != context {
            return u32::MAX;
        }
        let Some(instance) = instance.as_mut() else {
            return u32::MAX;
        };
        super::cold::chain_next(instance, core)
    })
}
