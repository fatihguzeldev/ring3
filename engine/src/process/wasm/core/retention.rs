use super::REGISTRY;

pub(crate) fn core_link_owners_ptr() -> u32 {
    REGISTRY.with(|registry| {
        registry
            .try_borrow()
            .ok()
            .and_then(|registry| {
                registry
                    .core
                    .as_ref()
                    .filter(|core| !core.active)
                    .map(|core| core.cache.link_owners_ptr() as usize as u32)
            })
            .unwrap_or(0)
    })
}

pub(crate) fn core_cache_victim() -> u32 {
    REGISTRY.with(|registry| {
        registry
            .try_borrow()
            .ok()
            .and_then(|registry| {
                registry
                    .core
                    .as_ref()
                    .filter(|core| !core.active)
                    .map(|core| core.cache.oldest_linked_slot())
            })
            .unwrap_or(0)
    })
}

pub(crate) fn core_cache_status_ptr() -> u32 {
    REGISTRY.with(|registry| {
        let Ok(mut registry) = registry.try_borrow_mut() else {
            return 0;
        };
        let Some(core) = registry.core.as_mut().filter(|core| !core.active) else {
            return 0;
        };
        // compiled, encoded bytes, count/byte evictions, source invalidations,
        // retained units, retained encoded bytes, and unit capacity: eight u64s.
        core.cache_status = core.cache.snapshot();
        core.cache_status.as_ptr() as usize as u32
    })
}

#[cfg(feature = "observability")]
pub(crate) fn core_cold_status_ptr() -> u32 {
    REGISTRY.with(|registry| {
        let Ok(mut registry) = registry.try_borrow_mut() else {
            return 0;
        };
        let Some(core) = registry.core.as_mut().filter(|core| !core.active) else {
            return 0;
        };
        core.cold_status = core.cold_state.diagnostics_snapshot();
        core.cold_status.as_ptr() as usize as u32
    })
}
