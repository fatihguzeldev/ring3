use super::{CoreSession32, REGISTRY, Registry, boundary, cold};
use crate::{
    cpu::core::{StopReason, continuation as route},
    process::EngineInstance,
};

fn identity(core: &CoreSession32) -> u64 {
    u64::from_le_bytes(core.context.bytes()[120..128].try_into().unwrap())
}

fn active(core: &CoreSession32, context: u32) -> bool {
    core.active
        && (&*core.context as *const crate::cpu::core::Context32 as usize as u32) == context
        && core.context.bytes()[112..116] == 1_u32.to_le_bytes()
}

pub(crate) fn core_continuation_format() -> u32 {
    route::FORMAT_VERSION
}

pub(crate) fn core_continuation_ptr() -> u32 {
    REGISTRY.with(|registry| {
        registry
            .try_borrow()
            .ok()
            .and_then(|registry| {
                registry.core.as_ref().map(|core| {
                    core.cache.continuation_header() as *const route::Header as usize as u32
                })
            })
            .unwrap_or(0)
    })
}

fn source(
    instance: &EngineInstance,
    core: &CoreSession32,
    context: u32,
    slot: u32,
    generation: u64,
    allow_stale: bool,
    require_entry: bool,
) -> Option<super::cache::RegionRef> {
    if !active(core, context) {
        return None;
    }
    core.cache.continuation_source(
        instance.memory.as_ref()?,
        context,
        slot,
        generation,
        core.code.as_ref(),
        allow_stale,
        require_entry,
    )
}

pub(crate) fn core_chain_resolve_owned(context: u32, pc: u32, retired: u32) -> u32 {
    resolve_owned(context, pc, retired, None)
}

pub(crate) fn core_chain_resolve_executable_owned(
    context: u32,
    pc: u32,
    retired: u32,
    remaining: u32,
) -> u32 {
    resolve_owned(context, pc, retired, Some(remaining))
}

fn resolve_owned(context: u32, pc: u32, retired: u32, remaining: Option<u32>) -> u32 {
    REGISTRY.with(|registry| {
        let Ok(mut registry) = registry.try_borrow_mut() else {
            return u32::MAX;
        };
        let Registry { instance, core, .. } = &mut *registry;
        let (Some(instance), Some(core)) = (instance.as_mut(), core.as_mut()) else {
            return u32::MAX;
        };
        let h = core.cache.continuation_header();
        let Some(code) = source(
            instance,
            core,
            context,
            h.current_slot,
            h.current_generation,
            false,
            true,
        ) else {
            return u32::MAX;
        };
        if identity(core) != code.identity() {
            return u32::MAX;
        }
        // Only this explicit ticket path can adopt a generated callee. The old
        // public probe/numeric interfaces retain their selected-owner contract.
        core.code = Some(code);
        core.refresh_trace_bypass(retired);
        let selected = match remaining {
            Some(remaining) => cold::chain_probe_executable(instance, core, pc, remaining),
            None => cold::chain_probe(instance, core, pc),
        };
        if selected != 0 && (selected as usize) <= core.cache.link_capacity() {
            if let Some(memory) = instance.memory.as_ref() {
                core.trace_selection(memory, pc);
            }
        }
        selected
    })
}

pub(crate) fn core_numeric_owned(
    context: u32,
    slot: u32,
    generation: u64,
    pc: u32,
    ordinal: u32,
) -> u32 {
    REGISTRY.with(|registry| {
        let Ok(mut registry) = registry.try_borrow_mut() else {
            return 8;
        };
        let Registry { instance, core, .. } = &mut *registry;
        let (Some(instance), Some(core)) = (instance.as_mut(), core.as_mut()) else {
            return 8;
        };
        let Some(code) = source(instance, core, context, slot, generation, false, true) else {
            return 8;
        };
        if identity(core) != code.identity()
            || code
                .native_instructions()
                .get(ordinal as usize)
                .is_none_or(|op| op.pc != pc)
        {
            return 8;
        }
        core.code = Some(code);
        boundary::numeric_selected(instance, core, ordinal)
    })
}

pub(crate) fn core_precise_owned(context: u32, slot: u32, generation: u64) -> u32 {
    REGISTRY.with(|registry| {
        let Ok(mut registry) = registry.try_borrow_mut() else {
            return StopReason::Infrastructure as u32;
        };
        let Registry { instance, core, .. } = &mut *registry;
        let (Some(instance), Some(core)) = (instance.as_mut(), core.as_mut()) else {
            return StopReason::Infrastructure as u32;
        };
        let Some(code) = source(instance, core, context, slot, generation, false, true) else {
            return StopReason::Infrastructure as u32;
        };
        if identity(core) != code.identity() || !code.contains_instruction(core.context.state().eip)
        {
            return StopReason::Infrastructure as u32;
        }
        core.code = Some(code);
        cold::precise_selected(instance, core) as u32
    })
}

pub(crate) fn core_end_owned(context: u32, slot: u32, generation: u64, reason: u32) -> u32 {
    if !(1..=10).contains(&reason) {
        return 7;
    }
    REGISTRY.with(|registry| {
        let Ok(mut registry) = registry.try_borrow_mut() else {
            return 9;
        };
        let Registry { instance, core, .. } = &mut *registry;
        let (Some(instance), Some(core)) = (instance.as_ref(), core.as_mut()) else {
            return 7;
        };
        if !active(core, context) {
            return 7;
        }
        let h = core.cache.continuation_header();
        if h.flags == route::COLD
            && h.current_id == 0
            && slot == 0
            && generation == 0
            && h.context == context
            && h.nonce != 0
            && h.current_nonce == h.nonce
            && instance
                .memory
                .as_ref()
                .is_some_and(|memory| memory.identity() == h.memory)
            && identity(core) == 0
            && core.code.is_none()
        {
            core.finish_end();
            return 0;
        }
        // A source-invalidating operation revokes execution but preserves the
        // exact final owner's stale metadata for the established begin=4 path.
        let stale = reason == StopReason::CodeInvalidated as u32;
        let Some(code) = source(instance, core, context, slot, generation, stale, false) else {
            return 7;
        };
        if identity(core) != code.identity() && !(stale && identity(core) == 0) {
            return 7;
        }
        core.code = Some(code);
        core.finish_end();
        0
    })
}
