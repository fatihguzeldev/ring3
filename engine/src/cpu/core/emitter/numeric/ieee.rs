use super::super::Output;
use crate::cpu::x86::X87Format;
use wasm_encoder::{BlockType, ValType};

const STATUS: u32 = 41;
const ZERO: u32 = 43;
const EXPONENT: u32 = 44;
const TEMPORARY: u32 = 45;
const SIGNIFICAND: u32 = 46;
const PACKED: u32 = 47;

pub(super) fn pack(out: &mut Output<'_>, low: u32, high: u32, format: X87Format) {
    pack_at(out, low, high, format, 0);
}

/// Pack into the caller's failure block. The caller may surround the pack with
/// an additional precision-mode branch without changing its failure target.
pub(super) fn pack_at(out: &mut Output<'_>, low: u32, high: u32, format: X87Format, depth: u32) {
    let (precision, bias, maximum) = match format {
        X87Format::Float32 => (24_u32, 127_u32, 255_u32),
        X87Format::Float64 => (53, 1023, 2047),
        _ => unreachable!(),
    };
    let shift = 64 - precision;
    let discarded = (1_u64 << shift) - 1;
    out.c()
        .local_get(low)
        .local_set(SIGNIFICAND)
        .local_get(high)
        .i32_const(0x7fff)
        .i32_and()
        .local_set(EXPONENT)
        .local_get(SIGNIFICAND)
        .i64_eqz()
        .local_get(EXPONENT)
        .i32_eqz()
        .i32_and()
        .local_tee(ZERO)
        .local_get(EXPONENT)
        .i32_const(1)
        .i32_sub()
        .i32_const(0x7ffe)
        .i32_lt_u()
        .local_get(SIGNIFICAND)
        .i64_const(0)
        .i64_lt_s()
        .i32_and()
        .i32_or()
        .i32_eqz()
        .br_if(depth)
        .local_get(EXPONENT)
        .i32_const((16383 - bias) as i32)
        .i32_sub()
        .local_set(EXPONENT);
    normal_output(out, maximum, depth);
    out.c()
        .local_get(SIGNIFICAND)
        .i64_const(i64::from(shift))
        .i64_shr_u()
        .local_set(PACKED)
        .local_get(STATUS)
        .i32_const(!0x200)
        .i32_and()
        .local_set(STATUS)
        .local_get(SIGNIFICAND)
        .i64_const(discarded as i64)
        .i64_and()
        .i64_eqz()
        .i32_eqz()
        .if_(BlockType::Empty)
        .local_get(STATUS)
        .i32_const(0x20)
        .i32_or()
        .local_set(STATUS)
        .local_get(super::span::CONTROL)
        .i32_const(10)
        .i32_shr_u()
        .i32_const(3)
        .i32_and()
        .local_tee(TEMPORARY)
        .i32_eqz()
        .if_(BlockType::Result(ValType::I32));
    out.c()
        .local_get(SIGNIFICAND)
        .i64_const(discarded as i64)
        .i64_and()
        .i64_const((1_u64 << (shift - 1)) as i64)
        .i64_gt_u()
        .local_get(SIGNIFICAND)
        .i64_const(discarded as i64)
        .i64_and()
        .i64_const((1_u64 << (shift - 1)) as i64)
        .i64_eq()
        .local_get(PACKED)
        .i32_wrap_i64()
        .i32_const(1)
        .i32_and()
        .i32_and()
        .i32_or()
        .else_()
        .local_get(TEMPORARY)
        .i32_const(1)
        .i32_eq()
        .if_(BlockType::Result(ValType::I32));
    sign(out, high);
    out.c()
        .else_()
        .local_get(TEMPORARY)
        .i32_const(2)
        .i32_eq()
        .if_(BlockType::Result(ValType::I32));
    sign(out, high);
    out.c()
        .i32_eqz()
        .else_()
        .i32_const(0)
        .end()
        .end()
        .end()
        .local_set(TEMPORARY)
        .local_get(STATUS)
        .local_get(TEMPORARY)
        .i32_const(9)
        .i32_shl()
        .i32_or()
        .local_set(STATUS)
        .local_get(PACKED)
        .local_get(TEMPORARY)
        .i64_extend_i32_u()
        .i64_add()
        .local_tee(PACKED)
        .i64_const(i64::from(precision))
        .i64_shr_u()
        .i32_wrap_i64()
        .local_set(TEMPORARY)
        .local_get(EXPONENT)
        .local_get(TEMPORARY)
        .i32_add()
        .local_set(EXPONENT)
        .local_get(PACKED)
        .local_get(TEMPORARY)
        .i64_extend_i32_u()
        .i64_shr_u()
        .local_set(PACKED);
    normal_output(out, maximum, depth + 1);
    out.c()
        .end()
        .i64_const(0)
        .local_get(PACKED)
        .i64_const(((1_u64 << (precision - 1)) - 1) as i64)
        .i64_and()
        .local_get(EXPONENT)
        .i64_extend_i32_u()
        .i64_const(i64::from(precision - 1))
        .i64_shl()
        .i64_or()
        .local_get(ZERO)
        .select()
        .local_get(high)
        .i32_const(0x8000)
        .i32_and()
        .i64_extend_i32_u()
        .i64_const(if precision == 24 { 16 } else { 48 })
        .i64_shl()
        .i64_or()
        .local_set(PACKED);
}

fn normal_output(out: &mut Output<'_>, maximum: u32, depth: u32) {
    out.c()
        .local_get(EXPONENT)
        .i32_const(1)
        .i32_sub()
        .i32_const((maximum - 1) as i32)
        .i32_ge_u()
        .local_get(ZERO)
        .i32_eqz()
        .i32_and()
        .br_if(depth);
}

fn sign(out: &mut Output<'_>, high: u32) {
    out.c().local_get(high).i32_const(15).i32_shr_u();
}
