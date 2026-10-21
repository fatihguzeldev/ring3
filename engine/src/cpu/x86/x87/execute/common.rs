use super::*;

pub(super) fn evaluate(
    state: &mut X87State,
    operation: X87Operation,
    input: &[u8],
    mut memory: Option<&mut [u8; 108]>,
) -> Option<X87Effects> {
    if state.control & 0x3f != 0x3f
        || input.len() != operation.read_size()
        || operation.write_size() != 0 && memory.is_none()
    {
        return None;
    }
    let original_status = state.status;
    let mut status = original_status & !0x3f;
    let top = state.top();
    let mut memory_len = 0;
    match operation {
        X87Operation::Load(format) => {
            let destination = top.wrapping_sub(1) & 7;
            if occupied(state, destination) {
                return None;
            }
            let (value, exceptions) = load(format, input);
            write(state, destination, value);
            status = with_top((status & !C1) | exceptions, destination);
        }
        X87Operation::Store {
            format,
            pop,
            truncate,
        } => {
            let value = read(state, top, 0)?;
            let control = if truncate {
                (state.control & !0xc00) | 0xc00
            } else {
                state.control
            };
            let (bytes, exceptions, incremented) = store(format, value, control);
            memory_len = format.size();
            memory.as_deref_mut().unwrap()[..memory_len].copy_from_slice(&bytes[..memory_len]);
            status = (status & !C1) | exceptions | if incremented { C1 } else { 0 };
            if pop {
                state.tag |= 3 << (top * 2);
                status = with_top(status, top + 1);
            }
        }
        X87Operation::LoadRegister { index } => {
            let value = read(state, top, index)?;
            let destination = top.wrapping_sub(1) & 7;
            if occupied(state, destination) {
                return None;
            }
            write(state, destination, value);
            status = with_top(status, destination);
        }
        X87Operation::StoreRegister { index, pop } => {
            let destination = physical(top, index)?;
            let value = read(state, top, 0)?;
            write(state, destination, value);
            if pop {
                state.tag |= 3 << (top * 2);
                status = with_top(status, top + 1);
            }
        }
        X87Operation::Exchange { index } => {
            let other = physical(top, index)?;
            let a = read(state, top, 0)?;
            let b = read(state, top, index)?;
            write(state, top, b);
            write(state, other, a);
            status &= !C1;
        }
        X87Operation::Binary {
            kind,
            destination,
            source,
            pop,
        } => {
            let physical_destination = physical(top, destination)?;
            let a = read(state, top, destination)?;
            let b = read(state, top, source)?;
            let result = binary(kind, a, b, state.control);
            write(state, physical_destination, result.value);
            status = (status & !C1) | result.exceptions | if result.incremented { C1 } else { 0 };
            if pop {
                state.tag |= 3 << (top * 2);
                status = with_top(status, top + 1);
            }
        }
        X87Operation::BinaryMemory { kind, format } => {
            let a = read(state, top, 0)?;
            let (b, exceptions) = load(format, input);
            if !matches!(a.kind(), Kind::NaN { .. }) || exceptions & INVALID != 0 {
                status |= exceptions;
            }
            let result = binary(kind, a, b, state.control);
            write(state, top, result.value);
            status = (status & !C1) | result.exceptions | if result.incremented { C1 } else { 0 };
        }
        X87Operation::ChangeSign | X87Operation::Absolute => {
            let value = read(state, top, 0)?;
            write(
                state,
                top,
                if matches!(operation, X87Operation::ChangeSign) {
                    value.negated()
                } else {
                    value.absolute()
                },
            );
            status &= !C1;
        }
        _ => return None,
    }
    // all exceptions are masked; keep the same new-exception priority and old sticky bits.
    let mut raised = status & 0x3f;
    if raised & INVALID != 0 {
        raised = INVALID;
    } else if raised & 4 != 0 {
        raised = 4;
    }
    state.status = (status & !0x3f) | (original_status & 0x3f) | raised;
    Some(X87Effects {
        memory_len,
        eflags: None,
        unmasked_exception: false,
    })
}

fn physical(top: usize, index: u8) -> Option<usize> {
    (index < 8).then_some((top + usize::from(index)) & 7)
}

fn occupied(state: &X87State, physical: usize) -> bool {
    (state.tag >> (physical * 2)) & 3 != 3
}

fn read(state: &X87State, top: usize, index: u8) -> Option<Ext80> {
    let physical = physical(top, index)?;
    occupied(state, physical).then(|| Ext80(state.registers[physical]))
}

fn write(state: &mut X87State, physical: usize, value: Ext80) {
    state.registers[physical] = value.0;
    state.tag = (state.tag & !(3 << (physical * 2))) | (value.tag() << (physical * 2));
}

fn with_top(status: u16, top: usize) -> u16 {
    (status & !0x3800) | (((top & 7) as u16) << 11)
}
