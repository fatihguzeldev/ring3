use super::super::{A, B, CARRY, CARRY_IN, FLAGS, KIND, Output, RESULT, WIDE};
use super::{Operand, byte, location, mask, reg};
use crate::cpu::x86::ir::*;
use wasm_encoder::BlockType;

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
    let (kind, target, width, count, source) = match operation {
        Operation::Shift {
            kind,
            destination,
            count,
        } => (Kind::Shift(kind), location(destination), 4, count, None),
        Operation::ShiftWord {
            kind,
            destination,
            count,
        } => (Kind::Shift(kind), reg(destination), 2, count, None),
        Operation::ShiftByte {
            kind,
            destination,
            count,
        } => (Kind::Shift(kind), byte(destination), 1, count, None),
        Operation::MemoryShiftWord {
            kind,
            address,
            count,
        } => (Kind::Shift(kind), Operand::Memory(address), 2, count, None),
        Operation::MemoryShiftByte { kind, address } => (
            Kind::Shift(kind),
            Operand::Memory(address),
            1,
            ShiftCount::Immediate(1),
            None,
        ),
        Operation::MemoryShiftByteImmediate {
            kind,
            address,
            count,
        } => (
            Kind::Shift(kind),
            Operand::Memory(address),
            1,
            ShiftCount::Immediate(count),
            None,
        ),
        Operation::MemoryShiftByteCl { kind, address } => (
            Kind::Shift(kind),
            Operand::Memory(address),
            1,
            ShiftCount::Cl,
            None,
        ),
        Operation::Rotate {
            kind,
            destination,
            count,
        } => (Kind::Rotate(kind, false), reg(destination), 4, count, None),
        Operation::RotateOne { kind, destination } => (
            Kind::Rotate(kind, false),
            reg(destination),
            4,
            ShiftCount::Immediate(1),
            None,
        ),
        Operation::RotateWord {
            kind,
            destination,
            count,
        } => (Kind::Rotate(kind, false), reg(destination), 2, count, None),
        Operation::ByteRotateImmediate {
            kind,
            destination,
            count,
        } => (
            Kind::Rotate(kind, false),
            byte(destination),
            1,
            ShiftCount::Immediate(count),
            None,
        ),
        Operation::ByteRotateCl { kind, destination } => (
            Kind::Rotate(kind, false),
            byte(destination),
            1,
            ShiftCount::Cl,
            None,
        ),
        Operation::ByteRotateOne { kind, destination } => (
            Kind::Rotate(kind, false),
            byte(destination),
            1,
            ShiftCount::Immediate(1),
            None,
        ),
        Operation::MemoryRotate {
            kind,
            address,
            count,
        } => (
            Kind::Rotate(kind, false),
            Operand::Memory(address),
            4,
            count,
            None,
        ),
        Operation::MemoryRotateOne { kind, address } => (
            Kind::Rotate(kind, false),
            Operand::Memory(address),
            4,
            ShiftCount::Immediate(1),
            None,
        ),
        Operation::MemoryRotateWord {
            kind,
            address,
            count,
        } => (
            Kind::Rotate(kind, false),
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
            Kind::Rotate(kind, false),
            Operand::Memory(address),
            1,
            count,
            None,
        ),
        Operation::MemoryByteRotateOne { kind, address } => (
            Kind::Rotate(kind, false),
            Operand::Memory(address),
            1,
            ShiftCount::Immediate(1),
            None,
        ),
        Operation::RotateThroughCarryOne { kind, destination } => (
            Kind::Rotate(kind, true),
            reg(destination),
            4,
            ShiftCount::Immediate(1),
            None,
        ),
        Operation::RotateThroughCarryImmediate {
            kind,
            destination,
            count,
        } => (
            Kind::Rotate(kind, true),
            reg(destination),
            4,
            ShiftCount::Immediate(count),
            None,
        ),
        Operation::RotateThroughCarryCl { kind, destination } => (
            Kind::Rotate(kind, true),
            reg(destination),
            4,
            ShiftCount::Cl,
            None,
        ),
        Operation::RotateThroughCarryWord {
            kind,
            destination,
            count,
        } => (Kind::Rotate(kind, true), reg(destination), 2, count, None),
        Operation::ByteRotateThroughCarryOne { kind, destination } => (
            Kind::Rotate(kind, true),
            byte(destination),
            1,
            ShiftCount::Immediate(1),
            None,
        ),
        Operation::ByteRotateThroughCarryImmediate {
            kind,
            destination,
            count,
        } => (
            Kind::Rotate(kind, true),
            byte(destination),
            1,
            ShiftCount::Immediate(count),
            None,
        ),
        Operation::ByteRotateThroughCarryCl { kind, destination } => (
            Kind::Rotate(kind, true),
            byte(destination),
            1,
            ShiftCount::Cl,
            None,
        ),
        Operation::MemoryRotateThroughCarryOne { kind, address } => (
            Kind::Rotate(kind, true),
            Operand::Memory(address),
            4,
            ShiftCount::Immediate(1),
            None,
        ),
        Operation::MemoryRotateThroughCarryImmediate {
            kind,
            address,
            count,
        } => (
            Kind::Rotate(kind, true),
            Operand::Memory(address),
            4,
            ShiftCount::Immediate(count),
            None,
        ),
        Operation::MemoryRotateThroughCarryCl { kind, address } => (
            Kind::Rotate(kind, true),
            Operand::Memory(address),
            4,
            ShiftCount::Cl,
            None,
        ),
        Operation::MemoryRotateThroughCarryWord {
            kind,
            address,
            count,
        } => (
            Kind::Rotate(kind, true),
            Operand::Memory(address),
            2,
            count,
            None,
        ),
        Operation::MemoryByteRotateThroughCarryOne { kind, address } => (
            Kind::Rotate(kind, true),
            Operand::Memory(address),
            1,
            ShiftCount::Immediate(1),
            None,
        ),
        Operation::MemoryByteRotateThroughCarryImmediate {
            kind,
            address,
            count,
        } => (
            Kind::Rotate(kind, true),
            Operand::Memory(address),
            1,
            ShiftCount::Immediate(count),
            None,
        ),
        Operation::MemoryByteRotateThroughCarryCl { kind, address } => (
            Kind::Rotate(kind, true),
            Operand::Memory(address),
            1,
            ShiftCount::Cl,
            None,
        ),
        Operation::DoubleShift {
            kind,
            destination,
            source,
            count,
        } => (
            Kind::Double(kind),
            reg(destination),
            4,
            count,
            Some(reg(source)),
        ),
        Operation::DoubleShiftWord {
            kind,
            destination,
            source,
            count,
        } => (
            Kind::Double(kind),
            reg(destination),
            2,
            count,
            Some(reg(source)),
        ),
        Operation::MemoryDoubleShift {
            kind,
            address,
            source,
            count,
        } => (
            Kind::Double(kind),
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
            Kind::Double(kind),
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

pub(super) fn supports(operation: &Operation) -> bool {
    lower(*operation).is_some()
}

pub(super) fn emit(out: &mut Output<'_>, operation: Operation) -> bool {
    let Some(shift) = lower(operation) else {
        return false;
    };
    out.materialize(super::super::super::flags::ARITHMETIC);
    out.c().i32_const(0).local_set(KIND);
    out.operand(shift.target, shift.width, A, true);
    match shift.count {
        ShiftCount::Immediate(count) => {
            out.c().i32_const(i32::from(count));
        }
        ShiftCount::Cl => {
            out.c().local_get(3);
        }
    }
    out.c().i32_const(31).i32_and().local_tee(CARRY_IN);
    if let Kind::Rotate(_, carry) = shift.kind {
        let modulo = shift.width * 8 + u32::from(carry);
        if modulo < 32 {
            out.c().i32_const(modulo as i32).i32_rem_u();
        }
    }
    out.c().local_set(B);
    if let Some(source) = shift.source {
        out.operand(source, shift.width, CARRY, false);
    }
    let tested_count = if matches!(shift.kind, Kind::Rotate(_, false)) {
        CARRY_IN
    } else {
        B
    };
    out.c().local_get(tested_count).if_(BlockType::Empty);
    match shift.kind {
        Kind::Shift(kind) => shift_value(out, kind, shift.width),
        Kind::Rotate(kind, carry) => rotate_value(out, kind, carry, shift.width),
        Kind::Double(kind) => double_value(out, kind, shift.width),
    }
    out.store(shift.target, shift.width, RESULT);
    match shift.kind {
        Kind::Shift(kind) => shift_flags(out, kind, shift.width, false),
        Kind::Rotate(kind, carry) => rotate_flags(out, kind, carry, shift.width),
        Kind::Double(kind) => shift_flags(
            out,
            if matches!(kind, DoubleShiftKind::Left) {
                ShiftKind::Shl
            } else {
                ShiftKind::Shr
            },
            shift.width,
            true,
        ),
    }
    out.c().i32_const(0).local_set(KIND).else_();
    out.store(shift.target, shift.width, A);
    out.c().end();
    true
}

fn shift_value(out: &mut Output<'_>, kind: ShiftKind, width: u32) {
    out.c().local_get(A);
    if kind == ShiftKind::Sar && width < 4 {
        let shift = (32 - width * 8) as i32;
        out.c()
            .i32_const(shift)
            .i32_shl()
            .i32_const(shift)
            .i32_shr_s();
    }
    out.c().local_get(B);
    match kind {
        ShiftKind::Shl => {
            out.c().i32_shl();
        }
        ShiftKind::Shr => {
            out.c().i32_shr_u();
        }
        ShiftKind::Sar => {
            out.c().i32_shr_s();
        }
    }
    out.c()
        .i32_const(mask(width) as i32)
        .i32_and()
        .local_set(RESULT);
}

fn double_value(out: &mut Output<'_>, kind: DoubleShiftKind, width: u32) {
    let bits = (width * 8) as i32;
    out.c()
        .local_get(B)
        .i32_const(bits)
        .i32_le_u()
        .if_(BlockType::Result(wasm_encoder::ValType::I32));
    out.c().local_get(A).local_get(B);
    match kind {
        DoubleShiftKind::Left => {
            out.c().i32_shl();
        }
        DoubleShiftKind::Right => {
            out.c().i32_shr_u();
        }
    }
    out.c()
        .local_get(CARRY)
        .i32_const(bits)
        .local_get(B)
        .i32_sub();
    match kind {
        DoubleShiftKind::Left => {
            out.c().i32_shr_u();
        }
        DoubleShiftKind::Right => {
            out.c().i32_shl();
        }
    }
    out.c()
        .i32_or()
        .i32_const(mask(width) as i32)
        .i32_and()
        .else_()
        .i32_const(0)
        .end()
        .local_set(RESULT);
}

fn rotate_value(out: &mut Output<'_>, kind: RotateKind, carry: bool, width: u32) {
    let bits = width * 8;
    if carry {
        out.c()
            .local_get(A)
            .i64_extend_i32_u()
            .local_get(FLAGS)
            .i32_const(1)
            .i32_and()
            .i64_extend_i32_u()
            .i64_const(i64::from(bits))
            .i64_shl()
            .i64_or()
            .local_set(WIDE);
        out.c().local_get(WIDE).local_get(B).i64_extend_i32_u();
        match kind {
            RotateKind::Left => {
                out.c().i64_shl();
            }
            RotateKind::Right => {
                out.c().i64_shr_u();
            }
        }
        out.c()
            .local_get(WIDE)
            .i64_const(i64::from(bits + 1))
            .local_get(B)
            .i64_extend_i32_u()
            .i64_sub();
        match kind {
            RotateKind::Left => {
                out.c().i64_shr_u();
            }
            RotateKind::Right => {
                out.c().i64_shl();
            }
        }
        out.c()
            .i64_or()
            .i64_const(((1_u64 << (bits + 1)) - 1) as i64)
            .i64_and()
            .local_set(WIDE)
            .local_get(WIDE)
            .i32_wrap_i64()
            .i32_const(mask(width) as i32)
            .i32_and()
            .local_set(RESULT);
    } else {
        out.c().local_get(A).local_get(B);
        match kind {
            RotateKind::Left => {
                out.c().i32_shl();
            }
            RotateKind::Right => {
                out.c().i32_shr_u();
            }
        }
        out.c()
            .local_get(A)
            .i32_const(bits as i32)
            .local_get(B)
            .i32_sub();
        match kind {
            RotateKind::Left => {
                out.c().i32_shr_u();
            }
            RotateKind::Right => {
                out.c().i32_shl();
            }
        }
        out.c()
            .i32_or()
            .i32_const(mask(width) as i32)
            .i32_and()
            .local_set(RESULT);
    }
}

fn shift_flags(out: &mut Output<'_>, kind: ShiftKind, width: u32, double: bool) {
    let bits = (width * 8) as i32;
    out.c().local_get(B).i32_const(bits);
    if double {
        out.c().i32_le_u();
    } else {
        out.c().i32_lt_u();
    }
    out.c()
        .if_(BlockType::Result(wasm_encoder::ValType::I32))
        .local_get(A);
    if kind == ShiftKind::Shl {
        out.c().i32_const(bits).local_get(B).i32_sub();
    } else {
        out.c().local_get(B).i32_const(1).i32_sub();
    }
    out.c().i32_shr_u().i32_const(1).i32_and().else_();
    if kind == ShiftKind::Sar {
        out.c().local_get(A).i32_const(bits - 1).i32_shr_u();
    } else {
        out.c().i32_const(0);
    }
    out.c().end().local_set(CARRY);
    out.c()
        .local_get(FLAGS)
        .i32_const(!0x8d5)
        .i32_and()
        .local_get(CARRY)
        .i32_or()
        .local_get(RESULT)
        .i32_const(0xff)
        .i32_and()
        .i32_popcnt()
        .i32_const(1)
        .i32_and()
        .i32_eqz()
        .i32_const(2)
        .i32_shl()
        .i32_or()
        .local_get(RESULT)
        .i32_eqz()
        .i32_const(6)
        .i32_shl()
        .i32_or()
        .local_get(RESULT)
        .i32_const(bits - 1)
        .i32_shr_u()
        .i32_const(7)
        .i32_shl()
        .i32_or()
        .local_set(FLAGS);
    out.c()
        .local_get(CARRY_IN)
        .i32_const(1)
        .i32_eq()
        .if_(BlockType::Empty);
    if double {
        out.c()
            .local_get(RESULT)
            .local_get(A)
            .i32_xor()
            .i32_const(bits - 1)
            .i32_shr_u();
    } else {
        match kind {
            ShiftKind::Shl => {
                out.c()
                    .local_get(RESULT)
                    .i32_const(bits - 1)
                    .i32_shr_u()
                    .local_get(CARRY)
                    .i32_xor();
            }
            ShiftKind::Shr => {
                out.c().local_get(A).i32_const(bits - 1).i32_shr_u();
            }
            ShiftKind::Sar => {
                out.c().i32_const(0);
            }
        }
    }
    out.c()
        .i32_const(1)
        .i32_and()
        .i32_const(11)
        .i32_shl()
        .local_get(FLAGS)
        .i32_or()
        .local_set(FLAGS)
        .end();
}

fn rotate_flags(out: &mut Output<'_>, kind: RotateKind, carry: bool, width: u32) {
    let bits = (width * 8) as i32;
    if carry {
        out.c()
            .local_get(WIDE)
            .i64_const(i64::from(bits))
            .i64_shr_u()
            .i32_wrap_i64();
    } else {
        out.c().local_get(RESULT);
        if kind == RotateKind::Right {
            out.c().i32_const(bits - 1).i32_shr_u();
        }
    }
    out.c()
        .i32_const(1)
        .i32_and()
        .local_set(CARRY)
        .local_get(FLAGS)
        .i32_const(!0x801)
        .i32_and()
        .local_get(CARRY)
        .i32_or()
        .local_set(FLAGS);
    out.c()
        .local_get(CARRY_IN)
        .i32_const(1)
        .i32_eq()
        .if_(BlockType::Empty);
    out.c().local_get(RESULT).i32_const(bits - 1).i32_shr_u();
    if kind == RotateKind::Left {
        out.c().local_get(CARRY);
    } else {
        out.c().local_get(RESULT).i32_const(bits - 2).i32_shr_u();
    }
    out.c()
        .i32_xor()
        .i32_const(1)
        .i32_and()
        .i32_const(11)
        .i32_shl()
        .local_get(FLAGS)
        .i32_or()
        .local_set(FLAGS)
        .end();
}
