use super::{Context32, NativeInstruction, StopReason};
use crate::{
    cpu::x86::{
        SegmentContext, State32,
        decode::{
            BoundDecode, CapturedFetchFaultKind32, CodeFetchContext, DecodeAuthority, DecodeError,
            DecodePolicy, DecodeScratch,
        },
    },
    memory::{AddressSpace, GuestAddress},
};

mod control;
#[cfg(any(test, feature = "observability"))]
mod diagnostics;
mod integer;
mod memory;
mod strings;
use memory::{Failure, Port};

/// The normal executor records only instructions whose complete architectural
/// effects have committed. A decoder result alone never confirms execution.
pub(crate) trait ColdObserver {
    fn exact_cold(&self) -> bool {
        false
    }
    fn begin_instruction(
        &mut self,
        _memory: &AddressSpace,
        _instruction: &NativeInstruction,
        _hot: bool,
    ) -> bool {
        false
    }
    fn complete_instruction(&mut self, _memory: &AddressSpace, _actual_post_pc: u32) {}
    fn abandon_instruction(&mut self) {}
}

pub(crate) struct Unobserved;
impl ColdObserver for Unobserved {}

const HEAT_SLOTS: usize = 32768;
const HEAT_WAYS: usize = 4;
const HEAT_SETS: usize = HEAT_SLOTS / HEAT_WAYS;
// this repeat gate is provisional; its amortization point must be measured.
const HOT_VISITS: u32 = 64;

#[derive(Clone, Copy, Debug)]
pub(crate) enum ColdOutcome {
    Stop(StopReason),
    NeedNative(NativeInstruction),
}

#[derive(Clone, Copy, Default)]
struct HeatRow {
    pc: u32,
    visits: u32,
    generation: u64,
}

struct ColdHeat {
    rows: Box<[HeatRow]>,
    memory: u64,
    generation: u64,
    threshold: u32,
    #[cfg(any(test, feature = "observability"))]
    diagnostics: diagnostics::Diagnostics,
}

#[derive(Default)]
pub(crate) struct ColdState {
    heat: ColdHeat,
    decode: DecodeScratch,
}

impl ColdState {
    pub(crate) fn retired(&mut self, memory: &AddressSpace, pc: u32) {
        self.heat.retired(memory, pc);
    }

    pub(crate) fn forget_compiled(
        &mut self,
        memory: &AddressSpace,
        pcs: impl IntoIterator<Item = u32>,
    ) {
        if self.heat.memory != memory.identity() {
            return;
        }
        for pc in pcs {
            self.heat.requested(pc);
        }
    }

    #[cfg(test)]
    fn with_threshold(threshold: u32) -> Self {
        Self {
            heat: ColdHeat::new(threshold),
            decode: DecodeScratch::default(),
        }
    }
}

impl Default for ColdHeat {
    fn default() -> Self {
        Self::new(HOT_VISITS)
    }
}

impl ColdHeat {
    fn new(threshold: u32) -> Self {
        Self {
            rows: vec![HeatRow::default(); HEAT_SLOTS].into_boxed_slice(),
            memory: 0,
            generation: 1,
            threshold: threshold.max(1),
            #[cfg(any(test, feature = "observability"))]
            diagnostics: diagnostics::Diagnostics::default(),
        }
    }

    fn index(pc: u32) -> usize {
        ((pc ^ (pc >> 11) ^ (pc >> 22)) as usize & (HEAT_SETS - 1)) * HEAT_WAYS
    }

    fn advance(&mut self) {
        if let Some(generation) = self.generation.checked_add(1) {
            self.generation = generation;
        } else {
            self.rows.fill(HeatRow::default());
            self.generation = 1;
        }
    }

    fn source(&mut self, memory: &AddressSpace) {
        if self.memory != memory.identity() {
            #[cfg(any(test, feature = "observability"))]
            self.diagnostics.source_reset();
            self.advance();
            self.memory = memory.identity();
        }
    }

    fn hot(&mut self, memory: &AddressSpace, pc: u32) -> bool {
        self.source(memory);
        let first = Self::index(pc);
        self.rows[first..first + HEAT_WAYS].iter().any(|row| {
            row.generation == self.generation && row.pc == pc && row.visits >= self.threshold
        })
    }

    fn requested(&mut self, pc: u32) {
        // a request consumes this site's reuse, not unrelated completed work.
        let first = Self::index(pc);
        for row in &mut self.rows[first..first + HEAT_WAYS] {
            if row.generation == self.generation && row.pc == pc {
                *row = HeatRow::default();
            }
        }
    }

    fn retired(&mut self, memory: &AddressSpace, pc: u32) {
        self.source(memory);
        let first = Self::index(pc);
        let rows = &mut self.rows[first..first + HEAT_WAYS];
        let mut replacement = 0;
        let mut fewest_visits = u32::MAX;
        let mut expired = false;
        for (index, row) in rows.iter_mut().enumerate() {
            if row.generation == self.generation {
                if row.pc == pc {
                    row.visits = row.visits.saturating_add(1);
                    return;
                }
                if !expired && row.visits < fewest_visits {
                    replacement = index;
                    fewest_visits = row.visits;
                }
            } else if !expired {
                replacement = index;
                expired = true;
            }
        }
        #[cfg(any(test, feature = "observability"))]
        if rows[replacement].generation == self.generation {
            self.diagnostics.occupied_replacement();
        }
        rows[replacement] = HeatRow {
            pc,
            visits: 1,
            generation: self.generation,
        };
    }
}

// gates and frontier belong to the same exclusive cpu lifetime.
#[cfg(test)]
pub(crate) fn run(
    memory: &mut AddressSpace,
    context: &mut Context32,
    segments: &SegmentContext,
    gates: &[(GuestAddress, u32)],
    frontier: &[GuestAddress],
    cold: &mut ColdState,
) -> ColdOutcome {
    run_with_frontier(
        memory,
        context,
        segments,
        gates,
        &|_, pc| frontier.binary_search_by_key(&pc, |entry| entry.0).is_ok(),
        cold,
    )
}

#[cfg(test)]
pub(crate) fn run_with_frontier(
    memory: &mut AddressSpace,
    context: &mut Context32,
    segments: &SegmentContext,
    gates: &[(GuestAddress, u32)],
    frontier: &impl Fn(&AddressSpace, u32) -> bool,
    cold: &mut ColdState,
) -> ColdOutcome {
    run_with_lookups(
        memory,
        context,
        segments,
        &|pc| gate_id(gates, pc),
        frontier,
        cold,
    )
}

pub(crate) fn run_with_lookups(
    memory: &mut AddressSpace,
    context: &mut Context32,
    segments: &SegmentContext,
    gate_at: &impl Fn(u32) -> Option<u32>,
    frontier: &impl Fn(&AddressSpace, u32) -> bool,
    cold: &mut ColdState,
) -> ColdOutcome {
    run_observed(
        memory,
        context,
        segments,
        gate_at,
        frontier,
        cold,
        &mut Unobserved,
    )
}

pub(crate) fn run_observed(
    memory: &mut AddressSpace,
    context: &mut Context32,
    segments: &SegmentContext,
    gate_at: &impl Fn(u32) -> Option<u32>,
    frontier: &impl Fn(&AddressSpace, u32) -> bool,
    cold: &mut ColdState,
    observer: &mut impl ColdObserver,
) -> ColdOutcome {
    if word(context, 112) != 1 || context.bytes()[120..128] != [0; 8] || !segments.is_legacy() {
        return ColdOutcome::Stop(StopReason::Infrastructure);
    }
    let Some(fetch) = CodeFetchContext::from_segments(segments) else {
        return ColdOutcome::Stop(StopReason::Infrastructure);
    };
    let ColdState { heat, decode } = cold;
    let mut state = context.state();
    let mut retired = context.retired();
    let mut remaining = context.remaining();
    if state.eflags & 0x50100 != 0 {
        return finish(context, state, retired, remaining, StopReason::Debug);
    }
    loop {
        if remaining == 0 {
            return finish(context, state, retired, remaining, StopReason::Budget);
        }
        let pc = state.eip;
        match gate(memory, fetch, pc, gate_at(pc)) {
            Ok(Some(id)) => {
                context.set_word(116, id);
                return finish(context, state, retired, remaining, StopReason::Host);
            }
            Ok(None) => {}
            Err(error) => {
                let reason = error.publish(context);
                return finish(context, state, retired, remaining, reason);
            }
        }
        if frontier(memory, pc) {
            return finish(context, state, retired, remaining, StopReason::NeedCode);
        }
        let decoded = match decode.decode(
            memory,
            GuestAddress(pc),
            DecodePolicy::Execution,
            Some(fetch),
            DecodeAuthority::default(),
        ) {
            Ok(BoundDecode::Instruction(decoded)) => decoded,
            Ok(BoundDecode::CodeFault(fault)) => {
                let error = match fault.kind {
                    CapturedFetchFaultKind32::Segment => {
                        Failure::Segment(crate::cpu::x86::SegmentRegister::Cs)
                    }
                    CapturedFetchFaultKind32::Paging => {
                        let mut bytes = [0; 15];
                        match fetch.fetch(
                            memory,
                            fault.pc,
                            &mut bytes[..usize::from(fault.attempted_length)],
                        ) {
                            Err(error) => Failure::paging(error, u32::from(fault.attempted_length)),
                            Ok(()) => Failure::Infrastructure,
                        }
                    }
                };
                let reason = error.publish(context);
                return finish(context, state, retired, remaining, reason);
            }
            Err(error) => {
                let error = match error {
                    DecodeError::MemoryFault { fault, length, .. } => {
                        Failure::Memory(crate::memory::MemoryError::Fault(fault), length)
                    }
                    DecodeError::Paging {
                        error,
                        attempted_length,
                        ..
                    } => Failure::paging(error, u32::from(attempted_length)),
                    DecodeError::CodeSegment { .. } => {
                        Failure::Segment(crate::cpu::x86::SegmentRegister::Cs)
                    }
                    DecodeError::Infrastructure(_) => Failure::Infrastructure,
                    _ => {
                        #[cfg(any(test, feature = "observability"))]
                        heat.diagnostics.request(diagnostics::Request::Decode);
                        heat.requested(pc);
                        return finish(
                            context,
                            state,
                            retired,
                            remaining,
                            if observer.exact_cold() {
                                StopReason::Unsupported
                            } else {
                                StopReason::NeedCode
                            },
                        );
                    }
                };
                let reason = error.publish(context);
                return finish(context, state, retired, remaining, reason);
            }
        };
        if decoded.segment_accesses() & (1 << 5) != 0 {
            #[cfg(any(test, feature = "observability"))]
            heat.diagnostics.request(diagnostics::Request::Admission);
            heat.requested(pc);
            return finish(
                context,
                state,
                retired,
                remaining,
                if observer.exact_cold() {
                    StopReason::Unsupported
                } else {
                    StopReason::NeedCode
                },
            );
        }
        let operation = *decoded.operation();
        let next = decoded.next_pc().0;
        let instruction = NativeInstruction {
            pc,
            next,
            operation,
        };
        let hot = heat.hot(memory, pc);
        if hot {
            heat.requested(pc);
        }
        if !observer.begin_instruction(memory, &instruction, hot) && hot {
            #[cfg(any(test, feature = "observability"))]
            heat.diagnostics.request(diagnostics::Request::Hot);
            return finish(context, state, retired, remaining, StopReason::NeedCode);
        }
        if let Some(result) = control::terminal(operation, &state) {
            observer.abandon_instruction();
            let reason = result.unwrap_or_else(|failure| failure.publish(context));
            return finish(context, state, retired, remaining, reason);
        }
        if super::emitter::is_native(&operation) {
            publish(context, state, retired, remaining);
            return ColdOutcome::NeedNative(instruction);
        }
        let mut after = state;
        after.eip = next;
        let mut port = Port {
            memory: &mut *memory,
            segments,
            code_written: false,
        };
        if let Some(progress) =
            strings::execute(operation, pc, next, &mut state, &mut port, remaining)
        {
            retired = retired.wrapping_add(progress.retired);
            remaining -= progress.work;
            if let Some(failure) = progress.failure {
                observer.abandon_instruction();
                let reason = failure.publish(context);
                return finish(context, state, retired, remaining, reason);
            }
            if let Some(reason) = progress.stop {
                observer.abandon_instruction();
                return finish(context, state, retired, remaining, reason);
            }
            if progress.instruction_completed {
                observer.complete_instruction(memory, state.eip);
                heat.retired(memory, pc);
                continue;
            }
            observer.abandon_instruction();
            return finish(
                context,
                state,
                retired,
                remaining,
                StopReason::Infrastructure,
            );
        }
        let completed = integer::execute(operation, &mut after, &mut port).and_then(|supported| {
            if supported {
                Ok(true)
            } else {
                control::execute(operation, next, &mut after, &mut port)
            }
        });
        let code_written = port.code_written;
        match completed {
            Ok(true) => {
                state = after;
                retired = retired.wrapping_add(1);
                remaining -= 1;
                if code_written {
                    observer.abandon_instruction();
                    return finish(
                        context,
                        state,
                        retired,
                        remaining,
                        StopReason::CodeInvalidated,
                    );
                }
                if let Some(result) = control::completed_stop(operation, &state) {
                    observer.abandon_instruction();
                    let reason = result.unwrap_or_else(|failure| failure.publish(context));
                    return finish(context, state, retired, remaining, reason);
                }
                observer.complete_instruction(memory, state.eip);
                heat.retired(memory, pc);
            }
            Ok(false) => {
                observer.abandon_instruction();
                #[cfg(any(test, feature = "observability"))]
                heat.diagnostics.request(diagnostics::Request::Unsupported);
                heat.requested(pc);
                return finish(
                    context,
                    state,
                    retired,
                    remaining,
                    if observer.exact_cold() {
                        StopReason::Unsupported
                    } else {
                        StopReason::NeedCode
                    },
                );
            }
            Err(error) => {
                observer.abandon_instruction();
                let reason = error.publish(context);
                return finish(context, state, retired, remaining, reason);
            }
        }
    }
}

fn word(context: &Context32, offset: usize) -> u32 {
    u32::from_le_bytes(context.bytes()[offset..offset + 4].try_into().unwrap())
}

fn publish(context: &mut Context32, state: State32, retired: u32, remaining: u32) {
    context.set_state(state);
    context.set_word(40, retired);
    context.set_word(44, remaining);
}

fn finish(
    context: &mut Context32,
    state: State32,
    retired: u32,
    remaining: u32,
    reason: StopReason,
) -> ColdOutcome {
    publish(context, state, retired, remaining);
    ColdOutcome::Stop(reason)
}

#[cfg(test)]
pub(crate) fn gate_id(gates: &[(GuestAddress, u32)], pc: u32) -> Option<u32> {
    let index = gates.binary_search_by_key(&pc, |(entry, _)| entry.0).ok()?;
    if index > 0 && gates[index - 1].0.0 == pc
        || gates.get(index + 1).is_some_and(|gate| gate.0.0 == pc)
    {
        return Some(0);
    }
    Some(gates[index].1)
}

fn gate(
    memory: &AddressSpace,
    fetch: CodeFetchContext,
    pc: u32,
    id: Option<u32>,
) -> Result<Option<u32>, Failure> {
    let Some(id) = id else {
        return Ok(None);
    };
    if id == 0 {
        return Err(Failure::Infrastructure);
    }
    let mut stub = [0; 2];
    fetch
        .fetch(memory, GuestAddress(pc), &mut stub)
        .map_err(|error| Failure::paging(error, 2))?;
    if stub != [0x0f, 0x0b] {
        return Err(Failure::Infrastructure);
    }
    Ok(Some(id))
}

#[cfg(test)]
#[path = "cold/tests.rs"]
mod tests;
