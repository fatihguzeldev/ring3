use super::{CoreSession32, REGISTRY, Registry, admission, status};
use crate::cpu::x86::SegmentContext;

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
        let gates = match instance.module_gates() {
            Ok(gates) => gates,
            Err(error) => return -(status(error) as i32),
        };
        let mut gates: Vec<_> = gates.iter().map(|gate| (gate.entry, gate.id)).collect();
        gates.sort_unstable_by_key(|gate| gate.0.0);
        let mut frontier = core.as_ref().map_or_else(Vec::new, |session| {
            session.cache.frontier(instance.memory.as_ref().unwrap())
        });
        frontier.sort_unstable_by_key(|entry| entry.0);
        frontier.dedup();
        let mut initial = if core.is_none() {
            match CoreSession32::from_instance(instance) {
                Ok(session) => Some(session),
                Err(error) => return -(status(error) as i32),
            }
        } else {
            None
        };
        let session = core.as_mut().or(initial.as_mut()).unwrap();
        if let Err(status) = session.begin(instance, false) {
            return -(status as i32);
        }
        if let Some(initial) = initial {
            *core = Some(initial);
        }
        let session = core.as_mut().unwrap();
        session.context.set_word(44, budget);
        let result = run(instance, session, &segments, &gates, &frontier);
        session.context.set_word(112, 0);
        session.active = false;
        result
    })
}

fn run(
    instance: &mut crate::process::EngineInstance,
    session: &mut CoreSession32,
    segments: &SegmentContext,
    gates: &[(crate::memory::GuestAddress, u32)],
    frontier: &[crate::memory::GuestAddress],
) -> i32 {
    execution::run(
        instance,
        &mut session.context,
        &mut session.numeric,
        segments,
        gates,
        frontier,
        &mut session.cold_state,
    ) as i32
}
