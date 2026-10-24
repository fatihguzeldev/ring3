use super::{CoreSession32, REGISTRY, Registry, admission, status};
use crate::cpu::{
    core::{StopReason, cold::ColdObserver},
    x86::SegmentContext,
};

mod execution;

pub(crate) fn core_precise(context: u32) -> u32 {
    REGISTRY.with(|registry| {
        let Ok(mut registry) = registry.try_borrow_mut() else {
            return StopReason::Infrastructure as u32;
        };
        let Registry { instance, core, .. } = &mut *registry;
        let (Some(instance), Some(session)) = (instance.as_mut(), core.as_mut()) else {
            return StopReason::Infrastructure as u32;
        };
        if !session.active || session.context_address() != context as usize {
            return StopReason::Infrastructure as u32;
        }
        precise_selected(instance, session) as u32
    })
}

pub(super) fn precise_selected(
    instance: &mut crate::process::EngineInstance,
    session: &mut CoreSession32,
) -> StopReason {
    let entry = session.cache.continuation_header().current_entry;
    // Exact execution contributes source-validated completed instructions to
    // an active capture. The preceding BLOCK alone confirms no suffix.
    let result = execution::precise(
        instance,
        &mut session.context,
        &mut session.numeric,
        &mut session.code,
        &session.cache,
        &mut session.cold_state,
        &mut session.tracing,
    );
    session.refresh_trace_bypass(session.context.retired());
    if result == StopReason::NeedCode {
        let Some(memory) = instance.memory.as_mut() else {
            return StopReason::Infrastructure;
        };
        let view = memory.core_view();
        for (offset, value) in [
            (56, view.mappings),
            (60, view.permissions),
            (64, view.backing),
            (68, view.backing_pages as usize),
            (72, view.executable_views),
            (76, view.private_slots),
            (80, view.transaction_version),
            (84, view.page_versions),
            (88, view.code_validation_version),
        ] {
            session.context.set_word(offset, value as u32);
        }
        session.context.bytes_mut()[96..104]
            .copy_from_slice(&view.expected_code_version.to_le_bytes());
    }
    if let (Some(memory), Some(code)) = (instance.memory.as_ref(), session.code.as_ref()) {
        if code.is_current(memory) {
            session.cache.bind_continuation(memory, code, entry);
        }
    }
    result
}

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
        if let Err(status) = session.begin_with_segments(instance, false, segments) {
            return -(status as i32);
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
        session.cache.end_continuation();
        result
    })
}

fn run(
    instance: &mut crate::process::EngineInstance,
    session: &mut CoreSession32,
    segments: &SegmentContext,
) -> i32 {
    let (bypass, bypass_retired) = session.tracing.bypass_token().unwrap_or((0, 0));
    let CoreSession32 {
        context,
        numeric,
        cache,
        cold_state,
        tracing,
        ..
    } = session;
    let reason = execution::run_observed(
        instance,
        context,
        numeric,
        segments,
        &|modules, pc| modules.gate_at(pc).map(|gate| gate.id),
        &|memory, pc, retired| {
            cache.frontier_contains_with_bypass(
                memory,
                pc,
                if retired == bypass_retired { bypass } else { 0 },
            )
        },
        cold_state,
        tracing,
    ) as i32;
    tracing.end();
    session.refresh_trace_bypass(session.context.retired());
    reason
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
        tracing,
        ..
    } = session;
    let next = execution::chain_next_observed(
        instance, context, numeric, code, cache, cold_state, tracing,
    );
    observe_selected(instance, session, next);
    next
}

pub(super) fn chain_probe(
    instance: &mut crate::process::EngineInstance,
    session: &mut CoreSession32,
    pc: u32,
) -> u32 {
    let next = execution::probe(
        instance,
        &mut session.context,
        &mut session.code,
        &mut session.cache,
        pc,
    );
    if next != 0 && (next as usize) <= session.cache.link_capacity() {
        if let (Some(memory), Some(code)) = (instance.memory.as_ref(), session.code.as_ref()) {
            session.cache.bind_continuation(memory, code, pc);
        }
    }
    next
}

pub(super) fn chain_cold(
    instance: &mut crate::process::EngineInstance,
    session: &mut CoreSession32,
) -> u32 {
    session.cache.clear_continuation();
    let next = execution::chain_cold_observed(
        instance,
        &mut session.context,
        &mut session.numeric,
        &mut session.code,
        &mut session.cache,
        &mut session.cold_state,
        &mut session.tracing,
    );
    observe_selected(instance, session, next);
    next
}

fn observe_selected(
    instance: &crate::process::EngineInstance,
    session: &mut CoreSession32,
    next: u32,
) {
    use crate::cpu::core::{COLD_MARK, STOP_MARK};
    let slot = next & !(COLD_MARK | STOP_MARK);
    session.refresh_trace_bypass(session.context.retired());
    if next & STOP_MARK == 0 && slot != 0 && (slot as usize) <= session.cache.link_capacity() {
        if let Some(memory) = instance.memory.as_ref() {
            if let Some(code) = session.code.as_ref() {
                session
                    .cache
                    .bind_continuation(memory, code, session.context.state().eip);
            }
            session.trace_selection(memory, session.context.state().eip);
        }
    } else if session.code.is_none() {
        session.cache.clear_continuation();
    }
}
