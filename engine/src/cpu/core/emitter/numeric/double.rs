use super::{super::Output, span};
use crate::cpu::x86::X87Binary;
use wasm_encoder::{BlockType, Function, InstructionSink, ValType};

const STATUS: u32 = 41;
const EXP: u32 = 44;
const A: u32 = 48;
const B: u32 = 49;
const SUM: u32 = 50;
pub(super) const RESULT: u32 = 66;
const LEFT: u32 = 73;
const RIGHT: u32 = 74;

pub(super) const PENDING: u32 = 100;
pub(super) const PENDING_LEFT: u32 = 101;
pub(super) const PENDING_RIGHT: u32 = 102;
pub(super) const PENDING_RESULT: u32 = 103;

#[derive(Clone, Copy)]
struct Residual {
    status: u32,
    exp: u32,
    tmp: u32,
    sum: u32,
    ah: u32,
    al: u32,
    bh: u32,
    bl: u32,
    work: u32,
    error: u32,
    left: u32,
    right: u32,
    result: u32,
}

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
    let exact_product = out.fp_schedule.at(out.before_frame.unwrap()).exact_mul;
    arithmetic_plan(out, kind, depth, exact_product);
}

pub(super) fn arithmetic_plan(
    out: &mut Output<'_>,
    kind: X87Binary,
    depth: u32,
    exact_product: bool,
) {
    if kind == X87Binary::Multiply && exact_product {
        // Two admitted native24 significands have at most 48 product bits. Their
        // finite binary32 exponent range fits normal binary64, including signed
        // zero. This arm runs only under the actual PC53 control selection.
        out.c()
            .local_get(A)
            .local_get(B)
            .f64_mul()
            .local_set(RESULT)
            .local_get(STATUS)
            .i32_const(!0x200)
            .i32_and()
            .local_set(STATUS);
        span::clear_status_recipe(out);
        return;
    }
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
    let recipe = match kind {
        X87Binary::Add | X87Binary::Subtract | X87Binary::ReverseSubtract => {
            out.c().local_get(left).local_set(LEFT).local_get(right);
            if !matches!(kind, X87Binary::Add) {
                out.c().f64_neg();
            }
            out.c().local_set(RIGHT);
            1
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
                .br_if(depth);
            2
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
                .local_get(right)
                .f64_abs()
                .f64_mul()
                .local_set(SUM);
            // This remains an execution-time admission guard. Resolution cannot fall back.
            bound(out, SUM, depth);
            3
        }
    };
    let left = if recipe == 1 { LEFT } else { left };
    let right = if recipe == 1 { RIGHT } else { right };
    out.c()
        .local_get(STATUS)
        .i32_const(0x20)
        .i32_and()
        .if_(BlockType::Empty);
    for (source, destination) in [
        (left, PENDING_LEFT),
        (right, PENDING_RIGHT),
        (RESULT, PENDING_RESULT),
    ] {
        out.c().local_get(source).local_set(destination);
    }
    out.c()
        .i32_const(recipe)
        .local_set(PENDING)
        .else_()
        .i32_const(recipe)
        .local_get(STATUS)
        .local_get(left)
        .local_get(right)
        .local_get(RESULT);
    let resolver = out.internal(8);
    out.c().call(resolver).local_set(STATUS);
    span::clear_status_recipe(out);
    out.c().end();
}

fn sum_error(code: &mut InstructionSink<'_>, r: Residual) {
    code.local_get(r.sum)
        .local_get(r.left)
        .f64_sub()
        .local_set(r.work)
        .local_get(r.sum)
        .local_get(r.work)
        .f64_sub()
        .local_set(r.ah)
        .local_get(r.left)
        .local_get(r.ah)
        .f64_sub()
        .local_get(r.right)
        .local_get(r.work)
        .f64_sub()
        .f64_add()
        .local_set(r.error);
}

fn split(code: &mut InstructionSink<'_>, r: Residual, value: u32, high: u32, low: u32) {
    code.local_get(value)
        .f64_const(134217729.0.into())
        .f64_mul()
        .local_set(r.work)
        .local_get(r.work)
        .local_get(r.work)
        .local_get(value)
        .f64_sub()
        .f64_sub()
        .local_set(high)
        .local_get(value)
        .local_get(high)
        .f64_sub()
        .local_set(low);
}

fn product_error(code: &mut InstructionSink<'_>, r: Residual) {
    // Execution-time bounds keep the split and complete 106-bit residual in the normal range.
    split(code, r, r.left, r.ah, r.al);
    split(code, r, r.right, r.bh, r.bl);
    code.local_get(r.sum)
        .local_get(r.ah)
        .local_get(r.bh)
        .f64_mul()
        .f64_sub()
        .local_set(r.work)
        .local_get(r.work)
        .local_get(r.al)
        .local_get(r.bh)
        .f64_mul()
        .f64_sub()
        .local_set(r.work)
        .local_get(r.work)
        .local_get(r.ah)
        .local_get(r.bl)
        .f64_mul()
        .f64_sub()
        .local_set(r.work)
        .local_get(r.al)
        .local_get(r.bl)
        .f64_mul()
        .local_get(r.work)
        .f64_sub()
        .local_set(r.error);
}

fn result_flags(code: &mut InstructionSink<'_>, r: Residual) {
    code.local_get(r.error)
        .f64_const(0.0.into())
        .f64_ne()
        .local_tee(r.tmp)
        .local_get(r.error)
        .i64_reinterpret_f64()
        .local_get(r.result)
        .i64_reinterpret_f64()
        .i64_xor()
        .i64_const(63)
        .i64_shr_u()
        .i32_wrap_i64()
        .i32_and()
        .local_set(r.exp);
    flags(code, r);
}

fn division_flags(code: &mut InstructionSink<'_>, r: Residual, dividend: u32) {
    code.local_get(dividend)
        .f64_abs()
        .local_set(r.right)
        .local_get(r.sum)
        .local_get(r.right)
        .f64_ne()
        .local_get(r.error)
        .f64_const(0.0.into())
        .f64_ne()
        .i32_or()
        .local_set(r.tmp)
        .local_get(r.sum)
        .local_get(r.right)
        .f64_eq()
        .if_(BlockType::Result(ValType::I32))
        .local_get(r.error)
        .f64_const(0.0.into())
        .f64_gt()
        .else_()
        .local_get(r.sum)
        .local_get(r.right)
        .f64_gt()
        .end()
        .local_set(r.exp);
    flags(code, r);
}

fn flags(code: &mut InstructionSink<'_>, r: Residual) {
    code.local_get(r.status)
        .i32_const(!0x200)
        .i32_and()
        .local_get(r.tmp)
        .i32_const(5)
        .i32_shl()
        .i32_or()
        .local_get(r.exp)
        .i32_const(9)
        .i32_shl()
        .i32_or()
        .local_set(r.status);
}

// params: kind, status, left, right, rounded. Every recipe passed all guards at execution.
pub(super) fn emit_resolver() -> Function {
    let r = Residual {
        status: 1,
        tmp: 5,
        exp: 6,
        sum: 7,
        ah: 8,
        al: 9,
        bh: 10,
        bl: 11,
        work: 12,
        error: 13,
        left: 14,
        right: 15,
        result: 4,
    };
    let mut function = Function::new([(2, ValType::I32), (9, ValType::F64)]);
    let mut code = function.instructions();
    code.local_get(0)
        .i32_eqz()
        .if_(BlockType::Empty)
        .local_get(1)
        .return_()
        .end()
        .local_get(2)
        .local_set(r.left)
        .local_get(3)
        .local_set(r.right)
        .local_get(4)
        .local_set(r.sum)
        .local_get(0)
        .i32_const(1)
        .i32_eq()
        .if_(BlockType::Empty);
    sum_error(&mut code, r);
    result_flags(&mut code, r);
    code.else_()
        .local_get(0)
        .i32_const(2)
        .i32_eq()
        .if_(BlockType::Empty);
    product_error(&mut code, r);
    result_flags(&mut code, r);
    code.else_()
        .local_get(4)
        .f64_abs()
        .local_set(r.left)
        .local_get(3)
        .f64_abs()
        .local_set(r.right)
        .local_get(r.left)
        .local_get(r.right)
        .f64_mul()
        .local_set(r.sum);
    product_error(&mut code, r);
    division_flags(&mut code, r, 2);
    code.end().end().local_get(1).end();
    function
}
