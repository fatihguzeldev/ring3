use crate::{
    cpu::{
        core::{
            COLD_MARK, Context32, STOP_MARK, StopReason,
            cold::{self, ColdObserver, ColdOutcome, ColdState, Unobserved},
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

pub(in crate::process) fn probe_executable(
    instance: &mut EngineInstance,
    context: &mut Context32,
    selected: &mut Option<RegionRef>,
    cache: &mut CoreCache32,
    pc: u32,
    remaining: u32,
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
    if !current.is_current(memory) {
        return 0;
    }
    let header = cache.continuation_header();
    let origin = (header.active == 1
        && header.current_id == current.identity()
        && header.context == context.bytes().as_ptr() as usize as u32
        && header.memory == memory.identity()
        && header.nonce != 0
        && header.current_nonce == header.nonce)
        .then_some(header.current_entry);
    let Some((code, slot)) = cache.lookup_executable_linked_context(memory, origin, pc, remaining)
    else {
        return COLD_MARK;
    };
    context.bytes_mut()[120..128].copy_from_slice(&code.identity().to_le_bytes());
    *selected = Some(code);
    slot
}

pub(in crate::process) fn precise(
    instance: &mut EngineInstance,
    context: &mut Context32,
    numeric: &mut NumericBank,
    selected: &mut Option<RegionRef>,
    cache: &CoreCache32,
    heat: &mut ColdState,
    observer: &mut impl ColdObserver,
) -> StopReason {
    let Some(source) = selected.as_ref().cloned() else {
        observer.abandon_instruction();
        return StopReason::Infrastructure;
    };
    let identity = u64::from_le_bytes(context.bytes()[120..128].try_into().unwrap());
    let Some(memory) = instance.memory.as_ref() else {
        observer.abandon_instruction();
        return StopReason::Infrastructure;
    };
    if context.bytes()[112..116] != 1_u32.to_le_bytes()
        || identity != source.identity()
        || !source.contains_instruction(context.state().eip)
    {
        observer.abandon_instruction();
        return StopReason::Infrastructure;
    }
    if !cache.precise_source_current(memory, &source) {
        observer.abandon_instruction();
        let reason = if source.is_current(memory) {
            StopReason::Infrastructure
        } else {
            StopReason::CodeInvalidated
        };
        context.bytes_mut()[120..128].fill(0);
        // Keep stale selection metadata for the public begin/reselect protocol;
        // the cleared Context identity grants no execution capability.
        return reason;
    }
    if !numeric.begin_precise() {
        observer.abandon_instruction();
        return StopReason::Infrastructure;
    }
    let initial_remaining = context.remaining();
    let segments = numeric.segment_context();
    let bypass = observer.bypass_token().unwrap_or((0, 0));
    context.bytes_mut()[120..128].fill(0);
    *selected = None;
    let mut precise_observer = PreciseObserver {
        observer,
        cache,
        suppress_heat: false,
        force_precise: true,
        source: Some(&source),
        initial_remaining,
        bypass,
    };
    let reason = run_observed_progress(
        instance,
        context,
        numeric,
        &segments,
        &|modules, pc| modules.gate_at(pc).map(|gate| gate.id),
        &|_, _, _| false,
        heat,
        &mut precise_observer,
    );
    if !matches!(reason, StopReason::NeedCode | StopReason::Budget) {
        // Some terminal paths (gates/fetch faults/debug state) stop before an
        // instruction begins. They still end the capture rather than leave an
        // unobserved host/fault transition between later recorded events.
        precise_observer.abandon_instruction();
    }
    if instance
        .memory
        .as_ref()
        .is_some_and(|memory| cache.precise_source_current(memory, &source))
    {
        context.bytes_mut()[120..128].copy_from_slice(&source.identity().to_le_bytes());
        *selected = Some(source);
    } else {
        // core_begin must report a stale selection (4), rather than no selection
        // (7). Numeric/compiled entry still require a live matching identity.
        *selected = Some(source);
    }
    reason
}

struct PreciseObserver<'a, O> {
    observer: &'a mut O,
    cache: &'a CoreCache32,
    suppress_heat: bool,
    force_precise: bool,
    source: Option<&'a RegionRef>,
    initial_remaining: u32,
    bypass: (u64, u32),
}

impl<O: ColdObserver> ColdObserver for PreciseObserver<'_, O> {
    fn exact_cold(&self) -> bool {
        self.force_precise || self.observer.exact_cold()
    }
    fn qualify_step(
        &mut self,
        memory: &AddressSpace,
        pc: u32,
        progress: cold::ColdProgress,
    ) -> Option<cold::ColdStepQualification> {
        let bypass = if progress.retired == self.bypass.1 {
            self.bypass.0
        } else {
            0
        };
        let (owned, frontier) =
            self.cache
                .qualify_cold_step(memory, pc, progress.remaining, bypass);
        self.suppress_heat |= owned;
        let frontier = self.source.map_or(frontier, |source| {
            progress.remaining < self.initial_remaining
                && (source.eligible_hot_span(pc, progress.remaining).is_some()
                    && self.cache.precise_source_current(memory, source)
                    || frontier)
        });
        Some(cold::ColdStepQualification {
            frontier,
            track_heat: !self.suppress_heat && self.observer.tracks_heat(memory, pc),
        })
    }
    fn tracks_heat(&self, memory: &AddressSpace, pc: u32) -> bool {
        !self.suppress_heat
            && self.cache.current_hot_port(memory, pc).is_none()
            && self.observer.tracks_heat(memory, pc)
    }
    fn begin_instruction(
        &mut self,
        memory: &AddressSpace,
        instruction: &crate::cpu::core::NativeInstruction,
        hot: bool,
    ) -> bool {
        self.observer.begin_instruction(memory, instruction, hot) || self.force_precise
    }
    fn complete_instruction(&mut self, memory: &AddressSpace, actual_post_pc: u32) {
        self.observer.complete_instruction(memory, actual_post_pc);
    }
    fn abandon_instruction(&mut self) {
        self.observer.abandon_instruction();
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
    chain_next_observed(
        instance,
        context,
        numeric,
        selected,
        cache,
        cold,
        &mut Unobserved,
    )
}

pub(in crate::process) fn chain_next_observed(
    instance: &mut EngineInstance,
    context: &mut Context32,
    numeric: &mut NumericBank,
    selected: &mut Option<RegionRef>,
    cache: &mut CoreCache32,
    cold: &mut ColdState,
    observer: &mut impl ColdObserver,
) -> u32 {
    let pc = context.state().eip;
    let next = probe(instance, context, selected, cache, pc);
    if next != COLD_MARK {
        return next;
    }
    chain_cold_observed(instance, context, numeric, selected, cache, cold, observer)
}

pub(in crate::process) fn chain_cold(
    instance: &mut EngineInstance,
    context: &mut Context32,
    numeric: &mut NumericBank,
    selected: &mut Option<RegionRef>,
    cache: &mut CoreCache32,
    cold: &mut ColdState,
) -> u32 {
    chain_cold_observed(
        instance,
        context,
        numeric,
        selected,
        cache,
        cold,
        &mut Unobserved,
    )
}

pub(in crate::process) fn chain_cold_observed(
    instance: &mut EngineInstance,
    context: &mut Context32,
    numeric: &mut NumericBank,
    selected: &mut Option<RegionRef>,
    cache: &mut CoreCache32,
    cold: &mut ColdState,
    observer: &mut impl ColdObserver,
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
    let (bypass, bypass_retired) = observer
        .bypass_token()
        .unwrap_or((cache.trace_bypass_identity(), context.retired()));
    let reason = run_executable_observed(
        instance,
        context,
        numeric,
        &segments,
        &|modules, pc| modules.gate_at(pc).map(|gate| gate.id),
        cache,
        (bypass, bypass_retired),
        cold,
        observer,
    );
    cache.bypass_trace(if context.retired() == bypass_retired {
        bypass
    } else {
        0
    });
    if reason != StopReason::NeedCode {
        return COLD_MARK | STOP_MARK | reason as u32;
    }
    let Some(memory) = instance.memory.as_ref() else {
        return COLD_MARK | STOP_MARK | StopReason::Infrastructure as u32;
    };
    if let Some((code, slot)) =
        cache.lookup_executable_linked(memory, context.state().eip, context.remaining())
    {
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
    run_observed(
        instance,
        context,
        numeric,
        segments,
        gate_at,
        &|memory, pc, _| frontier(memory, pc),
        heat,
        &mut Unobserved,
    )
}

pub(in crate::process) fn run_executable_observed(
    instance: &mut EngineInstance,
    context: &mut Context32,
    numeric: &mut NumericBank,
    segments: &SegmentContext,
    gate_at: &impl Fn(&ModuleProcess32, u32) -> Option<u32>,
    cache: &CoreCache32,
    bypass: (u64, u32),
    heat: &mut ColdState,
    observer: &mut impl ColdObserver,
) -> StopReason {
    let mut observed = PreciseObserver {
        observer,
        cache,
        suppress_heat: false,
        force_precise: false,
        source: None,
        initial_remaining: 0,
        bypass,
    };
    run_observed_progress(
        instance,
        context,
        numeric,
        segments,
        gate_at,
        &|_, _, _| false,
        heat,
        &mut observed,
    )
}

pub(in crate::process) fn run_observed(
    instance: &mut EngineInstance,
    context: &mut Context32,
    numeric: &mut NumericBank,
    segments: &SegmentContext,
    gate_at: &impl Fn(&ModuleProcess32, u32) -> Option<u32>,
    frontier: &impl Fn(&AddressSpace, u32, u32) -> bool,
    heat: &mut ColdState,
    observer: &mut impl ColdObserver,
) -> StopReason {
    run_observed_progress(
        instance,
        context,
        numeric,
        segments,
        gate_at,
        &|memory, pc, progress| frontier(memory, pc, progress.retired),
        heat,
        observer,
    )
}

pub(in crate::process) fn run_observed_progress(
    instance: &mut EngineInstance,
    context: &mut Context32,
    numeric: &mut NumericBank,
    segments: &SegmentContext,
    gate_at: &impl Fn(&ModuleProcess32, u32) -> Option<u32>,
    frontier: &impl Fn(&AddressSpace, u32, cold::ColdProgress) -> bool,
    heat: &mut ColdState,
    observer: &mut impl ColdObserver,
) -> StopReason {
    loop {
        let outcome = {
            let EngineInstance {
                memory, modules, ..
            } = instance;
            let Some(memory) = memory.as_mut() else {
                return StopReason::Infrastructure;
            };
            cold::run_observed_progress(
                memory,
                context,
                segments,
                &|pc| gate_at(modules, pc),
                frontier,
                heat,
                observer,
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
            observer.abandon_instruction();
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
        if result != 6 && observer.tracks_heat(memory, instruction.pc) {
            heat.retired(memory, instruction.pc);
        }
        match result {
            6 => {
                observer.abandon_instruction();
                return StopReason::CodeInvalidated;
            }
            10 => {
                observer.abandon_instruction();
                return StopReason::SystemEntry;
            }
            _ => observer.complete_instruction(memory, context.state().eip),
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

#[cfg(test)]
#[path = "precise_tests.rs"]
mod precise_tests;

#[cfg(test)]
#[path = "executable_tests.rs"]
mod executable_tests;
