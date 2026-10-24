//! pure bit/selector sources; no decoded instruction or rounded effect is replayed.
use super::*;
use crate::cpu::core::fp_pool as pool;

fn result32(v: &mut Vm) {
    v.c().i64_extend_i32_u().local_set(RESULT);
}
fn input_top(v: &mut Vm) {
    v.c()
        .local_get(DEP)
        .if_(BlockType::Result(ValType::I32))
        .local_get(D0)
        .i32_wrap_i64()
        .else_();
    v.load(3, p::POOL);
    v.c().i32_load(mem(pool::TOP, 0)).end();
}
fn record(v: &mut Vm, logical: u32, explicit_top: bool) {
    v.load(3, p::POOL);
    if explicit_top {
        v.c().local_get(D0).i32_wrap_i64();
    } else {
        input_top(v);
    }
    v.c()
        .local_get(logical)
        .i32_add()
        .i32_const(7)
        .i32_and()
        .i32_const(5)
        .i32_shl()
        .i32_add()
        .i32_const(pool::RECORDS as i32)
        .i32_add()
        .local_set(TEMP);
}
fn input(v: &mut Vm) {
    // the record's first dependency is its explicit logical TOP, when present.
    let word = v.word;
    v.c()
        .local_get(1)
        .local_get(RECORD)
        .i32_const(2)
        .i32_add()
        .call(word)
        .i32_wrap_i64()
        .local_set(DEP);
    for (field, offset, wide) in [
        (0, pool::NATIVE, true),
        (1, pool::LOW, true),
        (2, pool::HIGH, false),
        (3, pool::VALID, false),
    ] {
        v.c()
            .local_get(ARG)
            .i32_const(field)
            .i32_eq()
            .if_(BlockType::Empty);
        record(v, ARGHI, false);
        v.c().local_get(TEMP);
        if wide {
            v.c().i64_load(mem(offset, 0)).local_set(RESULT);
        } else {
            v.c().i32_load(mem(offset, 0));
            result32(v);
        }
        v.c().end();
    }
    v.c()
        .local_get(ARG)
        .i32_const(4)
        .i32_eq()
        .if_(BlockType::Empty);
    v.load(3, p::POOL);
    v.c().i32_load(mem(pool::TAG, 0));
    input_top(v);
    v.c()
        .local_get(ARGHI)
        .i32_add()
        .i32_const(7)
        .i32_and()
        .i32_const(1)
        .i32_shl()
        .i32_shr_u()
        .i32_const(3)
        .i32_and();
    result32(v);
    v.c().end();
    for (field, offset, wide) in [
        (5, pool::TOP, false),
        (6, pool::CONTROL, false),
        (7, pool::STATUS, false),
        (8, pool::PENDING, false),
        (9, pool::PENDING_LEFT, true),
        (10, pool::PENDING_RIGHT, true),
        (11, pool::PENDING_RESULT, true),
        (12, pool::FIP, false),
        (13, pool::FDP, false),
        (14, pool::OPCODE, false),
        (15, pool::CS, false),
        (16, pool::DS, false),
    ] {
        v.c()
            .local_get(ARG)
            .i32_const(field)
            .i32_eq()
            .if_(BlockType::Empty);
        v.load(3, p::POOL);
        if wide {
            v.c().i64_load(mem(offset, 0)).local_set(RESULT);
        } else {
            v.c().i32_load(mem(offset, 0));
            result32(v);
        }
        v.c().end();
    }
    v.c()
        .local_get(ARG)
        .i32_const(17)
        .i32_eq()
        .if_(BlockType::Empty);
    v.load(3, p::BANK);
    v.c()
        .local_get(ARGHI)
        .i32_const(1)
        .i32_shl()
        .i32_add()
        .i32_load16_u(mem(100, 0));
    result32(v);
    v.c().end();
}
fn native_class(v: &mut Vm, value: u32) {
    v.c()
        .local_get(value)
        .f64_reinterpret_i64()
        .f64_const(0.0.into())
        .f64_eq();
}
fn raw_class(v: &mut Vm, low: u32, high: u32) {
    v.c()
        .local_get(low)
        .i64_eqz()
        .local_get(high)
        .i32_wrap_i64()
        .i32_const(0x7fff)
        .i32_and()
        .i32_eqz()
        .i32_and()
        .if_(BlockType::Result(ValType::I32))
        .i32_const(1)
        .else_()
        .local_get(high)
        .i32_wrap_i64()
        .i32_const(0x7fff)
        .i32_and()
        .i32_const(1)
        .i32_sub()
        .i32_const(0x7ffe)
        .i32_lt_u()
        .local_get(low)
        .i64_const(0)
        .i64_lt_s()
        .i32_and()
        .if_(BlockType::Result(ValType::I32))
        .i32_const(0)
        .else_()
        .i32_const(2)
        .end()
        .end();
}
pub(super) fn evaluate(v: &mut Vm) {
    v.c().i64_const(0).local_set(RESULT);
    v.c().local_get(KIND).i32_const(14).i32_gt_u();
    v.reject();
    for kind in 2..=14 {
        v.c()
            .local_get(KIND)
            .i32_const(kind)
            .i32_eq()
            .if_(BlockType::Empty);
        match kind {
            2 => {
                v.c().local_get(VALUE).local_set(RESULT);
            }
            3..=7 => {
                v.c()
                    .local_get(D0)
                    .i32_wrap_i64()
                    .local_get(D1)
                    .i32_wrap_i64();
                match kind {
                    3 => {
                        v.c().i32_add();
                    }
                    4 => {
                        v.c().i32_sub();
                    }
                    5 => {
                        v.c().i32_and();
                    }
                    6 => {
                        v.c().i32_or();
                    }
                    7 => {
                        v.c().i32_xor();
                    }
                    _ => unreachable!(),
                };
                result32(v);
            }
            8 => {
                v.c().local_get(D0).i32_wrap_i64().local_get(ARG).i32_shl();
                result32(v);
            }
            9 => input(v),
            10 => {
                v.c()
                    .local_get(D0)
                    .i32_wrap_i64()
                    .i32_const(0x300)
                    .i32_and()
                    .i32_const(0x200)
                    .i32_eq();
                result32(v);
            }
            11 => {
                record(v, ARG, true);
                v.c()
                    .local_get(D2)
                    .i32_wrap_i64()
                    .local_get(D1)
                    .i32_wrap_i64()
                    .local_get(TEMP)
                    .i32_load(mem(pool::PROOF_CONTROL, 0))
                    .i32_eq()
                    .i32_and()
                    .if_(BlockType::Result(ValType::I32))
                    .local_get(TEMP)
                    .i32_load(mem(pool::PROOFS, 0))
                    .i32_const(15)
                    .i32_and()
                    .else_()
                    .i32_const(0)
                    .end();
                result32(v);
            }
            12 => {
                native_class(v, D0);
                result32(v);
            }
            13 => {
                raw_class(v, D0, D1);
                result32(v);
            }
            14 => {
                v.c()
                    .local_get(D0)
                    .i32_wrap_i64()
                    .if_(BlockType::Result(ValType::I32));
                native_class(v, D1);
                v.c().else_();
                raw_class(v, D2, D3);
                v.c().end();
                result32(v);
            }
            _ => unreachable!(),
        }
        v.c().end();
    }
}
