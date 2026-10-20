use super::{Context32, NativeInstruction, StopReason};
use crate::{
    cpu::x86::{
        SegmentContext, State32,
        decode::{
            BoundDecode, CapturedFetchFaultKind32, CodeFetchContext, DecodeAuthority, DecodeError,
            DecodePolicy, decode_for_compilation,
        },
    },
    memory::{AddressSpace, GuestAddress},
};

mod control;
mod integer;
mod memory;
use memory::{Failure, Port};

const HEAT_SLOTS: usize = 8192;
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

pub(crate) struct ColdHeat {
    rows: Box<[HeatRow]>,
    memory: u64,
    version: u64,
    generation: u64,
    threshold: u32,
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
            version: 0,
            generation: 1,
            threshold: threshold.max(1),
        }
    }

    fn index(pc: u32) -> usize {
        ((pc ^ pc.rotate_left(13)) as usize) & (HEAT_SLOTS - 1)
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
        if self.memory != memory.identity() || self.version != memory.code_source_version() {
            self.advance();
            self.memory = memory.identity();
            self.version = memory.code_source_version();
        }
    }

    fn hot(&mut self, memory: &AddressSpace, pc: u32) -> bool {
        self.source(memory);
        let row = &self.rows[Self::index(pc)];
        row.generation == self.generation && row.pc == pc && row.visits >= self.threshold
    }

    fn requested(&mut self) {
        // adjacent hot instructions must also requalify after the compiled unit is evicted.
        self.advance();
    }

    pub(crate) fn retired(&mut self, memory: &AddressSpace, pc: u32) {
        self.source(memory);
        let row = &mut self.rows[Self::index(pc)];
        if row.generation != self.generation || row.pc != pc {
            *row = HeatRow {
                pc,
                visits: 1,
                generation: self.generation,
            };
        } else {
            row.visits = row.visits.saturating_add(1);
        }
    }

    #[cfg(test)]
    fn with_threshold(threshold: u32) -> Self {
        Self::new(threshold)
    }
}

// gates and frontier are sorted snapshots from the same exclusive cpu lifetime.
pub(crate) fn run(
    memory: &mut AddressSpace,
    context: &mut Context32,
    segments: &SegmentContext,
    gates: &[(GuestAddress, u32)],
    frontier: &[GuestAddress],
    heat: &mut ColdHeat,
) -> ColdOutcome {
    if word(context, 112) != 1 || context.bytes()[120..128] != [0; 8] || !segments.is_legacy() {
        return ColdOutcome::Stop(StopReason::Infrastructure);
    }
    let Some(fetch) = CodeFetchContext::from_segments(segments) else {
        return ColdOutcome::Stop(StopReason::Infrastructure);
    };
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
        match gate(memory, fetch, pc, gates) {
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
        if frontier.binary_search_by_key(&pc, |entry| entry.0).is_ok() {
            heat.requested();
            return finish(context, state, retired, remaining, StopReason::NeedCode);
        }
        let decoded = match decode_for_compilation(
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
                        heat.requested();
                        return finish(context, state, retired, remaining, StopReason::NeedCode);
                    }
                };
                let reason = error.publish(context);
                return finish(context, state, retired, remaining, reason);
            }
        };
        if decoded.segment_accesses() & (1 << 5) != 0
            || decoded.is_privileged_instruction()
            || heat.hot(memory, pc)
        {
            heat.requested();
            return finish(context, state, retired, remaining, StopReason::NeedCode);
        }
        let operation = *decoded.operation();
        let next = decoded.next_pc().0;
        if super::emitter::is_native(&operation) {
            publish(context, state, retired, remaining);
            return ColdOutcome::NeedNative(NativeInstruction {
                pc,
                next,
                operation,
            });
        }
        let mut after = state;
        after.eip = next;
        let mut port = Port {
            memory: &mut *memory,
            segments,
            code_written: false,
        };
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
                    return finish(
                        context,
                        state,
                        retired,
                        remaining,
                        StopReason::CodeInvalidated,
                    );
                }
                heat.retired(memory, pc);
            }
            Ok(false) => {
                heat.requested();
                return finish(context, state, retired, remaining, StopReason::NeedCode);
            }
            Err(error) => {
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

fn gate(
    memory: &AddressSpace,
    fetch: CodeFetchContext,
    pc: u32,
    gates: &[(GuestAddress, u32)],
) -> Result<Option<u32>, Failure> {
    let Ok(index) = gates.binary_search_by_key(&pc, |(entry, _)| entry.0) else {
        return Ok(None);
    };
    let id = gates[index].1;
    if id == 0
        || index > 0 && gates[index - 1].0.0 == pc
        || gates.get(index + 1).is_some_and(|gate| gate.0.0 == pc)
    {
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
