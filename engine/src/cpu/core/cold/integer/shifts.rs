use super::extended::signed;
use super::*;

#[derive(Clone, Copy)]
enum Kind {
    Shift(ShiftKind),
    Rotate(RotateKind, bool),
    Double(DoubleShiftKind),
}
struct Shift {
    kind: Kind,
    target: Operand,
    width: u32,
    count: ShiftCount,
    source: Option<Operand>,
}

fn lower(operation: Operation) -> Option<Shift> {
    use Kind::{Double, Rotate, Shift as S};
    use ShiftCount::{Cl, Immediate as N};
    let (kind, target, width, count, source) = match operation {
        Operation::Shift {
            kind,
            destination,
            count,
        } => (S(kind), location(destination), 4, count, None),
        Operation::ShiftWord {
            kind,
            destination,
            count,
        } => (S(kind), reg(destination), 2, count, None),
        Operation::ShiftByte {
            kind,
            destination,
            count,
        } => (S(kind), byte(destination), 1, count, None),
        Operation::MemoryShiftWord {
            kind,
            address,
            count,
        } => (S(kind), Operand::Memory(address), 2, count, None),
        Operation::MemoryShiftByte { kind, address } => {
            (S(kind), Operand::Memory(address), 1, N(1), None)
        }
        Operation::MemoryShiftByteImmediate {
            kind,
            address,
            count,
        } => (S(kind), Operand::Memory(address), 1, N(count), None),
        Operation::MemoryShiftByteCl { kind, address } => {
            (S(kind), Operand::Memory(address), 1, Cl, None)
        }
        Operation::Rotate {
            kind,
            destination,
            count,
        } => (Rotate(kind, false), reg(destination), 4, count, None),
        Operation::RotateOne { kind, destination } => {
            (Rotate(kind, false), reg(destination), 4, N(1), None)
        }
        Operation::RotateWord {
            kind,
            destination,
            count,
        } => (Rotate(kind, false), reg(destination), 2, count, None),
        Operation::ByteRotateImmediate {
            kind,
            destination,
            count,
        } => (Rotate(kind, false), byte(destination), 1, N(count), None),
        Operation::ByteRotateCl { kind, destination } => {
            (Rotate(kind, false), byte(destination), 1, Cl, None)
        }
        Operation::ByteRotateOne { kind, destination } => {
            (Rotate(kind, false), byte(destination), 1, N(1), None)
        }
        Operation::MemoryRotate {
            kind,
            address,
            count,
        } => (
            Rotate(kind, false),
            Operand::Memory(address),
            4,
            count,
            None,
        ),
        Operation::MemoryRotateOne { kind, address } => {
            (Rotate(kind, false), Operand::Memory(address), 4, N(1), None)
        }
        Operation::MemoryRotateWord {
            kind,
            address,
            count,
        } => (
            Rotate(kind, false),
            Operand::Memory(address),
            2,
            count,
            None,
        ),
        Operation::MemoryByteRotate {
            kind,
            address,
            count,
        } => (
            Rotate(kind, false),
            Operand::Memory(address),
            1,
            count,
            None,
        ),
        Operation::MemoryByteRotateOne { kind, address } => {
            (Rotate(kind, false), Operand::Memory(address), 1, N(1), None)
        }
        Operation::RotateThroughCarryOne { kind, destination } => {
            (Rotate(kind, true), reg(destination), 4, N(1), None)
        }
        Operation::RotateThroughCarryImmediate {
            kind,
            destination,
            count,
        } => (Rotate(kind, true), reg(destination), 4, N(count), None),
        Operation::RotateThroughCarryCl { kind, destination } => {
            (Rotate(kind, true), reg(destination), 4, Cl, None)
        }
        Operation::RotateThroughCarryWord {
            kind,
            destination,
            count,
        } => (Rotate(kind, true), reg(destination), 2, count, None),
        Operation::ByteRotateThroughCarryOne { kind, destination } => {
            (Rotate(kind, true), byte(destination), 1, N(1), None)
        }
        Operation::ByteRotateThroughCarryImmediate {
            kind,
            destination,
            count,
        } => (Rotate(kind, true), byte(destination), 1, N(count), None),
        Operation::ByteRotateThroughCarryCl { kind, destination } => {
            (Rotate(kind, true), byte(destination), 1, Cl, None)
        }
        Operation::MemoryRotateThroughCarryOne { kind, address } => {
            (Rotate(kind, true), Operand::Memory(address), 4, N(1), None)
        }
        Operation::MemoryRotateThroughCarryImmediate {
            kind,
            address,
            count,
        } => (
            Rotate(kind, true),
            Operand::Memory(address),
            4,
            N(count),
            None,
        ),
        Operation::MemoryRotateThroughCarryCl { kind, address } => {
            (Rotate(kind, true), Operand::Memory(address), 4, Cl, None)
        }
        Operation::MemoryRotateThroughCarryWord {
            kind,
            address,
            count,
        } => (Rotate(kind, true), Operand::Memory(address), 2, count, None),
        Operation::MemoryByteRotateThroughCarryOne { kind, address } => {
            (Rotate(kind, true), Operand::Memory(address), 1, N(1), None)
        }
        Operation::MemoryByteRotateThroughCarryImmediate {
            kind,
            address,
            count,
        } => (
            Rotate(kind, true),
            Operand::Memory(address),
            1,
            N(count),
            None,
        ),
        Operation::MemoryByteRotateThroughCarryCl { kind, address } => {
            (Rotate(kind, true), Operand::Memory(address), 1, Cl, None)
        }
        Operation::DoubleShift {
            kind,
            destination,
            source,
            count,
        } => (Double(kind), reg(destination), 4, count, Some(reg(source))),
        Operation::DoubleShiftWord {
            kind,
            destination,
            source,
            count,
        } => (Double(kind), reg(destination), 2, count, Some(reg(source))),
        Operation::MemoryDoubleShift {
            kind,
            address,
            source,
            count,
        } => (
            Double(kind),
            Operand::Memory(address),
            4,
            count,
            Some(reg(source)),
        ),
        Operation::MemoryDoubleShiftWord {
            kind,
            address,
            source,
            count,
        } => (
            Double(kind),
            Operand::Memory(address),
            2,
            count,
            Some(reg(source)),
        ),
        _ => return None,
    };
    Some(Shift {
        kind,
        target,
        width,
        count,
        source,
    })
}

pub(super) fn execute(
    operation: Operation,
    state: &mut State32,
    port: &mut Port<'_>,
) -> Result<bool, Failure> {
    let Some(shift) = lower(operation) else {
        return Ok(false);
    };
    let old = read(shift.target, state, port, shift.width, true)?;
    let original_count = match shift.count {
        ShiftCount::Immediate(value) => u32::from(value),
        ShiftCount::Cl => state.registers[1],
    } & 31;
    let bits = shift.width * 8;
    let count = match shift.kind {
        Kind::Rotate(_, carry) if bits + u32::from(carry) < 32 => {
            original_count % (bits + u32::from(carry))
        }
        _ => original_count,
    };
    let source = match shift.source {
        Some(source) => read(source, state, port, shift.width, false)?,
        None => 0,
    };
    let active = if matches!(shift.kind, Kind::Rotate(_, false)) {
        original_count != 0
    } else {
        count != 0
    };
    let (result, flags) = if !active {
        (old, state.eflags)
    } else {
        match shift.kind {
            Kind::Shift(kind) => {
                let value = (match kind {
                    ShiftKind::Shl => old.wrapping_shl(count),
                    ShiftKind::Shr => old.wrapping_shr(count),
                    ShiftKind::Sar => (signed(old, shift.width) as i32).wrapping_shr(count) as u32,
                }) & mask(shift.width);
                (
                    value,
                    shift_flags(
                        state.eflags,
                        old,
                        value,
                        kind,
                        bits,
                        count,
                        original_count,
                        false,
                    ),
                )
            }
            Kind::Double(kind) => {
                let value = if count > bits {
                    0
                } else {
                    (match kind {
                        DoubleShiftKind::Left => {
                            old.wrapping_shl(count) | source.wrapping_shr(bits - count)
                        }
                        DoubleShiftKind::Right => {
                            old.wrapping_shr(count) | source.wrapping_shl(bits - count)
                        }
                    }) & mask(shift.width)
                };
                let kind = if kind == DoubleShiftKind::Left {
                    ShiftKind::Shl
                } else {
                    ShiftKind::Shr
                };
                (
                    value,
                    shift_flags(
                        state.eflags,
                        old,
                        value,
                        kind,
                        bits,
                        count,
                        original_count,
                        true,
                    ),
                )
            }
            Kind::Rotate(kind, carry) => {
                let value;
                let carry_out;
                if carry {
                    let ring_bits = bits + 1;
                    let ring = u64::from(old) | u64::from(state.eflags & 1) << bits;
                    let rotated = (match kind {
                        RotateKind::Left => (ring << count) | (ring >> (ring_bits - count)),
                        RotateKind::Right => (ring >> count) | (ring << (ring_bits - count)),
                    }) & ((1_u64 << ring_bits) - 1);
                    value = rotated as u32 & mask(shift.width);
                    carry_out = (rotated >> bits) as u32 & 1;
                } else {
                    value = (match kind {
                        RotateKind::Left => {
                            old.wrapping_shl(count) | old.wrapping_shr(bits - count)
                        }
                        RotateKind::Right => {
                            old.wrapping_shr(count) | old.wrapping_shl(bits - count)
                        }
                    }) & mask(shift.width);
                    carry_out = if kind == RotateKind::Left {
                        value & 1
                    } else {
                        value >> (bits - 1)
                    };
                }
                let overflow = if original_count != 1 {
                    0
                } else {
                    ((value >> (bits - 1))
                        ^ if kind == RotateKind::Left {
                            carry_out
                        } else {
                            value >> (bits - 2)
                        })
                        & 1
                };
                (value, (state.eflags & !0x801) | carry_out | overflow << 11)
            }
        }
    };
    // even a masked-zero count performs the checked memory write transaction.
    store(shift.target, state, port, shift.width, result)?;
    state.eflags = flags;
    Ok(true)
}

fn shift_flags(
    flags: u32,
    old: u32,
    result: u32,
    kind: ShiftKind,
    bits: u32,
    count: u32,
    original_count: u32,
    double: bool,
) -> u32 {
    let carry = if count < bits || double && count == bits {
        old.wrapping_shr(if kind == ShiftKind::Shl {
            bits - count
        } else {
            count - 1
        }) & 1
    } else if kind == ShiftKind::Sar {
        old >> (bits - 1)
    } else {
        0
    };
    let overflow = if original_count != 1 {
        0
    } else if double {
        (result ^ old) >> (bits - 1) & 1
    } else {
        (match kind {
            ShiftKind::Shl => (result >> (bits - 1)) ^ carry,
            ShiftKind::Shr => old >> (bits - 1),
            ShiftKind::Sar => 0,
        }) & 1
    };
    (flags & !0x8d5)
        | carry
        | (u32::from((result as u8).count_ones() & 1 == 0) << 2)
        | (u32::from(result == 0) << 6)
        | ((result >> (bits - 1)) << 7)
        | (overflow << 11)
}
