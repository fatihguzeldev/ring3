//! finite source recipes consume only the current descriptor's proven closure.
use super::{Locals, State, program::Color, recipes::Kind};
use crate::cpu::core::{
    emitter::{BlockType, Output, mem, numeric::pool},
    fp_pool as p,
    region::values::ValueType,
};

const CURSOR: u32 = 16;
const RECORD: u32 = 17;
const MODE: u32 = 18;
const KIND: u32 = 19;
const DESTINATION: u32 = 20;
const DEPENDENCY: u32 = 21;
const MARKER: u32 = 22;
const DEPENDENCY_MARKER: u32 = 23;
const COUNT: u32 = 24;
const ARGUMENT: u32 = 25;
const ARGUMENT_HIGH: u32 = 26;
const LIST: u32 = 27;
const POINTER: u32 = 28;
const WRITE_TAG: u32 = 29;
const HAS_TOP: u32 = 30;

fn getter(out: &Output<'_>) -> u32 {
    super::super::support::owner_defined(9, !out.native.is_empty())
}

fn word(out: &mut Output<'_>, l: Locals, field: u32) {
    let function = getter(out);
    out.c()
        .local_get(l.at(RECORD))
        .i32_const(field as i32)
        .i32_add()
        .call(function);
}

fn choose(out: &mut Output<'_>, state: &State, action: fn(&mut Output<'_>, Color, Locals)) {
    let colors: Vec<_> = state.program.colors().collect();
    super::executor::choose(out, &colors, state.locals, action);
}

fn read_tag(out: &mut Output<'_>, color: Color, l: Locals) {
    out.c().local_get(l.need(color.local)).local_set(l.at(13));
}

fn write_tag(out: &mut Output<'_>, color: Color, l: Locals) {
    let tag = l.need(color.local);
    out.c()
        .local_get(l.at(7))
        .i32_const(0)
        .i32_gt_s()
        .if_(BlockType::Empty)
        .local_get(tag)
        .local_get(l.at(7))
        .i32_const(-1)
        .i32_mul()
        .i32_ne()
        .if_(BlockType::Empty)
        .local_get(l.at(7))
        .local_set(tag)
        .end()
        .else_()
        .local_get(l.at(7))
        .local_set(tag)
        .end();
}

fn read_bits(out: &mut Output<'_>, color: Color, l: Locals) {
    out.c().local_get(color.local);
    match color.ty {
        ValueType::I32 => {
            out.c().i64_extend_i32_u();
        }
        ValueType::I64 => {}
        ValueType::F64 => {
            out.c().i64_reinterpret_f64();
        }
    }
    out.c().local_set(l.wide(2));
}

fn write_bits(out: &mut Output<'_>, color: Color, l: Locals) {
    out.c().local_get(l.wide(7));
    match color.ty {
        ValueType::I32 => {
            out.c().i32_wrap_i64();
        }
        ValueType::I64 => {}
        ValueType::F64 => {
            out.c().f64_reinterpret_i64();
        }
    }
    out.c().local_set(color.local);
}

fn result32(out: &mut Output<'_>, l: Locals) {
    out.c().i64_extend_i32_u().local_set(l.wide(7));
}

fn input_top(out: &mut Output<'_>, l: Locals) {
    out.c()
        .local_get(l.at(HAS_TOP))
        .if_(BlockType::Result(wasm_encoder::ValType::I32))
        .local_get(l.wide(3))
        .i32_wrap_i64()
        .else_()
        .local_get(pool::POINTER)
        .i32_load(mem(p::TOP, 2))
        .end();
}

fn record(out: &mut Output<'_>, l: Locals, logical: u32, supplied_top: bool) {
    out.c().local_get(pool::POINTER);
    if supplied_top {
        out.c().local_get(l.wide(3)).i32_wrap_i64();
    } else {
        input_top(out, l);
    }
    out.c()
        .local_get(l.at(logical))
        .i32_add()
        .i32_const(7)
        .i32_and()
        .i32_const(5)
        .i32_shl()
        .i32_add()
        .i32_const(p::RECORDS as i32)
        .i32_add()
        .local_set(l.at(POINTER));
}

fn fp_input(out: &mut Output<'_>, l: Locals) {
    for (field, offset, wide) in [
        (0, p::NATIVE, true),
        (1, p::LOW, true),
        (2, p::HIGH, false),
        (3, p::VALID, false),
    ] {
        out.c()
            .local_get(l.at(ARGUMENT))
            .i32_const(field)
            .i32_eq()
            .if_(BlockType::Empty);
        record(out, l, ARGUMENT_HIGH, false);
        out.c().local_get(l.at(POINTER));
        if wide {
            out.c().i64_load(mem(offset, 3)).local_set(l.wide(7));
        } else {
            out.c().i32_load(mem(offset, 2));
            result32(out, l);
        }
        out.c().end();
    }
    out.c()
        .local_get(l.at(ARGUMENT))
        .i32_const(4)
        .i32_eq()
        .if_(BlockType::Empty)
        .local_get(pool::POINTER)
        .i32_load(mem(p::TAG, 2));
    input_top(out, l);
    out.c()
        .local_get(l.at(ARGUMENT_HIGH))
        .i32_add()
        .i32_const(7)
        .i32_and()
        .i32_const(1)
        .i32_shl()
        .i32_shr_u()
        .i32_const(3)
        .i32_and();
    result32(out, l);
    out.c().end();
    for (field, offset, wide) in [
        (5, p::TOP, false),
        (6, p::CONTROL, false),
        (7, p::STATUS, false),
        (8, p::PENDING, false),
        (9, p::PENDING_LEFT, true),
        (10, p::PENDING_RIGHT, true),
        (11, p::PENDING_RESULT, true),
        (12, p::FIP, false),
        (13, p::FDP, false),
        (14, p::OPCODE, false),
        (15, p::CS, false),
        (16, p::DS, false),
    ] {
        out.c()
            .local_get(l.at(ARGUMENT))
            .i32_const(field)
            .i32_eq()
            .if_(BlockType::Empty)
            .local_get(pool::POINTER);
        if wide {
            out.c().i64_load(mem(offset, 3)).local_set(l.wide(7));
        } else {
            out.c().i32_load(mem(offset, 2));
            result32(out, l);
        }
        out.c().end();
    }
    out.c()
        .local_get(l.at(ARGUMENT))
        .i32_const(17)
        .i32_eq()
        .if_(BlockType::Empty)
        .local_get(96)
        .local_get(l.at(ARGUMENT_HIGH))
        .i32_const(1)
        .i32_shl()
        .i32_add()
        .i32_load16_u(mem(100, 1));
    result32(out, l);
    out.c().end();
}

fn raw_class(out: &mut Output<'_>, l: Locals, low: u32, high: u32) {
    out.c()
        .local_get(l.wide(low))
        .i64_eqz()
        .local_get(l.wide(high))
        .i32_wrap_i64()
        .i32_const(0x7fff)
        .i32_and()
        .i32_eqz()
        .i32_and()
        .if_(BlockType::Result(wasm_encoder::ValType::I32))
        .i32_const(1)
        .else_()
        .local_get(l.wide(high))
        .i32_wrap_i64()
        .i32_const(0x7fff)
        .i32_and()
        .i32_const(1)
        .i32_sub()
        .i32_const(0x7ffe)
        .i32_lt_u()
        .local_get(l.wide(low))
        .i64_const(0)
        .i64_lt_s()
        .i32_and()
        .if_(BlockType::Result(wasm_encoder::ValType::I32))
        .i32_const(0)
        .else_()
        .i32_const(2)
        .end()
        .end();
}

fn native_class(out: &mut Output<'_>, l: Locals, value: u32) {
    out.c()
        .local_get(l.wide(value))
        .f64_reinterpret_i64()
        .f64_const(0.0.into())
        .f64_eq();
}

fn evaluate(out: &mut Output<'_>, state: &State) {
    let l = state.locals;
    for kind in [
        Kind::PhysicalInput,
        Kind::Constant,
        Kind::Add,
        Kind::Sub,
        Kind::And,
        Kind::Or,
        Kind::Xor,
        Kind::ShiftLeft32,
        Kind::FpInput,
        Kind::Precision,
        Kind::Certificate,
        Kind::ClassNative,
        Kind::ClassRaw,
        Kind::ClassTagged,
    ] {
        if state.recipes.kinds & (1 << kind as u32) == 0 {
            continue;
        }
        out.c()
            .local_get(l.at(KIND))
            .i32_const(kind as i32)
            .i32_eq()
            .if_(BlockType::Empty);
        match kind {
            Kind::PhysicalInput => {
                // physical source roles are validated scalar gprs, not colors.
                for local in 2..10 {
                    out.c()
                        .local_get(l.at(ARGUMENT))
                        .i32_const(local as i32)
                        .i32_eq()
                        .if_(BlockType::Empty)
                        .local_get(local);
                    result32(out, l);
                    out.c().end();
                }
            }
            Kind::Constant => {
                out.c().local_get(l.wide(0)).local_set(l.wide(7));
            }
            Kind::Add | Kind::Sub | Kind::And | Kind::Or | Kind::Xor => {
                out.c()
                    .local_get(l.wide(3))
                    .i32_wrap_i64()
                    .local_get(l.wide(4))
                    .i32_wrap_i64();
                match kind {
                    Kind::Add => {
                        out.c().i32_add();
                    }
                    Kind::Sub => {
                        out.c().i32_sub();
                    }
                    Kind::And => {
                        out.c().i32_and();
                    }
                    Kind::Or => {
                        out.c().i32_or();
                    }
                    Kind::Xor => {
                        out.c().i32_xor();
                    }
                    _ => unreachable!(),
                }
                result32(out, l);
            }
            Kind::ShiftLeft32 => {
                out.c()
                    .local_get(l.wide(3))
                    .i32_wrap_i64()
                    .local_get(l.at(ARGUMENT))
                    .i32_shl();
                result32(out, l);
            }
            Kind::FpInput => fp_input(out, l),
            Kind::Precision => {
                out.c()
                    .local_get(l.wide(3))
                    .i32_wrap_i64()
                    .i32_const(0x300)
                    .i32_and()
                    .i32_const(0x200)
                    .i32_eq();
                result32(out, l);
            }
            Kind::Certificate => {
                record(out, l, ARGUMENT, true);
                out.c()
                    .local_get(l.wide(5))
                    .i32_wrap_i64()
                    .local_get(l.wide(4))
                    .i32_wrap_i64()
                    .local_get(l.at(POINTER))
                    .i32_load(mem(p::PROOF_CONTROL, 2))
                    .i32_eq()
                    .i32_and()
                    .if_(BlockType::Result(wasm_encoder::ValType::I32))
                    .local_get(l.at(POINTER))
                    .i32_load(mem(p::PROOFS, 2))
                    .i32_const(15)
                    .i32_and()
                    .else_()
                    .i32_const(0)
                    .end();
                result32(out, l);
            }
            Kind::ClassNative => {
                native_class(out, l, 3);
                result32(out, l);
            }
            Kind::ClassRaw => {
                raw_class(out, l, 3, 4);
                result32(out, l);
            }
            Kind::ClassTagged => {
                out.c()
                    .local_get(l.wide(3))
                    .i32_wrap_i64()
                    .if_(BlockType::Result(wasm_encoder::ValType::I32));
                native_class(out, l, 4);
                out.c().else_();
                raw_class(out, l, 5, 6);
                out.c().end();
                result32(out, l);
            }
            Kind::Available => unreachable!(),
        }
        out.c().end();
    }
}

fn dependencies(out: &mut Output<'_>, state: &State) {
    let l = state.locals;
    let function = getter(out);
    out.c()
        .i32_const(0)
        .local_set(l.at(DEPENDENCY))
        .block(BlockType::Empty)
        .loop_(BlockType::Empty)
        .local_get(l.at(DEPENDENCY))
        .i32_const(5)
        .i32_ge_u()
        .br_if(1)
        .i32_const(0)
        .local_set(l.at(WRITE_TAG))
        .local_get(l.at(DEPENDENCY))
        .i32_const(4)
        .i32_eq()
        .if_(BlockType::Empty)
        .local_get(l.at(MODE))
        .if_(BlockType::Empty);
    evaluate(out, state);
    out.c().local_get(l.at(DESTINATION)).local_set(l.at(6));
    choose(out, state, write_bits);
    out.c()
        .local_get(l.at(MARKER))
        .i32_const(-1)
        .i32_mul()
        .local_set(l.at(7))
        .i32_const(1)
        .local_set(l.at(WRITE_TAG))
        .end()
        .else_()
        .local_get(l.at(RECORD))
        .i32_const(2)
        .i32_add()
        .local_get(l.at(DEPENDENCY))
        .i32_const(1)
        .i32_shr_u()
        .i32_add()
        .call(function)
        .local_get(l.at(DEPENDENCY))
        .i32_const(1)
        .i32_and()
        .i32_const(5)
        .i32_shl()
        .i64_extend_i32_u()
        .i64_shr_u()
        .i32_wrap_i64()
        .local_set(l.at(DEPENDENCY_MARKER))
        .local_get(l.at(DEPENDENCY))
        .i32_eqz()
        .if_(BlockType::Empty)
        .local_get(l.at(DEPENDENCY_MARKER))
        .local_set(l.at(HAS_TOP))
        .end()
        .local_get(l.at(DEPENDENCY_MARKER))
        .if_(BlockType::Empty)
        .local_get(l.at(DEPENDENCY_MARKER))
        .i32_const(1)
        .i32_sub()
        .i32_const(2)
        .i32_shl()
        .i32_const(state.recipes.base as i32)
        .i32_add()
        .call(function)
        .local_tee(l.wide(1))
        .i64_const(32)
        .i64_shr_u()
        .i32_wrap_i64()
        .local_set(l.at(6))
        .local_get(l.at(MODE))
        .i32_eqz()
        .if_(BlockType::Empty)
        .local_get(l.wide(1))
        .i32_wrap_i64()
        .i32_const(255)
        .i32_and()
        .if_(BlockType::Empty)
        .local_get(l.at(DEPENDENCY_MARKER))
        .local_set(l.at(7))
        .i32_const(1)
        .local_set(l.at(WRITE_TAG))
        .end()
        .else_();
    choose(out, state, read_bits);
    for index in 0..4 {
        out.c()
            .local_get(l.at(DEPENDENCY))
            .i32_const(index)
            .i32_eq()
            .if_(BlockType::Empty)
            .local_get(l.wide(2))
            .local_set(l.wide(3 + index as u32))
            .end();
    }
    out.c()
        .end()
        .end()
        .end()
        .local_get(l.at(WRITE_TAG))
        .if_(BlockType::Empty);
    choose(out, state, write_tag);
    out.c()
        .end()
        .local_get(l.at(DEPENDENCY))
        .i32_const(1)
        .i32_add()
        .local_set(l.at(DEPENDENCY))
        .br(0)
        .end()
        .end();
}

pub(super) fn emit(out: &mut Output<'_>, state: &State) {
    if state.recipes.count == 0 {
        return;
    }
    let l = state.locals;
    let function = getter(out);
    // header word six contains an exact sorted closure, packed two markers per
    // word. it bounds both sweeps to this request, not the whole owner's refs.
    out.c()
        .local_get(l.at(1))
        .i32_const(6)
        .i32_add()
        .call(function)
        .local_tee(l.wide(1))
        .i32_wrap_i64()
        .local_set(l.at(LIST))
        .local_get(l.wide(1))
        .i64_const(32)
        .i64_shr_u()
        .i32_wrap_i64()
        .local_tee(l.at(COUNT))
        .local_set(l.at(CURSOR))
        .i32_const(0)
        .local_set(l.at(MODE))
        .block(BlockType::Empty)
        .loop_(BlockType::Empty)
        .local_get(l.at(MODE))
        .i32_eqz()
        .local_get(l.at(CURSOR))
        .i32_eqz()
        .i32_and()
        .if_(BlockType::Empty)
        .i32_const(1)
        .local_set(l.at(MODE))
        .end()
        .local_get(l.at(MODE))
        .local_get(l.at(CURSOR))
        .local_get(l.at(COUNT))
        .i32_ge_u()
        .i32_and()
        .br_if(1)
        .local_get(l.at(MODE))
        .i32_eqz()
        .if_(BlockType::Empty)
        .local_get(l.at(CURSOR))
        .i32_const(1)
        .i32_sub()
        .local_set(l.at(CURSOR))
        .end()
        .local_get(l.at(LIST))
        .local_get(l.at(CURSOR))
        .i32_const(1)
        .i32_shr_u()
        .i32_add()
        .call(function)
        .local_get(l.at(CURSOR))
        .i32_const(1)
        .i32_and()
        .i32_const(5)
        .i32_shl()
        .i64_extend_i32_u()
        .i64_shr_u()
        .i32_wrap_i64()
        .local_tee(l.at(MARKER))
        .i32_const(1)
        .i32_sub()
        .i32_const(2)
        .i32_shl()
        .i32_const(state.recipes.base as i32)
        .i32_add()
        .local_set(l.at(RECORD));
    word(out, l, 0);
    out.c()
        .local_tee(l.wide(1))
        .i32_wrap_i64()
        .i32_const(255)
        .i32_and()
        .local_set(l.at(KIND))
        .local_get(l.wide(1))
        .i64_const(32)
        .i64_shr_u()
        .i32_wrap_i64()
        .local_tee(l.at(DESTINATION))
        .local_set(l.at(6))
        .i32_const(0)
        .local_set(l.at(13));
    choose(out, state, read_tag);
    out.c()
        .local_get(l.at(KIND))
        .i32_eqz()
        .i32_eqz()
        .local_get(l.at(13))
        .local_get(l.at(MARKER))
        .i32_eq()
        .i32_and()
        .if_(BlockType::Empty);
    word(out, l, 1);
    out.c()
        .local_tee(l.wide(0))
        .i32_wrap_i64()
        .local_set(l.at(ARGUMENT))
        .local_get(l.wide(0))
        .i64_const(32)
        .i64_shr_u()
        .i32_wrap_i64()
        .local_set(l.at(ARGUMENT_HIGH));
    dependencies(out, state);
    out.c()
        .end()
        .local_get(l.at(MODE))
        .if_(BlockType::Empty)
        .local_get(l.at(CURSOR))
        .i32_const(1)
        .i32_add()
        .local_set(l.at(CURSOR))
        .end()
        .br(0)
        .end()
        .end();
}
