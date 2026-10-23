use super::{
    integer,
    memory::{Failure, Port},
};
use crate::cpu::{
    core::StopReason,
    x86::{
        Register32, SegmentRegister, State32,
        atomic::{self, AtomicKind},
        ir::*,
    },
};

/// REP work is charged per completed element; retirement is charged only when
/// the opcode completes. The caller must preserve `state` even on partial faults
/// and record an observed instruction only when instruction_completed is true.
pub(super) struct Progress {
    pub(super) retired: u32,
    pub(super) work: u32,
    pub(super) instruction_completed: bool,
    pub(super) stop: Option<StopReason>,
    pub(super) failure: Option<Failure>,
}

fn finish(
    state: &mut State32,
    pc: u32,
    next: u32,
    work: u32,
    completed: bool,
    stop: Option<StopReason>,
    failure: Option<Failure>,
) -> Progress {
    state.eip = if completed { next } else { pc };
    Progress {
        retired: u32::from(completed),
        work,
        instruction_completed: completed,
        stop,
        failure,
    }
}

pub(super) fn execute(
    operation: Operation,
    pc: u32,
    next: u32,
    state: &mut State32,
    port: &mut Port<'_>,
    budget: u32,
) -> Option<Progress> {
    let (kind, width, size, segment, repeat) = match operation {
        Operation::String {
            kind,
            width,
            address_size,
            source_segment,
        } => (kind, width, address_size, source_segment, None),
        Operation::RepeatString {
            kind,
            width,
            address_size,
            source_segment,
            repeat,
        } => (kind, width, address_size, source_segment, Some(repeat)),
        _ => return None,
    };
    let width = match width {
        StringWidth::Byte => 1,
        StringWidth::Word => 2,
        StringWidth::Dword => 4,
    };
    let source = EffectiveAddress {
        base: Some(Register32::Esi),
        segment,
        address_size: size,
        ..Default::default()
    };
    let destination = EffectiveAddress {
        base: Some(Register32::Edi),
        segment: Some(SegmentRegister::Es),
        address_size: size,
        ..Default::default()
    };
    if budget == 0 {
        return Some(finish(
            state,
            pc,
            next,
            0,
            false,
            Some(StopReason::Budget),
            None,
        ));
    }
    if repeat.is_some() && count(state, size) == 0 {
        return Some(finish(state, pc, next, 1, true, None, None));
    }
    let mut work = 0;
    loop {
        if work == budget {
            return Some(finish(
                state,
                pc,
                next,
                work,
                false,
                Some(StopReason::Budget),
                None,
            ));
        }
        // Each element commits its state only after all fallible accesses. A
        // later failed element keeps the already committed prefix unchanged.
        if let Err(error) = element(kind, width, size, source, destination, state, port) {
            return Some(finish(state, pc, next, work, false, None, Some(error)));
        }
        work += 1;
        let completed = match repeat {
            None => true,
            Some(condition) => {
                let remaining = count(state, size).wrapping_sub(1);
                state.registers[1] = selected(state.registers[1], remaining, size);
                remaining == 0
                    || match condition {
                        RepeatCondition::Always => false,
                        RepeatCondition::Equal => state.eflags & 0x40 == 0,
                        RepeatCondition::NotEqual => state.eflags & 0x40 != 0,
                    }
            }
        };
        if port.code_written {
            return Some(finish(
                state,
                pc,
                next,
                work,
                completed,
                Some(StopReason::CodeInvalidated),
                None,
            ));
        }
        if completed {
            return Some(finish(state, pc, next, work, true, None, None));
        }
    }
}

fn count(state: &State32, size: AddressSize) -> u32 {
    match size {
        AddressSize::Bits16 => state.registers[1] & 0xffff,
        AddressSize::Bits32 => state.registers[1],
    }
}

fn selected(previous: u32, value: u32, size: AddressSize) -> u32 {
    match size {
        AddressSize::Bits16 => previous & !0xffff | value & 0xffff,
        AddressSize::Bits32 => value,
    }
}

fn advance(state: &mut State32, index: usize, width: u32, size: AddressSize) {
    let previous = state.registers[index];
    let value = if state.eflags & 0x400 != 0 {
        previous.wrapping_sub(width)
    } else {
        previous.wrapping_add(width)
    };
    state.registers[index] = selected(previous, value, size);
}

fn element(
    kind: StringKind,
    width: u32,
    size: AddressSize,
    source: EffectiveAddress,
    destination: EffectiveAddress,
    state: &mut State32,
    port: &mut Port<'_>,
) -> Result<(), Failure> {
    let source_used = matches!(
        kind,
        StringKind::Move | StringKind::Load | StringKind::Compare
    );
    let destination_used = matches!(
        kind,
        StringKind::Move | StringKind::Store | StringKind::Compare | StringKind::Scan
    );
    // Preserve the exact owner's current-element span ordering. No permission,
    // memory translation, or future-element access is performed here.
    if source_used {
        port.check(source, state, width, false)?;
    }
    if destination_used {
        port.check(
            destination,
            state,
            width,
            matches!(kind, StringKind::Move | StringKind::Store),
        )?;
    }
    match kind {
        StringKind::Move => {
            let value = port.read(source, state, width, false)?;
            port.write(destination, state, width, value)?;
        }
        StringKind::Store => port.write(destination, state, width, state.registers[0])?,
        StringKind::Load => {
            let value = port.read(source, state, width, false)?;
            let mask = integer::mask(width);
            state.registers[0] = state.registers[0] & !mask | value & mask;
        }
        StringKind::Compare | StringKind::Scan => {
            let left = if kind == StringKind::Compare {
                port.read(source, state, width, false)?
            } else {
                state.registers[0]
            };
            let right = port.read(destination, state, width, false)?;
            state.eflags = atomic::evaluate(AtomicKind::Subtract, width, left, right, state.eflags)
                .ok_or(Failure::Infrastructure)?
                .flags;
        }
    }
    if source_used {
        advance(state, 6, width, size);
    }
    if destination_used {
        advance(state, 7, width, size);
    }
    Ok(())
}

#[cfg(test)]
#[path = "strings/tests.rs"]
mod tests;
