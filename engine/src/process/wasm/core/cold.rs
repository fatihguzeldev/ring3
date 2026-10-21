use super::{CoreSession32, REGISTRY, Registry, admission, status};
use crate::cpu::{core::StopReason, x86::SegmentContext};

mod execution;

pub(crate) fn core_cold_run(budget: u32) -> i32 {
    REGISTRY.with(|registry| {
        let Ok(mut registry) = registry.try_borrow_mut() else {
            return -9;
        };
        let Registry { instance, core, .. } = &mut *registry;
        if core.as_ref().is_some_and(CoreSession32::is_active) {
            return -12;
        }
        let Some(instance) = instance.as_mut() else {
            return -5;
        };
        let segments = match instance
            .enable_core_execution()
            .and_then(|_| admission(instance))
        {
            Ok(segments) => segments,
            Err(error) => return -(status(error) as i32),
        };
        let mut initial = if core.is_none() {
            match CoreSession32::from_instance(instance) {
                Ok(session) => Some(session),
                Err(error) => return -(status(error) as i32),
            }
        } else {
            None
        };
        let session = core.as_mut().or(initial.as_mut()).unwrap();
        let pc = session.context.state().eip;
        let gate = (budget != 0).then(|| instance.module_gate_at(pc)).flatten();
        let cached = budget != 0
            && gate.is_none()
            && session
                .cache
                .frontier_contains(instance.memory.as_ref().unwrap(), pc);
        if let Err(status) = session.begin(instance, false) {
            return -(status as i32);
        }
        if !cached {
            session.cache.clear_transition();
        }
        if let Some(initial) = initial {
            *core = Some(initial);
        }
        let session = core.as_mut().unwrap();
        session.context.set_word(44, budget);
        let result = if cached {
            StopReason::NeedCode as i32
        } else {
            run(instance, session, &segments)
        };
        session.context.set_word(112, 0);
        session.active = false;
        result
    })
}

fn run(
    instance: &mut crate::process::EngineInstance,
    session: &mut CoreSession32,
    segments: &SegmentContext,
) -> i32 {
    let CoreSession32 {
        context,
        numeric,
        cache,
        cold_state,
        ..
    } = session;
    execution::run_with_lookups(
        instance,
        context,
        numeric,
        segments,
        &|modules, pc| modules.gate_at(pc).map(|gate| gate.id),
        &|memory, pc| cache.frontier_contains(memory, pc),
        cold_state,
    ) as i32
}

pub(super) fn chain_next(
    instance: &mut crate::process::EngineInstance,
    session: &mut CoreSession32,
) -> u32 {
    let CoreSession32 {
        context,
        numeric,
        code,
        cache,
        cold_state,
        ..
    } = session;
    execution::chain_next(instance, context, numeric, code, cache, cold_state)
}
