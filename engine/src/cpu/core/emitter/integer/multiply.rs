use super::super::{A, B, CARRY, CTX, FLAGS, KIND, Output, RESULT, WIDE, mem};
use super::{Operand, byte, location, mask, reg};
use crate::cpu::{
    core::StopReason,
    x86::{Register32, ir::*},
};
use wasm_encoder::BlockType;

enum Multiply {
    Accumulate(MultiplyKind, Operand, u32),
    Signed(Register32, Operand, Option<u32>, u32),
    Divide(DivideKind, Operand, u32),
}

fn lower(operation: Operation) -> Option<Multiply> {
    use Multiply as M;
    Some(match operation {
        Operation::SignedMultiply {
            destination,
            source,
            immediate,
        } => M::Signed(destination, location(source), immediate, 4),
        Operation::SignedMultiplyWord {
            destination,
            source,
        } => M::Signed(destination, reg(source), None, 2),
        Operation::ReadSignedMultiplyWord {
            destination,
            address,
        } => M::Signed(destination, Operand::Memory(address), None, 2),
        Operation::SignedMultiplyImmediateWord {
            destination,
            source,
            immediate,
        } => M::Signed(destination, reg(source), Some(u32::from(immediate)), 2),
        Operation::ReadSignedMultiplyImmediateWord {
            destination,
            address,
            immediate,
        } => M::Signed(
            destination,
            Operand::Memory(address),
            Some(u32::from(immediate)),
            2,
        ),
        Operation::MultiplyAccumulator { kind, source } => M::Accumulate(kind, reg(source), 4),
        Operation::WordMultiplyAccumulator { kind, source } => M::Accumulate(kind, reg(source), 2),
        Operation::ByteMultiplyAccumulator { kind, source } => M::Accumulate(kind, byte(source), 1),
        Operation::ReadMultiplyAccumulator { kind, address } => {
            M::Accumulate(kind, Operand::Memory(address), 4)
        }
        Operation::ReadWordMultiplyAccumulator { kind, address } => {
            M::Accumulate(kind, Operand::Memory(address), 2)
        }
        Operation::ReadByteMultiplyAccumulator { kind, address } => {
            M::Accumulate(kind, Operand::Memory(address), 1)
        }
        Operation::DivideAccumulator { kind, source } => M::Divide(kind, reg(source), 4),
        Operation::WordDivideAccumulator { kind, source } => M::Divide(kind, reg(source), 2),
        Operation::ByteDivideAccumulator { kind, source } => M::Divide(kind, byte(source), 1),
        Operation::ReadDivideAccumulator { kind, address } => {
            M::Divide(kind, Operand::Memory(address), 4)
        }
        Operation::ReadWordDivideAccumulator { kind, address } => {
            M::Divide(kind, Operand::Memory(address), 2)
        }
        Operation::ReadByteDivideAccumulator { kind, address } => {
            M::Divide(kind, Operand::Memory(address), 1)
        }
        _ => return None,
    })
}

pub(super) fn supports(operation: &Operation) -> bool {
    lower(*operation).is_some()
}

pub(super) fn emit(out: &mut Output<'_>, operation: Operation) -> bool {
    let Some(operation) = lower(operation) else {
        return false;
    };
    match operation {
        Multiply::Signed(destination, source, immediate, width) => {
            out.operand(source, width, B, false);
            if let Some(immediate) = immediate {
                out.c()
                    .i32_const((immediate & mask(width)) as i32)
                    .local_set(A);
            } else {
                out.operand(reg(destination), width, A, false);
            }
            product(out, MultiplyKind::Signed, width);
            out.c()
                .local_get(WIDE)
                .i32_wrap_i64()
                .i32_const(mask(width) as i32)
                .i32_and()
                .local_set(RESULT);
            overflow(out, MultiplyKind::Signed, width);
            out.store(reg(destination), width, RESULT);
            multiply_flags(out);
        }
        Multiply::Accumulate(kind, source, width) => {
            out.operand(source, width, B, false);
            out.operand(reg(Register32::Eax), width, A, false);
            product(out, kind, width);
            out.c()
                .local_get(WIDE)
                .i32_wrap_i64()
                .i32_const(mask(if width == 1 { 2 } else { width }) as i32)
                .i32_and()
                .local_set(RESULT);
            overflow(out, kind, width);
            out.store(
                reg(Register32::Eax),
                if width == 1 { 2 } else { width },
                RESULT,
            );
            if width != 1 {
                out.c()
                    .local_get(WIDE)
                    .i64_const(i64::from(width * 8))
                    .i64_shr_u()
                    .i32_wrap_i64()
                    .local_set(A);
                out.store(reg(Register32::Edx), width, A);
            }
            multiply_flags(out);
        }
        Multiply::Divide(kind, source, width) => divide(out, kind, source, width),
    }
    true
}

fn extend(out: &mut Output<'_>, local: u32, kind: MultiplyKind, width: u32) {
    out.c().local_get(local);
    if kind == MultiplyKind::Signed {
        if width < 4 {
            let shift = (32 - width * 8) as i32;
            out.c()
                .i32_const(shift)
                .i32_shl()
                .i32_const(shift)
                .i32_shr_s();
        }
        out.c().i64_extend_i32_s();
    } else {
        out.c().i64_extend_i32_u();
    }
}

fn product(out: &mut Output<'_>, kind: MultiplyKind, width: u32) {
    extend(out, A, kind, width);
    extend(out, B, kind, width);
    out.c().i64_mul().local_set(WIDE);
}

fn overflow(out: &mut Output<'_>, kind: MultiplyKind, width: u32) {
    out.c().local_get(WIDE);
    if kind == MultiplyKind::Unsigned {
        out.c().i64_const(i64::from(mask(width))).i64_gt_u();
    } else {
        out.c().local_get(WIDE).i32_wrap_i64();
        if width < 4 {
            let shift = (32 - width * 8) as i32;
            out.c()
                .i32_const(shift)
                .i32_shl()
                .i32_const(shift)
                .i32_shr_s();
        }
        out.c().i64_extend_i32_s().i64_ne();
    }
    out.c().local_set(CARRY);
}

fn multiply_flags(out: &mut Output<'_>) {
    out.materialize(super::super::super::flags::ARITHMETIC);
    out.c()
        .i32_const(0)
        .local_set(KIND)
        .local_get(FLAGS)
        .i32_const(!0x8d5)
        .i32_and()
        .local_get(CARRY)
        .i32_const(0x801)
        .i32_mul()
        .i32_or()
        .local_set(FLAGS);
}

fn numerator(out: &mut Output<'_>, kind: DivideKind, width: u32) {
    if width == 4 {
        out.c()
            .local_get(4)
            .i64_extend_i32_u()
            .i64_const(32)
            .i64_shl()
            .local_get(2)
            .i64_extend_i32_u()
            .i64_or();
    } else {
        if width == 1 {
            out.c().local_get(2).i32_const(0xffff).i32_and();
        } else {
            out.c()
                .local_get(4)
                .i32_const(16)
                .i32_shl()
                .local_get(2)
                .i32_const(0xffff)
                .i32_and()
                .i32_or();
        }
        if kind == DivideKind::Signed {
            if width == 1 {
                out.c().i32_const(16).i32_shl().i32_const(16).i32_shr_s();
            }
            out.c().i64_extend_i32_s();
        } else {
            out.c().i64_extend_i32_u();
        }
    }
}

fn denominator(out: &mut Output<'_>, kind: DivideKind, width: u32) {
    extend(
        out,
        B,
        if kind == DivideKind::Signed {
            MultiplyKind::Signed
        } else {
            MultiplyKind::Unsigned
        },
        width,
    );
}

fn divide(out: &mut Output<'_>, kind: DivideKind, source: Operand, width: u32) {
    out.operand(source, width, B, false);
    out.c().local_get(B).i32_eqz();
    error_if(out);
    numerator(out, kind, width);
    out.c().local_set(WIDE);
    if kind == DivideKind::Signed && width == 4 {
        out.c()
            .local_get(WIDE)
            .i64_const(i64::MIN)
            .i64_eq()
            .local_get(B)
            .i32_const(-1)
            .i32_eq()
            .i32_and();
        error_if(out);
    }
    out.c().local_get(WIDE);
    denominator(out, kind, width);
    if kind == DivideKind::Signed {
        out.c().i64_div_s();
    } else {
        out.c().i64_div_u();
    }
    out.c().local_set(WIDE);
    if kind == DivideKind::Signed {
        let limit = 1_i64 << (width * 8 - 1);
        out.c()
            .local_get(WIDE)
            .i64_const(-limit)
            .i64_lt_s()
            .local_get(WIDE)
            .i64_const(limit - 1)
            .i64_gt_s()
            .i32_or();
    } else {
        out.c()
            .local_get(WIDE)
            .i64_const(i64::from(mask(width)))
            .i64_gt_u();
    }
    error_if(out);
    out.c().local_get(WIDE).i32_wrap_i64().local_set(RESULT);
    numerator(out, kind, width);
    denominator(out, kind, width);
    if kind == DivideKind::Signed {
        out.c().i64_rem_s();
    } else {
        out.c().i64_rem_u();
    }
    out.c().i32_wrap_i64().local_set(CARRY);
    if width == 1 {
        out.c()
            .local_get(RESULT)
            .i32_const(0xff)
            .i32_and()
            .local_get(CARRY)
            .i32_const(0xff)
            .i32_and()
            .i32_const(8)
            .i32_shl()
            .i32_or()
            .local_set(RESULT);
        out.store(reg(Register32::Eax), 2, RESULT);
    } else {
        out.store(reg(Register32::Eax), width, RESULT);
        out.store(reg(Register32::Edx), width, CARRY);
    }
}

fn error_if(out: &mut Output<'_>) {
    out.c().if_(BlockType::Empty);
    for offset in [48, 52, 116] {
        out.c()
            .local_get(CTX)
            .i32_const(0)
            .i32_store(mem(offset, 2));
    }
    out.exit(StopReason::CpuFault, Some(out.pc), out.prefix);
    out.c().end();
}
