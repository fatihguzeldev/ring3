use super::super::{CoreSession32, REGISTRY, Registry, cold};
use crate::process::EngineInstance;

fn access(
    context: u32,
    operation: impl FnOnce(&mut EngineInstance, &mut CoreSession32) -> u32,
) -> u32 {
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
        operation(instance, core)
    })
}

pub(crate) fn core_chain_probe(context: u32, pc: u32) -> u32 {
    access(context, |instance, core| {
        cold::chain_probe(instance, core, pc)
    })
}

pub(crate) fn core_chain_select(context: u32, pc: u32, retired: u32) -> u32 {
    access(context, |instance, core| {
        core.refresh_trace_bypass(retired);
        let selected = cold::chain_probe(instance, core, pc);
        if selected != 0 && (selected as usize) <= core.cache.link_capacity() {
            if let Some(memory) = instance.memory.as_ref() {
                core.trace_selection(memory, pc);
            }
        }
        selected
    })
}

pub(crate) fn core_chain_cold(context: u32) -> u32 {
    access(context, |instance, core| {
        // Explicit legacy captures retain their old witness-only cold boundary.
        // Normal captures record completed cold instructions in the same arena.
        core.tracing.cold_boundary();
        cold::chain_cold(instance, core)
    })
}
