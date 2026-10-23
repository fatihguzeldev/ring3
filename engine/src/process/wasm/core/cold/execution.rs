use crate::{
    cpu::{
        core::{
            COLD_MARK, Context32, STOP_MARK, StopReason,
            cold::{self, ColdOutcome, ColdState},
        },
        x86::SegmentContext,
    },
    memory::AddressSpace,
    process::{EngineInstance, loading::modules::ModuleProcess32},
};

#[cfg(test)]
use crate::memory::GuestAddress;

#[cfg(target_arch = "wasm32")]
use super::super::numeric::NumericBank;
#[cfg(not(target_arch = "wasm32"))]
use crate::process::core_numeric_tests::NumericBank;

#[cfg(target_arch = "wasm32")]
use super::super::cache::{CoreCache32, RegionRef, TransitionLookup};
#[cfg(not(target_arch = "wasm32"))]
use crate::process::core_cache_tests::{CoreCache32, RegionRef, TransitionLookup};

pub(in crate::process) fn probe(
    instance: &mut EngineInstance,
    context: &mut Context32,
    selected: &mut Option<RegionRef>,
    cache: &mut CoreCache32,
    pc: u32,
) -> u32 {
    let Some(current) = selected.as_ref() else {
        return u32::MAX;
    };
    let raw = context.bytes();
    if u32::from_le_bytes(raw[112..116].try_into().unwrap()) != 1
        || u64::from_le_bytes(raw[120..128].try_into().unwrap()) != current.identity()
    {
        return u32::MAX;
    }
    let Some(memory) = instance.memory.as_ref() else {
        return u32::MAX;
    };
    match cache.lookup_transition(memory, current, pc) {
        TransitionLookup::Stop => 0,
        TransitionLookup::Linked(code, slot) => {
            context.bytes_mut()[120..128].copy_from_slice(&code.identity().to_le_bytes());
            *selected = Some(code);
            slot
        }
        TransitionLookup::Miss => COLD_MARK,
    }
}

pub(in crate::process) fn chain_next(
    instance: &mut EngineInstance,
    context: &mut Context32,
    numeric: &mut NumericBank,
    selected: &mut Option<RegionRef>,
    cache: &mut CoreCache32,
    cold: &mut ColdState,
) -> u32 {
    let pc = context.state().eip;
    let next = probe(instance, context, selected, cache, pc);
    if next != COLD_MARK {
        return next;
    }
    chain_cold(instance, context, numeric, selected, cache, cold)
}

pub(in crate::process) fn chain_cold(
    instance: &mut EngineInstance,
    context: &mut Context32,
    numeric: &mut NumericBank,
    selected: &mut Option<RegionRef>,
    cache: &mut CoreCache32,
    cold: &mut ColdState,
) -> u32 {
    let Some(current) = selected.as_ref() else {
        return u32::MAX;
    };
    let raw = context.bytes();
    if u32::from_le_bytes(raw[112..116].try_into().unwrap()) != 1
        || u64::from_le_bytes(raw[120..128].try_into().unwrap()) != current.identity()
    {
        return u32::MAX;
    }
    let segments = numeric.segment_context();
    context.bytes_mut()[120..128].fill(0);
    *selected = None;
    let reason = run_with_lookups(
        instance,
        context,
        numeric,
        &segments,
        &|modules, pc| modules.gate_at(pc).map(|gate| gate.id),
        &|memory, pc| cache.frontier_contains(memory, pc),
        cold,
    );
    if reason != StopReason::NeedCode {
        return COLD_MARK | STOP_MARK | reason as u32;
    }
    let Some(memory) = instance.memory.as_ref() else {
        return COLD_MARK | STOP_MARK | StopReason::Infrastructure as u32;
    };
    if let Some((code, slot)) = cache.lookup_linked(memory, context.state().eip) {
        context.bytes_mut()[120..128].copy_from_slice(&code.identity().to_le_bytes());
        *selected = Some(code);
        return COLD_MARK | slot;
    }
    COLD_MARK
}

#[cfg(test)]
pub(in crate::process) fn run(
    instance: &mut EngineInstance,
    context: &mut Context32,
    numeric: &mut NumericBank,
    segments: &SegmentContext,
    gates: &[(GuestAddress, u32)],
    frontier: &[GuestAddress],
    heat: &mut ColdState,
) -> StopReason {
    run_with_frontier(
        instance,
        context,
        numeric,
        segments,
        gates,
        &|_, pc| frontier.binary_search_by_key(&pc, |entry| entry.0).is_ok(),
        heat,
    )
}

#[cfg(test)]
pub(in crate::process) fn run_with_frontier(
    instance: &mut EngineInstance,
    context: &mut Context32,
    numeric: &mut NumericBank,
    segments: &SegmentContext,
    gates: &[(GuestAddress, u32)],
    frontier: &impl Fn(&AddressSpace, u32) -> bool,
    heat: &mut ColdState,
) -> StopReason {
    run_with_lookups(
        instance,
        context,
        numeric,
        segments,
        &|_, pc| cold::gate_id(gates, pc),
        frontier,
        heat,
    )
}

pub(in crate::process) fn run_with_lookups(
    instance: &mut EngineInstance,
    context: &mut Context32,
    numeric: &mut NumericBank,
    segments: &SegmentContext,
    gate_at: &impl Fn(&ModuleProcess32, u32) -> Option<u32>,
    frontier: &impl Fn(&AddressSpace, u32) -> bool,
    heat: &mut ColdState,
) -> StopReason {
    loop {
        let outcome = {
            let EngineInstance {
                memory, modules, ..
            } = instance;
            let Some(memory) = memory.as_mut() else {
                return StopReason::Infrastructure;
            };
            cold::run_with_lookups(
                memory,
                context,
                segments,
                &|pc| gate_at(modules, pc),
                frontier,
                heat,
            )
        };
        let instruction = match outcome {
            ColdOutcome::Stop(reason) => return reason,
            ColdOutcome::NeedNative(instruction) => instruction,
        };
        if context.remaining() == 0 || context.state().eip != instruction.pc {
            return StopReason::Infrastructure;
        }
        let result = numeric.execute(instance, context, &instruction);
        if !matches!(result, 0 | 6 | 10) {
            return match result {
                1 => StopReason::NeedCode,
                2 => StopReason::Budget,
                3 => StopReason::MemoryFault,
                4 => StopReason::Unsupported,
                5 => StopReason::Debug,
                7 => StopReason::Host,
                9 => StopReason::CpuFault,
                _ => StopReason::Infrastructure,
            };
        }
        context.set_word(32, instruction.next);
        context.set_word(40, context.retired().wrapping_add(1));
        context.set_word(44, context.remaining() - 1);
        #[cfg(any(test, feature = "observability"))]
        heat.native_retired();
        let Some(memory) = instance.memory.as_ref() else {
            return StopReason::Infrastructure;
        };
        if result != 6 {
            heat.retired(memory, instruction.pc);
        }
        match result {
            6 => return StopReason::CodeInvalidated,
            10 => return StopReason::SystemEntry,
            _ => {}
        }
    }
}

#[cfg(test)]
#[test]
fn cold_diagnostics_count_only_completed_native_operations_including_code_writes() {
    use crate::{cpu::x86::State32, memory::GuestAddress};

    for (destination, reason, retired) in [
        (0x1000, StopReason::CodeInvalidated, 2),
        (0x5000, StopReason::MemoryFault, 1),
    ] {
        let mut instance = EngineInstance::new(4, 7).unwrap();
        instance.map(0x1000, 1, 7).unwrap();
        instance.map(0x3000, 1, 3).unwrap();
        let memory = instance.memory.as_mut().unwrap();
        memory
            .write(GuestAddress(0x1000), &[0xd9, 0x03, 0xd9, 0x1f])
            .unwrap();
        memory
            .write(GuestAddress(0x3000), &3_f32.to_le_bytes())
            .unwrap();
        let mut context = Context32::new(State32 {
            registers: [0, 0, 0, 0x3000, 0x3800, 0, 0, destination],
            eip: 0x1000,
            eflags: 2,
        });
        context.set_word(44, 2);
        context.set_word(112, 1);
        let mut bank = NumericBank::from_instance(&instance).unwrap();
        let segments = instance.segment_context().unwrap();
        let mut cold = ColdState::default();
        assert_eq!(
            run(
                &mut instance,
                &mut context,
                &mut bank,
                &segments,
                &[],
                &[],
                &mut cold
            ),
            reason
        );
        assert_eq!(context.retired(), retired);
        assert_eq!(cold.diagnostics_snapshot()[4], u64::from(retired));
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;

#[cfg(test)]
#[path = "dispatch_tests.rs"]
mod dispatch_tests;
