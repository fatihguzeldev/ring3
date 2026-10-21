use super::super::Output;
use crate::cpu::x86::X87Binary;
use wasm_encoder::{BlockType, ValType};

const STATUS: u32 = 41;
const EXP: u32 = 44;
const TMP: u32 = 45;
const A: u32 = 48;
const B: u32 = 49;
const SUM: u32 = 50;
pub(super) const RESULT: u32 = 66;
const AH: u32 = 67;
const AL: u32 = 68;
const BH: u32 = 69;
const BL: u32 = 70;
const WORK: u32 = 71;
const ERROR: u32 = 72;
const LEFT: u32 = 73;
const RIGHT: u32 = 74;

pub(super) fn bound(out: &mut Output<'_>, value: u32, depth: u32) {
    out.c()
        .local_get(value)
        .i64_reinterpret_f64()
        .i64_const(52)
        .i64_shr_u()
        .i32_wrap_i64()
        .i32_const(2047)
        .i32_and()
        .i32_const(1023)
        .i32_sub()
        .local_tee(EXP)
        .i32_const(-900)
        .i32_lt_s()
        .local_get(EXP)
        .i32_const(900)
        .i32_gt_s()
        .i32_or()
        .local_get(value)
        .f64_const(0.0.into())
        .f64_ne()
        .i32_and()
        .br_if(depth);
}

pub(super) fn arithmetic(out: &mut Output<'_>, kind: X87Binary, depth: u32) {
    let reverse = matches!(kind, X87Binary::ReverseSubtract | X87Binary::ReverseDivide);
    let left = if reverse { B } else { A };
    let right = if reverse { A } else { B };
    out.c().local_get(left).local_get(right);
    match kind {
        X87Binary::Add => {
            out.c().f64_add();
        }
        X87Binary::Multiply => {
            out.c().f64_mul();
        }
        X87Binary::Subtract | X87Binary::ReverseSubtract => {
            out.c().f64_sub();
        }
        X87Binary::Divide | X87Binary::ReverseDivide => {
            out.c().f64_div();
        }
    }
    out.c().local_tee(SUM).local_set(RESULT);
    bound(out, RESULT, depth);
    match kind {
        X87Binary::Add | X87Binary::Subtract | X87Binary::ReverseSubtract => {
            out.c().local_get(left).local_set(LEFT).local_get(right);
            if !matches!(kind, X87Binary::Add) {
                out.c().f64_neg();
            }
            out.c().local_set(RIGHT);
            sum_error(out);
            result_flags(out);
        }
        X87Binary::Multiply => {
            out.c()
                .local_get(SUM)
                .f64_const(0.0.into())
                .f64_eq()
                .local_get(A)
                .f64_const(0.0.into())
                .f64_ne()
                .i32_and()
                .local_get(B)
                .f64_const(0.0.into())
                .f64_ne()
                .i32_and()
                .br_if(depth)
                .local_get(A)
                .local_set(LEFT)
                .local_get(B)
                .local_set(RIGHT);
            product_error(out);
            result_flags(out);
        }
        X87Binary::Divide | X87Binary::ReverseDivide => {
            out.c()
                .local_get(SUM)
                .f64_const(0.0.into())
                .f64_eq()
                .local_get(left)
                .f64_const(0.0.into())
                .f64_ne()
                .i32_and()
                .br_if(depth)
                .local_get(SUM)
                .f64_abs()
                .local_set(LEFT)
                .local_get(right)
                .f64_abs()
                .local_set(RIGHT)
                .local_get(LEFT)
                .local_get(RIGHT)
                .f64_mul()
                .local_set(SUM);
            bound(out, SUM, depth);
            product_error(out);
            out.c()
                .local_get(left)
                .f64_abs()
                .local_set(RIGHT)
                .local_get(SUM)
                .local_get(RIGHT)
                .f64_ne()
                .local_get(ERROR)
                .f64_const(0.0.into())
                .f64_ne()
                .i32_or()
                .local_set(TMP)
                .local_get(SUM)
                .local_get(RIGHT)
                .f64_eq()
                .if_(BlockType::Result(ValType::I32))
                .local_get(ERROR)
                .f64_const(0.0.into())
                .f64_gt()
                .else_()
                .local_get(SUM)
                .local_get(RIGHT)
                .f64_gt()
                .end()
                .local_set(EXP);
            flags(out);
        }
    }
}

fn sum_error(out: &mut Output<'_>) {
    out.c()
        .local_get(SUM)
        .local_get(LEFT)
        .f64_sub()
        .local_set(WORK)
        .local_get(SUM)
        .local_get(WORK)
        .f64_sub()
        .local_set(AH)
        .local_get(LEFT)
        .local_get(AH)
        .f64_sub()
        .local_get(RIGHT)
        .local_get(WORK)
        .f64_sub()
        .f64_add()
        .local_set(ERROR);
}

fn split(out: &mut Output<'_>, value: u32, high: u32, low: u32) {
    out.c()
        .local_get(value)
        .f64_const(134217729.0.into())
        .f64_mul()
        .local_set(WORK)
        .local_get(WORK)
        .local_get(WORK)
        .local_get(value)
        .f64_sub()
        .f64_sub()
        .local_set(high)
        .local_get(value)
        .local_get(high)
        .f64_sub()
        .local_set(low);
}

fn product_error(out: &mut Output<'_>) {
    // bounded exponents keep both the split and the complete 106-bit residual in the normal range.
    split(out, LEFT, AH, AL);
    split(out, RIGHT, BH, BL);
    out.c()
        .local_get(SUM)
        .local_get(AH)
        .local_get(BH)
        .f64_mul()
        .f64_sub()
        .local_set(WORK)
        .local_get(WORK)
        .local_get(AL)
        .local_get(BH)
        .f64_mul()
        .f64_sub()
        .local_set(WORK)
        .local_get(WORK)
        .local_get(AH)
        .local_get(BL)
        .f64_mul()
        .f64_sub()
        .local_set(WORK)
        .local_get(AL)
        .local_get(BL)
        .f64_mul()
        .local_get(WORK)
        .f64_sub()
        .local_set(ERROR);
}

fn result_flags(out: &mut Output<'_>) {
    out.c()
        .local_get(ERROR)
        .f64_const(0.0.into())
        .f64_ne()
        .local_tee(TMP)
        .local_get(ERROR)
        .i64_reinterpret_f64()
        .local_get(RESULT)
        .i64_reinterpret_f64()
        .i64_xor()
        .i64_const(63)
        .i64_shr_u()
        .i32_wrap_i64()
        .i32_and()
        .local_set(EXP);
    flags(out);
}

fn flags(out: &mut Output<'_>) {
    out.c()
        .local_get(STATUS)
        .i32_const(!0x200)
        .i32_and()
        .local_get(TMP)
        .i32_const(5)
        .i32_shl()
        .i32_or()
        .local_get(EXP)
        .i32_const(9)
        .i32_shl()
        .i32_or()
        .local_set(STATUS);
}
