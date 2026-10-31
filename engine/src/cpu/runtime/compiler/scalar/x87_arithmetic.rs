use super::*;
use crate::cpu::x86::{X87Binary, X87Format, X87Operation, X87State};
use std::mem::offset_of;

const MEMORY_BITS: u32 = 39;
const RESULT_BITS: u32 = 40;
const INEXACT: u32 = 41;
const INCREMENTED: u32 = 42;
const CONTROL: u32 = 43;
const BITS64: u32 = 45;
const NARROW: u32 = 47;
const LEFT: u32 = 50;
const RIGHT: u32 = 51;
const WIDE: u32 = 52;
const RESULT: u32 = 53;
const VIRTUAL_RIGHT: u32 = 54;
const ERROR: u32 = 55;
const RESIDUAL: u32 = 56;

struct Arithmetic {
    kind: X87Binary,
    destination: u8,
    source: Option<u8>,
    pop: bool,
    address: Option<EffectiveAddress>,
    memory_width: u32,
    waiting: bool,
    opcode: u16,
}

fn lower(operation: Operation) -> Option<Arithmetic> {
    let Operation::X87 {
        operation,
        address,
        waiting,
        opcode,
    } = operation
    else {
        return None;
    };
    let waiting = waiting || !operation.ignores_pending_exception();
    let (kind, destination, source, pop, memory_width) = match operation {
        X87Operation::Binary {
            kind,
            destination,
            source,
            pop,
        } if destination < 8 && source < 8 && address.is_none() => {
            (kind, destination, Some(source), pop, 0)
        }
        X87Operation::BinaryMemory {
            kind,
            format: format @ (X87Format::Float32 | X87Format::Float64),
        } if address.is_some() => (
            kind,
            0,
            None,
            false,
            if format == X87Format::Float64 { 8 } else { 4 },
        ),
        _ => return None,
    };
    if !matches!(
        kind,
        X87Binary::Add | X87Binary::Multiply | X87Binary::Divide
    ) {
        return None;
    }
    if memory_width == 8 && kind != X87Binary::Add {
        return None;
    }
    Some(Arithmetic {
        kind,
        destination,
        source,
        pop,
        address,
        memory_width,
        waiting,
        opcode,
    })
}

pub(super) fn supports(operation: &Operation) -> bool {
    lower(*operation).is_some()
}

fn reject_non_normal(out: &mut Output, bits: u32) {
    out.c()
        .local_get(bits)
        .i32_const(0x7f800000)
        .i32_and()
        .i32_const(0x7f800000)
        .i32_eq()
        .local_get(bits)
        .i32_const(0x7fffffff)
        .i32_and()
        .i32_const(1)
        .i32_sub()
        .i32_const(0x7ffffe)
        .i32_le_u()
        .i32_or();
    x87::miss_if(out);
}

fn emit_pc24(
    out: &mut Output,
    kind: X87Binary,
    destination: u8,
    source: Option<u8>,
    address: Option<EffectiveAddress>,
) {
    x87::read_raw_f32(out, destination, LEFT);
    if let Some(source) = source {
        x87::read_raw_f32(out, source, RIGHT);
    } else {
        out.address(address.expect("decoded memory arithmetic"));
        out.read(4, MEMORY_BITS);
        reject_non_normal(out, MEMORY_BITS);
        out.c()
            .local_get(MEMORY_BITS)
            .f32_reinterpret_i32()
            .f64_promote_f32()
            .local_set(RIGHT);
    }
    if kind == X87Binary::Divide {
        out.c().local_get(RIGHT).f64_const(0.0.into()).f64_eq();
        x87::miss_if(out);
    }
    if kind != X87Binary::Divide {
        out.c().local_get(LEFT).local_get(RIGHT);
        if kind == X87Binary::Add {
            out.c().f64_add();
        } else {
            out.c().f64_mul();
        }
        out.c().local_set(WIDE);
    }
    if kind == X87Binary::Add {
        // error-free TwoSum also detects addends lost by binary64 itself.
        // binary32 inputs cannot overflow or underflow any binary64 intermediate.
        out.c()
            .local_get(WIDE)
            .local_get(LEFT)
            .f64_sub()
            .local_set(VIRTUAL_RIGHT)
            .local_get(LEFT)
            .local_get(WIDE)
            .local_get(VIRTUAL_RIGHT)
            .f64_sub()
            .f64_sub()
            .local_get(RIGHT)
            .local_get(VIRTUAL_RIGHT)
            .f64_sub()
            .f64_add()
            .local_set(ERROR);
    } else if kind == X87Binary::Multiply {
        // x87 keeps24 bits even below the binary32 normal range. Do not admit a
        // tiny exact product merely because binary32 rounds it up to minnormal.
        out.c()
            .local_get(WIDE)
            .f64_abs()
            .f64_const(f64::from(f32::MIN_POSITIVE).into())
            .f64_lt()
            .local_get(WIDE)
            .f64_const(0.0.into())
            .f64_ne()
            .i32_and();
        x87::miss_if(out);
    } else {
        // This product is exact (a binary32 value times a power of two).
        out.c()
            .local_get(LEFT)
            .f64_abs()
            .local_get(RIGHT)
            .f64_abs()
            .f64_const(f64::from(f32::MIN_POSITIVE).into())
            .f64_mul()
            .f64_lt()
            .local_get(LEFT)
            .f64_const(0.0.into())
            .f64_ne()
            .i32_and();
        x87::miss_if(out);
    }
    // Both demotions are exact. Perform the one guest rounding in binary32,
    // rather than double-rounding an intermediate binary64 quotient or sum.
    out.c()
        .local_get(LEFT)
        .f32_demote_f64()
        .local_get(RIGHT)
        .f32_demote_f64();
    match kind {
        X87Binary::Add => {
            out.c().f32_add();
        }
        X87Binary::Multiply => {
            out.c().f32_mul();
        }
        X87Binary::Divide => {
            out.c().f32_div();
        }
        _ => unreachable!(),
    }
    out.c()
        .local_tee(NARROW)
        .f64_promote_f32()
        .local_set(RESULT)
        .local_get(NARROW)
        .i32_reinterpret_f32()
        .local_set(RESULT_BITS);
    reject_non_normal(out, RESULT_BITS);
    if kind == X87Binary::Add {
        // TwoSum gives exact sum WIDE+ERROR. WIDE-RESULT is exact by Sterbenz;
        // if nonzero its magnitude exceeds ERROR, so residual sign/nonzero survive.
        out.c()
            .local_get(WIDE)
            .local_get(RESULT)
            .f64_sub()
            .local_get(ERROR)
            .f64_add()
            .local_set(RESIDUAL)
            .local_get(RESIDUAL)
            .f64_const(0.0.into())
            .f64_ne()
            .local_set(INEXACT)
            .local_get(RESULT)
            .f64_const(0.0.into())
            .f64_lt()
            .if_(BlockType::Result(ValType::I32))
            .local_get(RESIDUAL)
            .f64_const(0.0.into())
            .f64_gt()
            .else_()
            .local_get(RESIDUAL)
            .f64_const(0.0.into())
            .f64_lt()
            .end()
            .local_set(INCREMENTED);
    } else if kind == X87Binary::Multiply {
        // The product has at most48 significant bits, exactly represented in f64.
        out.c()
            .local_get(RESULT)
            .local_get(WIDE)
            .f64_ne()
            .local_set(INEXACT)
            .local_get(RESULT)
            .f64_abs()
            .local_get(WIDE)
            .f64_abs()
            .f64_gt()
            .local_set(INCREMENTED);
    } else {
        // Both factors have24 bits. Compare the exact product with the numerator
        // to recover division's inexactness and magnitude increment for either sign.
        out.c()
            .local_get(RESULT)
            .f64_abs()
            .local_get(RIGHT)
            .f64_abs()
            .f64_mul()
            .local_set(RESIDUAL)
            .local_get(RESIDUAL)
            .local_get(LEFT)
            .f64_abs()
            .f64_ne()
            .local_set(INEXACT)
            .local_get(RESIDUAL)
            .local_get(LEFT)
            .f64_abs()
            .f64_gt()
            .local_set(INCREMENTED);
    }
    x87::commit_raw_f32(out, destination, RESULT);
    commit_status(out);
}

fn commit_status(out: &mut Output) {
    out.c()
        .local_get(x87::STATE)
        .local_get(x87::STATE)
        .i32_load16_u(MemArg {
            offset: offset_of!(X87State, status) as u64,
            align: 1,
            memory_index: 0,
        })
        .i32_const(!0x200)
        .i32_and()
        .local_get(INEXACT)
        .i32_const(5)
        .i32_shl()
        .i32_or()
        .local_get(INCREMENTED)
        .i32_const(9)
        .i32_shl()
        .i32_or()
        .i32_store16(MemArg {
            offset: offset_of!(X87State, status) as u64,
            align: 1,
            memory_index: 0,
        });
}

fn reject_non_normal64(out: &mut Output) {
    out.c()
        .local_get(BITS64)
        .i64_const(0x7ff0_0000_0000_0000)
        .i64_and()
        .i64_const(0x7ff0_0000_0000_0000)
        .i64_eq()
        .local_get(BITS64)
        .i64_const(0x7fff_ffff_ffff_ffff)
        .i64_and()
        .i64_const(1)
        .i64_sub()
        .i64_const(0x000f_ffff_ffff_fffe)
        .i64_le_u()
        .i32_or();
    x87::miss_if(out);
}

fn emit_pc53_add(
    out: &mut Output,
    destination: u8,
    source: Option<u8>,
    address: Option<EffectiveAddress>,
    memory_width: u32,
) {
    x87::read_raw_f64(out, destination, LEFT);
    if let Some(source) = source {
        x87::read_raw_f64(out, source, RIGHT);
    } else {
        out.address(address.expect("decoded memory addition"));
        if memory_width == 4 {
            out.read(4, MEMORY_BITS);
            reject_non_normal(out, MEMORY_BITS);
            out.c()
                .local_get(MEMORY_BITS)
                .f32_reinterpret_i32()
                .f64_promote_f32()
                .local_set(RIGHT);
        } else {
            memory::read_words(out, &[MEMORY_BITS, RESULT_BITS], 1);
            out.c()
                .local_get(MEMORY_BITS)
                .i64_extend_i32_u()
                .local_get(RESULT_BITS)
                .i64_extend_i32_u()
                .i64_const(32)
                .i64_shl()
                .i64_or()
                .local_set(BITS64);
            reject_non_normal64(out);
            out.c()
                .local_get(BITS64)
                .f64_reinterpret_i64()
                .local_set(RIGHT);
        }
    }
    out.c()
        .local_get(LEFT)
        .local_get(RIGHT)
        .f64_add()
        .local_tee(RESULT)
        .i64_reinterpret_f64()
        .local_set(BITS64);
    reject_non_normal64(out);
    // all finite binary64 operands lie on the 2^-1074 grid. TwoSum therefore
    // retains even a subnormal residual exactly; a nonzero subnormal sum rejects
    // above, and zero can only be exact cancellation or signed zero.
    out.c()
        .local_get(RESULT)
        .local_get(LEFT)
        .f64_sub()
        .local_set(VIRTUAL_RIGHT)
        .local_get(LEFT)
        .local_get(RESULT)
        .local_get(VIRTUAL_RIGHT)
        .f64_sub()
        .f64_sub()
        .local_get(RIGHT)
        .local_get(VIRTUAL_RIGHT)
        .f64_sub()
        .f64_add()
        .local_set(ERROR)
        .local_get(ERROR)
        .f64_const(0.0.into())
        .f64_ne()
        .local_set(INEXACT)
        .local_get(RESULT)
        .f64_const(0.0.into())
        .f64_lt()
        .if_(BlockType::Result(ValType::I32))
        .local_get(ERROR)
        .f64_const(0.0.into())
        .f64_gt()
        .else_()
        .local_get(ERROR)
        .f64_const(0.0.into())
        .f64_lt()
        .end()
        .local_set(INCREMENTED);
    // these checks state the exact lower-range boundary explicitly. They are
    // unreachable for sums of admitted binary64 values on the common grid.
    out.c()
        .local_get(RESULT)
        .f64_abs()
        .f64_const(f64::MIN_POSITIVE.into())
        .f64_eq()
        .local_get(INCREMENTED)
        .i32_and()
        .local_get(RESULT)
        .f64_const(0.0.into())
        .f64_eq()
        .local_get(INEXACT)
        .i32_and()
        .i32_or();
    x87::miss_if(out);
    x87::commit_raw_f64(out, destination, RESULT);
    commit_status(out);
}

pub(super) fn emit(out: &mut Output, operation: Operation) -> bool {
    let Some(Arithmetic {
        kind,
        destination,
        source,
        pop,
        address,
        memory_width,
        waiting,
        opcode,
    }) = lower(operation)
    else {
        return false;
    };
    x87::enter(out, waiting);
    out.c()
        .local_get(x87::STATE)
        .i32_load16_u(MemArg {
            offset: offset_of!(X87State, control) as u64,
            align: 1,
            memory_index: 0,
        })
        .i32_const(!0x40)
        .i32_and()
        .local_tee(CONTROL)
        .i32_const(0x023f)
        .i32_eq()
        .if_(BlockType::Empty);
    if kind == X87Binary::Add {
        emit_pc53_add(out, destination, source, address, memory_width);
    } else {
        out.exit(7);
    }
    out.c()
        .else_()
        .local_get(CONTROL)
        .i32_const(0x003f)
        .i32_ne();
    x87::miss_if(out);
    if memory_width == 8 {
        out.exit(7);
    } else {
        emit_pc24(out, kind, destination, source, address);
    }
    out.c().end();
    if pop {
        x87::pop(out);
    }
    x87::metadata(out, opcode, address.is_some());
    true
}
