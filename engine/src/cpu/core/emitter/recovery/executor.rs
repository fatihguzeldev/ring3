//! Request-local NeedTags and two topological sweeps. Each immutable source is
//! encoded once. No effects or floating arithmetic are replayed by recovery.
use super::{
    Locals, State,
    program::{Color, SourceOp},
};
use crate::cpu::core::{
    emitter::{BlockType, CARRY_IN, FLAGS, FP_PAYLOAD_TOKENS, KIND, Output, WIDTH},
    region::values::ValueType,
};

fn data(out: &mut Output<'_>, l: Locals, field: u32) {
    let getter = super::super::support::owner_defined(9, !out.native.is_empty());
    out.c()
        .local_get(l.at(1))
        .i32_const(field as i32)
        .i32_add()
        .call(getter);
}
fn i32_data(out: &mut Output<'_>, l: Locals, field: u32) {
    data(out, l, field);
    out.c().i32_wrap_i64();
}
fn upper_data(out: &mut Output<'_>, l: Locals, field: u32) {
    data(out, l, field);
    out.c().i64_const(32).i64_shr_u().i32_wrap_i64();
}
pub(super) fn choose(
    out: &mut Output<'_>,
    colors: &[Color],
    l: Locals,
    action: fn(&mut Output<'_>, Color, Locals),
) {
    if colors.is_empty() {
        return;
    }
    if colors.len() <= 64 {
        let first = colors[0].local - l.expression_base;
        let last = colors[colors.len() - 1].local - l.expression_base;
        let mut labels = vec![colors.len() as u32; (last - first + 1) as usize];
        for (index, color) in colors.iter().enumerate() {
            labels[(color.local - l.expression_base - first) as usize] = index as u32;
        }
        out.c().block(BlockType::Empty);
        for _ in colors.iter().rev() {
            out.c().block(BlockType::Empty);
        }
        out.c()
            .local_get(l.at(6))
            .i32_const(first as i32)
            .i32_sub()
            .br_table(labels, colors.len() as u32);
        for (index, &color) in colors.iter().enumerate() {
            out.c().end();
            action(out, color, l);
            out.c().br((colors.len() - index - 1) as u32);
        }
        out.c().end();
        return;
    }
    let mid = colors.len() / 2;
    let color = colors[mid];
    out.c()
        .local_get(l.at(6))
        .i32_const((color.local - l.expression_base) as i32)
        .i32_eq()
        .if_(BlockType::Empty);
    action(out, color, l);
    out.c()
        .else_()
        .local_get(l.at(6))
        .i32_const((color.local - l.expression_base) as i32)
        .i32_lt_u()
        .if_(BlockType::Empty);
    choose(out, &colors[..mid], l, action);
    out.c().else_();
    choose(out, &colors[mid + 1..], l, action);
    out.c().end().end();
}
fn set_need(out: &mut Output<'_>, color: Color, l: Locals) {
    let tag = l.need(color.local);
    out.c()
        .local_get(tag)
        .local_get(l.at(7))
        .i32_const(-1)
        .i32_mul()
        .i32_ne()
        .if_(BlockType::Empty)
        .local_get(l.at(7))
        .local_set(tag)
        .end();
}
fn read_color(out: &mut Output<'_>, color: Color, l: Locals) {
    let target = match color.ty {
        ValueType::I32 => l.at(13),
        ValueType::I64 => l.wide(2),
        ValueType::F64 => l.float(),
    };
    out.c().local_get(color.local).local_set(target);
}

fn sweeps(out: &mut Output<'_>, state: &State) {
    if state.compact {
        super::recipe_vm::emit(out, state);
    } else {
        super::legacy::emit(out, state);
    }
}

fn target_type(target: u32) -> ValueType {
    if (76..84).contains(&target) || (101..104).contains(&target) {
        ValueType::F64
    } else if (110..118).contains(&target) {
        ValueType::I64
    } else {
        ValueType::I32
    }
}
fn destinations(out: &mut Output<'_>, l: Locals) {
    let targets: Vec<u32> = (2..10)
        .chain([
            FLAGS,
            KIND,
            WIDTH,
            super::super::LEFT,
            super::super::RIGHT,
            super::super::VALUE,
            CARRY_IN,
        ])
        .chain([87, 88, 90, 91, 92, 93, 94, 95, 100, 101, 102, 103])
        .chain(76..84)
        .chain(110..126)
        .collect();
    for target in targets {
        out.c()
            .local_get(l.at(8))
            .i32_const(target as i32)
            .i32_eq()
            .if_(BlockType::Empty);
        let source = match target_type(target) {
            ValueType::I32 => l.at(13),
            ValueType::I64 => l.wide(2),
            ValueType::F64 => l.float(),
        };
        out.c().local_get(source).local_set(target);
        if (76..84).contains(&target) {
            out.c()
                .local_get(85)
                .i32_const(1 << (target - 76))
                .i32_or()
                .local_set(85);
        }
        out.c().end();
    }
}

fn rows(out: &mut Output<'_>, state: &State) {
    let l = state.locals;
    let getter = super::super::support::owner_defined(9, !out.native.is_empty());
    let colors: Vec<_> = state.program.colors().collect();
    fn list(out: &mut Output<'_>, l: Locals, getter: u32) {
        out.c()
            .local_get(l.at(2))
            .i32_const(2)
            .i32_add()
            .local_set(l.at(4))
            .local_get(l.at(2))
            .i32_const(1)
            .i32_add()
            .call(getter)
            .i32_wrap_i64()
            .local_get(l.at(4))
            .i32_add()
            .local_set(l.at(3));
    }
    i32_data(out, l, 0);
    out.c().local_set(l.at(2));
    list(out, l, getter);
    out.c()
        .block(BlockType::Empty)
        .loop_(BlockType::Empty)
        .local_get(l.at(2))
        .i32_eqz()
        .br_if(1)
        .local_get(l.at(4))
        .local_get(l.at(3))
        .i32_ge_u()
        .if_(BlockType::Empty)
        .local_get(l.at(2))
        .call(getter)
        .i32_wrap_i64()
        .local_tee(l.at(2))
        .i32_eqz()
        .br_if(2);
    list(out, l, getter);
    out.c()
        .end()
        .block(BlockType::Empty)
        .local_get(l.at(4))
        .call(getter)
        .i32_wrap_i64()
        .local_tee(l.at(15))
        .call(getter)
        .local_set(l.wide(0))
        .local_get(l.at(15))
        .i32_const(1)
        .i32_add()
        .call(getter)
        .local_tee(l.wide(1))
        .i32_wrap_i64()
        .i32_const(0xffff)
        .i32_and()
        .local_set(l.at(8));
    // First two phases select roots; the final phase publishes their already
    // computed colors. A zero destination is a VALID discriminator only.
    out.c()
        .local_get(l.at(5))
        .i32_const(2)
        .i32_eq()
        .if_(BlockType::Empty)
        .local_get(l.at(8))
        .i32_eqz()
        .br_if(1)
        .else_()
        .local_get(l.wide(1))
        .i32_wrap_i64()
        .i32_const(18)
        .i32_shr_u()
        .i32_const(1)
        .i32_and()
        .local_get(l.at(5))
        .i32_ne()
        .br_if(1)
        .end();
    out.c()
        .local_get(l.wide(1))
        .i32_wrap_i64()
        .i32_const(16)
        .i32_shr_u()
        .i32_const(3)
        .i32_and()
        .local_tee(l.at(10))
        .if_(BlockType::Empty)
        .local_get(84)
        .i32_eqz()
        .br_if(1)
        .local_get(l.wide(1))
        .i32_wrap_i64()
        .i32_const(24)
        .i32_shr_u()
        .local_tee(l.at(15))
        .if_(BlockType::Empty)
        .local_get(l.at(15))
        .local_get(l.at(14))
        .i32_and()
        .i32_eqz()
        .br_if(2)
        .end()
        .end();
    out.c()
        .local_get(l.wide(1))
        .i64_const(32)
        .i64_shr_u()
        .i32_wrap_i64()
        .local_set(l.at(9));
    // One local reader per type serves both the VALID decision and publication.
    // A discriminator request never reads any inactive payload color.
    out.c()
        .local_get(l.at(9))
        .i32_eqz()
        .local_set(l.at(11))
        .block(BlockType::Empty)
        .loop_(BlockType::Empty);
    out.c()
        .local_get(l.at(11))
        .i32_eqz()
        .if_(BlockType::Empty)
        .local_get(l.at(9))
        .i32_const(1)
        .i32_sub()
        .local_set(l.at(6))
        .i32_const(0)
        .local_set(l.at(12))
        .else_()
        .local_get(l.at(5))
        .i32_const(2)
        .i32_eq()
        .if_(BlockType::Empty)
        .local_get(l.wide(0))
        .i64_const(32)
        .i64_shr_u()
        .i32_wrap_i64()
        .local_set(l.at(6))
        .i32_const(0)
        .local_set(l.at(12));
    // Destination type is a fixed role, not an unchecked local index.
    out.c()
        .local_get(l.at(8))
        .i32_const(76)
        .i32_ge_u()
        .local_get(l.at(8))
        .i32_const(84)
        .i32_lt_u()
        .i32_and()
        .local_get(l.at(8))
        .i32_const(101)
        .i32_ge_u()
        .local_get(l.at(8))
        .i32_const(104)
        .i32_lt_u()
        .i32_and()
        .i32_or()
        .if_(BlockType::Empty)
        .i32_const(2)
        .local_set(l.at(12))
        .end()
        .local_get(l.at(8))
        .i32_const(110)
        .i32_ge_u()
        .local_get(l.at(8))
        .i32_const(118)
        .i32_lt_u()
        .i32_and()
        .if_(BlockType::Empty)
        .i32_const(1)
        .local_set(l.at(12))
        .end()
        .else_()
        .br(3)
        .end()
        .end();
    for (ty, code) in [
        (ValueType::I32, 0),
        (ValueType::I64, 1),
        (ValueType::F64, 2),
    ] {
        out.c()
            .local_get(l.at(12))
            .i32_const(code)
            .i32_eq()
            .if_(BlockType::Empty);
        let typed: Vec<_> = colors.iter().copied().filter(|c| c.ty == ty).collect();
        choose(out, &typed, l, read_color);
        out.c().end();
    }
    out.c()
        .local_get(l.at(11))
        .i32_eqz()
        .if_(BlockType::Empty)
        .local_get(l.at(13))
        .i32_eqz()
        .local_get(l.at(10))
        .i32_const(super::NATIVE as i32)
        .i32_eq()
        .i32_eq()
        .br_if(3)
        .i32_const(1)
        .local_set(l.at(11))
        .br(1)
        .else_();
    destinations(out, l);
    out.c().br(2).end().end().end();
    out.c()
        .local_get(l.at(5))
        .i32_const(2)
        .i32_lt_u()
        .if_(BlockType::Empty)
        .local_get(l.wide(0))
        .i32_wrap_i64()
        .local_tee(l.at(7))
        .i32_const(0)
        .i32_gt_s()
        .if_(BlockType::Empty)
        .local_get(l.wide(0))
        .i64_const(32)
        .i64_shr_u()
        .i32_wrap_i64()
        .local_set(l.at(6));
    let pending_colors: std::collections::BTreeSet<_> = state
        .program
        .refs()
        .iter()
        .filter(|source| !matches!(source.op, SourceOp::Available))
        .map(|source| source.color.local)
        .collect();
    let pending: Vec<_> = colors
        .iter()
        .copied()
        .filter(|color| pending_colors.contains(&color.local))
        .collect();
    choose(out, &pending, l, set_need);
    out.c()
        .end()
        .end()
        .end()
        .local_get(l.at(4))
        .i32_const(1)
        .i32_add()
        .local_set(l.at(4))
        .br(0)
        .end()
        .end();
}

fn frame(out: &mut Output<'_>, l: Locals) {
    if out.native.is_empty() {
        return;
    }
    upper_data(out, l, 1);
    out.c()
        .local_tee(l.at(15))
        .i32_const(1)
        .i32_eq()
        .if_(BlockType::Empty)
        .i32_const(0)
        .local_set(109)
        .else_()
        .local_get(l.at(15))
        .i32_const(2)
        .i32_ge_u()
        .local_get(84)
        .i32_and()
        .local_get(126)
        .i32_eqz()
        .i32_and()
        .local_get(109)
        .i32_eqz()
        .i32_eqz()
        .i32_and()
        .if_(BlockType::Empty);
    data(out, l, 3);
    out.c()
        .local_tee(l.wide(1))
        .i64_const(48)
        .i64_shr_u()
        .i32_wrap_i64()
        .i32_const(1)
        .i32_and()
        .if_(BlockType::Empty)
        .local_get(l.wide(1))
        .i32_wrap_i64()
        .local_set(91)
        .local_get(l.wide(1))
        .i64_const(32)
        .i64_shr_u()
        .i32_wrap_i64()
        .i32_const(0xffff)
        .i32_and()
        .local_set(93)
        .local_get(96)
        .i32_load16_u(super::super::mem(102, 1))
        .local_set(94)
        .end()
        .local_get(l.wide(1))
        .i64_const(49)
        .i64_shr_u()
        .i32_wrap_i64()
        .i32_const(7)
        .i32_and()
        .local_tee(l.at(6))
        .if_(BlockType::Empty)
        .local_get(96)
        .local_get(l.at(6))
        .i32_const(1)
        .i32_sub()
        .i32_const(1)
        .i32_shl()
        .i32_add()
        .i32_load16_u(super::super::mem(100, 1))
        .local_set(95)
        .end()
        .local_get(l.at(15))
        .i32_const(1)
        .i32_sub()
        .local_set(109)
        .end()
        .end();
}

pub(super) fn emit(out: &mut Output<'_>, state: &State) {
    if state.words.is_empty() {
        return;
    }
    let l = state.locals;
    out.c()
        .local_get(l.request())
        .if_(BlockType::Empty)
        .local_get(l.request())
        .i32_const(0)
        .i32_gt_s()
        .local_get(84)
        .i32_or()
        .if_(BlockType::Empty)
        .local_get(l.request())
        .local_get(l.request())
        .i32_const(-1)
        .i32_mul()
        .local_get(l.request())
        .i32_const(0)
        .i32_gt_s()
        .select()
        .i32_const(1)
        .i32_sub()
        .local_set(l.at(1));
    for color in state.program.colors() {
        out.c().i32_const(0).local_set(l.need(color.local));
    }
    out.c().i32_const(0).local_set(l.at(14));
    if !out.native.is_empty() {
        i32_data(out, l, 1);
        out.c()
            .local_tee(l.at(15))
            .i32_const(1 << 16)
            .i32_and()
            .i32_eqz()
            .i32_eqz()
            .local_get(84)
            .i32_and()
            .if_(BlockType::Empty)
            .local_get(86)
            .local_get(l.at(15))
            .i32_const(7)
            .i32_and()
            .i32_shr_u()
            .local_get(86)
            .i32_const(8)
            .local_get(l.at(15))
            .i32_const(7)
            .i32_and()
            .i32_sub()
            .i32_shl()
            .i32_or()
            .i32_const(255)
            .i32_and()
            .local_get(l.at(15))
            .i32_const(8)
            .i32_shr_u()
            .i32_const(255)
            .i32_and()
            .i32_or()
            .local_set(l.at(14))
            .end();
    }
    out.c()
        .i32_const(0)
        .local_set(l.at(5))
        .block(BlockType::Empty)
        .loop_(BlockType::Empty);
    rows(out, state);
    out.c().local_get(l.at(5)).i32_const(2).i32_eq().br_if(1);
    sweeps(out, state);
    // Initialize projection bookkeeping only after both source sweeps. The
    // scalar architectural destinations are likewise untouched until phase 2.
    out.c()
        .local_get(l.at(5))
        .i32_const(1)
        .i32_eq()
        .if_(BlockType::Empty);
    if !out.native.is_empty() {
        i32_data(out, l, 1);
        out.c()
            .i32_const(1 << 16)
            .i32_and()
            .i32_eqz()
            .i32_eqz()
            .local_get(84)
            .i32_and()
            .if_(BlockType::Empty);
        data(out, l, 2);
        out.c()
            .local_set(FP_PAYLOAD_TOKENS)
            .i32_const(0)
            .local_set(85)
            .local_get(l.at(14))
            .local_set(158)
            .end();
    }
    out.c()
        .end()
        .local_get(l.at(5))
        .i32_const(1)
        .i32_add()
        .local_set(l.at(5))
        .br(0)
        .end()
        .end();
    i32_data(out, l, 4);
    out.c()
        .local_tee(l.at(15))
        .if_(BlockType::Empty)
        .local_get(l.at(15))
        .i32_const(1)
        .i32_sub()
        .local_set(KIND);
    upper_data(out, l, 4);
    out.c()
        .local_set(WIDTH)
        .i32_const(0)
        .local_set(CARRY_IN)
        .end();
    frame(out, l);
    i32_data(out, l, 5);
    out.c()
        .local_tee(l.at(15))
        .if_(BlockType::Empty)
        .local_get(l.at(15))
        .local_get(super::super::PREFIX_BASE)
        .i32_sub()
        .local_set(l.at(15))
        .local_get(super::super::REMAIN)
        .local_get(l.at(15))
        .i32_sub()
        .local_set(super::super::REMAIN)
        .local_get(super::super::RETIRED)
        .local_get(l.at(15))
        .i32_add()
        .local_set(super::super::RETIRED)
        .i32_const(0)
        .local_set(super::super::PREFIX_BASE)
        .end();
    i32_data(out, l, 1);
    out.c().i32_const(1 << 17).i32_and().if_(BlockType::Empty);
    upper_data(out, l, 5);
    out.c().local_set(super::super::EIP).end();
    out.c().end().i32_const(0).local_set(l.request()).end();
}
