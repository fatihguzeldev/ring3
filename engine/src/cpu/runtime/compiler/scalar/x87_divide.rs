use super::x87_multiply::{HIGH, LOW, X, Y};
use super::*;
use crate::cpu::x86::{X87Binary, X87Format, X87Operation, X87State};
use std::mem::offset_of;

const NUMERATOR: u32 = 25;
const MEMORY_BITS: u32 = 39;
const PRODUCT_EXPONENT: u32 = 40;
const INEXACT: u32 = 41;
const INCREMENTED: u32 = 42;
const NUMERATOR_EXPONENT: u32 = 43;
const BITS64: u32 = 45;
const LEFT: u32 = 50;
const RIGHT: u32 = 51;
const RESULT: u32 = 53;

struct Divide {
    destination: u8,
    source: Option<u8>,
    address: Option<EffectiveAddress>,
    width: u32,
}

fn lower(operation: Operation) -> Option<Divide> {
    let Operation::X87 {
        operation, address, ..
    } = operation
    else {
        return None;
    };
    let (destination, source, width) = match operation {
        X87Operation::Binary {
            kind: X87Binary::Divide,
            destination,
            source,
            ..
        } if destination < 8 && source < 8 && address.is_none() => (destination, Some(source), 0),
        X87Operation::BinaryMemory {
            kind: X87Binary::Divide,
            format: format @ (X87Format::Float32 | X87Format::Float64),
        } if address.is_some() => (0, None, if format == X87Format::Float64 { 8 } else { 4 }),
        _ => return None,
    };
    Some(Divide {
        destination,
        source,
        address,
        width,
    })
}

fn reject_non_normal32(out: &mut Output) {
    out.c()
        .local_get(MEMORY_BITS)
        .i32_const(0x7f800000)
        .i32_and()
        .i32_const(0x7f800000)
        .i32_eq()
        .local_get(MEMORY_BITS)
        .i32_const(0x7fffffff)
        .i32_and()
        .i32_const(1)
        .i32_sub()
        .i32_const(0x7ffffe)
        .i32_le_u()
        .i32_or();
    x87::miss_if(out);
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

fn memory_operand(out: &mut Output, address: EffectiveAddress, width: u32) {
    out.address(address);
    if width == 4 {
        out.read(4, MEMORY_BITS);
        reject_non_normal32(out);
        out.c()
            .local_get(MEMORY_BITS)
            .f32_reinterpret_i32()
            .f64_promote_f32()
            .local_set(RIGHT);
        x87::raw_from_f32_bits(out, MEMORY_BITS);
    } else {
        memory::read_words(out, &[MEMORY_BITS, PRODUCT_EXPONENT], 1);
        out.c()
            .local_get(MEMORY_BITS)
            .i64_extend_i32_u()
            .local_get(PRODUCT_EXPONENT)
            .i64_extend_i32_u()
            .i64_const(32)
            .i64_shl()
            .i64_or()
            .local_set(BITS64);
        reject_non_normal64(out);
        out.c()
            .local_get(BITS64)
            .f64_reinterpret_i64()
            .local_set(RIGHT)
            .local_get(BITS64)
            .local_set(x87::RAW);
        x87::raw_from_f64_bits(out);
    }
}

fn compare_product(out: &mut Output) {
    // each input significand has its leading bit at 63. normalize the exact
    // product to a 64-bit leading limb and a nonnegative discarded tail.
    out.c()
        .local_get(PRODUCT_EXPONENT)
        .local_get(HIGH)
        .i64_const(63)
        .i64_shr_u()
        .i32_wrap_i64()
        .i32_add()
        .local_set(PRODUCT_EXPONENT)
        .local_get(HIGH)
        .i64_const(0)
        .i64_ge_s()
        .if_(BlockType::Empty)
        .local_get(HIGH)
        .i64_const(1)
        .i64_shl()
        .local_get(LOW)
        .i64_const(63)
        .i64_shr_u()
        .i64_or()
        .local_set(HIGH)
        .local_get(LOW)
        .i64_const(i64::MAX)
        .i64_and()
        .local_set(LOW)
        .end()
        .local_get(PRODUCT_EXPONENT)
        .local_get(NUMERATOR_EXPONENT)
        .i32_ne()
        .local_get(HIGH)
        .local_get(NUMERATOR)
        .i64_ne()
        .i32_or()
        .local_get(LOW)
        .i64_eqz()
        .i32_eqz()
        .i32_or()
        .local_set(INEXACT)
        .local_get(PRODUCT_EXPONENT)
        .local_get(NUMERATOR_EXPONENT)
        .i32_gt_s()
        .local_get(PRODUCT_EXPONENT)
        .local_get(NUMERATOR_EXPONENT)
        .i32_eq()
        .local_get(HIGH)
        .local_get(NUMERATOR)
        .i64_gt_u()
        .local_get(HIGH)
        .local_get(NUMERATOR)
        .i64_eq()
        .local_get(LOW)
        .i64_eqz()
        .i32_eqz()
        .i32_and()
        .i32_or()
        .i32_and()
        .i32_or()
        .local_set(INCREMENTED);
}

fn commit_status(out: &mut Output) {
    let status = MemArg {
        offset: offset_of!(X87State, status) as u64,
        align: 1,
        memory_index: 0,
    };
    out.c()
        .local_get(x87::STATE)
        .local_get(x87::STATE)
        .i32_load16_u(status)
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
        .i32_store16(status);
}

/// the caller entered x87 and proved control word 0x023f or 0x027f. it retains
/// the pop and instruction metadata effects.
pub(super) fn emit_pc53(out: &mut Output, operation: Operation) -> bool {
    let Some(Divide {
        destination,
        source,
        address,
        width,
    }) = lower(operation)
    else {
        return false;
    };
    x87::read_raw_f64(out, destination, LEFT);
    out.c()
        .local_get(x87::RAW)
        .local_set(NUMERATOR)
        .local_get(x87::TMP)
        .i32_const(0x7fff)
        .i32_and()
        .local_set(NUMERATOR_EXPONENT);
    if let Some(source) = source {
        x87::read_raw_f64(out, source, RIGHT);
    } else {
        memory_operand(out, address.expect("decoded memory division"), width);
    }
    out.c()
        .local_get(x87::RAW)
        .local_set(Y)
        .local_get(x87::TMP)
        .i32_const(0x7fff)
        .i32_and()
        .local_set(PRODUCT_EXPONENT)
        .local_get(RIGHT)
        .f64_const(0.0.into())
        .f64_eq();
    x87::miss_if(out);
    // the operands are exact binary64 values. this division performs the sole
    // guest pc53 rounding; no rounded floating product is used to infer flags.
    out.c()
        .local_get(LEFT)
        .local_get(RIGHT)
        .f64_div()
        .local_tee(RESULT)
        .i64_reinterpret_f64()
        .local_set(BITS64);
    reject_non_normal64(out);
    out.c()
        .local_get(NUMERATOR)
        .i64_eqz()
        .i32_eqz()
        .local_get(RESULT)
        .f64_const(0.0.into())
        .f64_eq()
        .i32_and();
    x87::miss_if(out);
    out.c()
        .local_get(NUMERATOR)
        .i64_eqz()
        .if_(BlockType::Empty)
        .i32_const(0)
        .local_set(INEXACT)
        .i32_const(0)
        .local_set(INCREMENTED)
        .else_()
        .local_get(BITS64)
        .local_set(x87::RAW);
    x87::raw_from_f64_bits(out);
    out.c()
        .local_get(x87::RAW)
        .local_set(X)
        .local_get(PRODUCT_EXPONENT)
        .local_get(x87::TMP)
        .i32_const(0x7fff)
        .i32_and()
        .i32_add()
        .i32_const(16383)
        .i32_sub()
        .local_set(PRODUCT_EXPONENT);
    x87_multiply::product(out);
    compare_product(out);
    // binary64 loses a precision bit below its normal range, while x87 pc53
    // keeps it. a quotient below minnormal must return to the exact service,
    // including a binary64 tie that rounds up to the minimum normal value.
    out.c()
        .local_get(RESULT)
        .f64_abs()
        .f64_const(f64::MIN_POSITIVE.into())
        .f64_eq()
        .local_get(INCREMENTED)
        .i32_and();
    x87::miss_if(out);
    out.c().end();
    x87::commit_raw_f64(out, destination, RESULT);
    commit_status(out);
    true
}
