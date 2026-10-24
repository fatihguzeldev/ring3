//! only an actual cold request transfers its selected live leaves to the cpu vm.
use super::{Locals, State, packet as p, program::Color};
use crate::cpu::core::{
    StopReason, continuation as d,
    emitter::{BlockType, Output, link, mem},
    region::values::ValueType,
};

const POINTER: u32 = 2;
const STATUS: u32 = 3;
const CURSOR: u32 = 4;
const COUNT: u32 = 5;
const COLOR: u32 = 6;
const MARKER: u32 = 7;
const RECORD: u32 = 8;
const KIND: u32 = 9;
const CELL: u32 = 10;
const LIST: u32 = 11;
const ARG: u32 = 12;
const CALLS: u32 = 13;
pub(super) const SCRATCH_GLOBAL: u32 = link::GLOBAL_COUNT + 2;

fn reject(out: &mut Output<'_>) {
    out.c()
        .if_(BlockType::Empty)
        .i32_const(StopReason::Infrastructure as i32)
        .raw()
        .return_();
    out.c().end();
}
fn entry_load(out: &mut Output<'_>, offset: u32) {
    out.c()
        .global_get(SCRATCH_GLOBAL)
        .i32_load(mem(u64::from(offset), 0));
}
pub(super) fn entry_guard(out: &mut Output<'_>, identity: u64) {
    out.c()
        .global_get(SCRATCH_GLOBAL)
        .i64_extend_i32_u()
        .i64_const(i64::from(p::WORKSPACE_BYTES))
        .i64_add()
        .raw()
        .memory_size(0)
        .i64_extend_i32_u()
        .i64_const(16)
        .i64_shl()
        .i64_gt_u();
    reject(out);
    for (offset, value) in [
        (p::MAGIC_OFFSET, p::MAGIC),
        (p::VERSION_OFFSET, p::VERSION),
        (p::BYTES_OFFSET, p::WORKSPACE_BYTES),
        (p::CAPACITY_OFFSET, p::MAX_CAPTURE_COLORS),
        (p::CELLS, p::CELLS_OFFSET),
    ] {
        entry_load(out, offset);
        out.c().i32_const(value as i32).i32_ne();
        reject(out);
    }
    entry_load(out, p::CONTEXT);
    out.c().local_get(0).i32_ne();
    reject(out);
    entry_load(out, p::CONTINUATION);
    out.c().global_get(link::CONTINUATION).i32_ne();
    reject(out);
    out.c().global_get(link::CONTINUATION).i32_eqz();
    reject(out);
    out.c()
        .global_get(link::CONTINUATION)
        .i64_extend_i32_u()
        .i64_const(d::HEADER_SIZE as i64)
        .i64_add()
        .raw()
        .memory_size(0)
        .i64_extend_i32_u()
        .i64_const(16)
        .i64_shl()
        .i64_gt_u();
    reject(out);
    out.c()
        .global_get(link::CONTINUATION)
        .i64_load(mem(d::CURRENT_ID, 0))
        .i64_const(identity as i64)
        .i64_ne();
    reject(out);
    out.c()
        .global_get(SCRATCH_GLOBAL)
        .i64_load(mem(u64::from(p::ARMED_NONCE), 0))
        .i64_eqz();
    reject(out);
    out.c()
        .global_get(SCRATCH_GLOBAL)
        .i64_load(mem(u64::from(p::ARMED_NONCE), 0))
        .global_get(link::CONTINUATION)
        .i64_load(mem(d::CURRENT_NONCE, 0))
        .i64_ne();
    reject(out);
}
fn put(out: &mut Output<'_>, l: Locals, offset: u32, value: u32) {
    out.c()
        .local_get(l.at(POINTER))
        .i32_const(value as i32)
        .i32_store(mem(u64::from(offset), 0));
}
fn local(out: &mut Output<'_>, l: Locals, offset: u32, value: u32) {
    out.c()
        .local_get(l.at(POINTER))
        .local_get(value)
        .i32_store(mem(u64::from(offset), 0));
}
fn finish_cell(out: &mut Output<'_>, l: Locals) {
    out.c()
        .i64_store(mem(0, 0))
        .local_get(l.at(CELL))
        .local_get(l.at(MARKER))
        .i32_const(-1)
        .i32_mul()
        .i32_store(mem(u64::from(p::CELL_TAG), 0));
}
fn available(out: &mut Output<'_>, color: Color, l: Locals) {
    out.c().local_get(l.at(CELL)).local_get(color.local);
    match color.ty {
        ValueType::I32 => {
            out.c().i64_extend_i32_u();
        }
        ValueType::I64 => {}
        ValueType::F64 => {
            out.c().i64_reinterpret_f64();
        }
    }
    finish_cell(out, l);
}
fn leaves(out: &mut Output<'_>, state: &State) {
    let l = state.locals;
    let getter = super::super::support::owner_defined(9, !out.native.is_empty());
    out.c()
        .local_get(l.request())
        .local_get(l.request())
        .i32_const(-1)
        .i32_mul()
        .local_get(l.request())
        .i32_const(0)
        .i32_gt_s()
        .select()
        .i32_const(5)
        .i32_add()
        .call(getter)
        .local_tee(l.at(32))
        .i32_wrap_i64()
        .local_set(l.at(LIST))
        .local_get(l.at(32))
        .i64_const(32)
        .i64_shr_u()
        .i32_wrap_i64()
        .local_set(l.at(COUNT))
        .i32_const(0)
        .local_set(l.at(CURSOR))
        .block(BlockType::Empty)
        .loop_(BlockType::Empty)
        .local_get(l.at(CURSOR))
        .local_get(l.at(COUNT))
        .i32_ge_u()
        .br_if(1)
        .local_get(l.at(LIST))
        .local_get(l.at(CURSOR))
        .i32_const(1)
        .i32_shr_u()
        .i32_add()
        .call(getter)
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
        .local_tee(l.at(RECORD))
        .call(getter)
        .local_tee(l.at(32))
        .i32_wrap_i64()
        .i32_const(255)
        .i32_and()
        .local_tee(l.at(KIND))
        .i32_const(1)
        .i32_le_u()
        .if_(BlockType::Empty)
        .local_get(l.at(32))
        .i64_const(32)
        .i64_shr_u()
        .i32_wrap_i64()
        .local_tee(l.at(COLOR))
        .i32_const(4)
        .i32_shl()
        .local_get(l.at(POINTER))
        .i32_add()
        .i32_const(p::CELLS_OFFSET as i32)
        .i32_add()
        .local_tee(l.at(CELL))
        .i32_load(mem(u64::from(p::CELL_TAG), 0))
        .local_get(l.at(MARKER))
        .i32_eq()
        .local_get(l.at(CELL))
        .i32_load(mem(u64::from(p::CELL_EPOCH), 0))
        .local_get(l.at(POINTER))
        .i32_load(mem(u64::from(p::EPOCH), 0))
        .i32_eq()
        .i32_and()
        .if_(BlockType::Empty)
        .local_get(l.at(KIND))
        .i32_eqz()
        .if_(BlockType::Empty);
    let colors: Vec<_> = state.program.colors().collect();
    super::executor::choose(out, &colors, l, available);
    out.c()
        .else_()
        .local_get(l.at(RECORD))
        .i32_const(1)
        .i32_add()
        .call(getter)
        .i32_wrap_i64()
        .local_set(l.at(ARG));
    for register in 2..10 {
        out.c()
            .local_get(l.at(ARG))
            .i32_const(register as i32)
            .i32_eq()
            .if_(BlockType::Empty)
            .local_get(l.at(CELL))
            .local_get(register)
            .i64_extend_i32_u();
        finish_cell(out, l);
        out.c().end();
    }
    out.c()
        .end()
        .end()
        .end()
        .local_get(l.at(CURSOR))
        .i32_const(1)
        .i32_add()
        .local_set(l.at(CURSOR))
        .br(0)
        .end()
        .end();
}
pub(super) fn emit(out: &mut Output<'_>, state: &State, identity: u64) {
    if state.words.is_empty() {
        return;
    }
    let l = state.locals;
    let recover = super::super::support::owner_recover(!out.native.is_empty());
    out.c()
        .local_get(l.request())
        .if_(BlockType::Empty)
        .local_get(l.request())
        .i32_const(0)
        .i32_gt_s()
        .local_get(84)
        .i32_or()
        .if_(BlockType::Empty)
        .global_get(SCRATCH_GLOBAL)
        .local_set(l.at(POINTER));
    out.c()
        .local_get(l.at(POINTER))
        .i32_load(mem(u64::from(p::STATE), 0));
    reject(out);
    out.c()
        .local_get(l.at(POINTER))
        .i64_const(identity as i64)
        .i64_store(mem(u64::from(p::EXPECTED_OWNER), 0));
    for (packet, header, wide) in [
        (p::EXPECTED_SLOT, d::CURRENT_SLOT, false),
        (p::EXPECTED_GENERATION, d::CURRENT_GENERATION, true),
        (p::EXPECTED_NONCE, d::CURRENT_NONCE, true),
    ] {
        out.c()
            .local_get(l.at(POINTER))
            .global_get(link::CONTINUATION);
        if wide {
            out.c()
                .i64_load(mem(header, 0))
                .i64_store(mem(u64::from(packet), 0));
        } else {
            out.c()
                .i32_load(mem(header, 0))
                .i32_store(mem(u64::from(packet), 0));
        }
    }
    for (offset, value) in [
        (p::COLORS, l.colors),
        (p::RECIPES_BASE, state.recipes.base),
        (p::RECIPES_COUNT, state.recipes.count),
        (p::SITE_WORDS, state.site_words),
        (p::EXPRESSION_BASE, l.expression_base),
    ] {
        put(out, l, offset, value);
    }
    out.c()
        .local_get(l.at(POINTER))
        .global_get(link::GLOBAL_COUNT + 1)
        .i32_store(mem(u64::from(p::BLOB), 0));
    local(out, l, p::SIGNED_SITE, l.request());
    for (offset, value) in [
        (p::FP_ACTIVE, 84),
        (p::FP_DIRTY, 86),
        (p::SELECTED, 109),
        (p::EXACT_SUFFIX, 126),
        (p::BANK, 96),
        (p::POOL, 152),
        (p::PREFIX_BASE, 131),
        (p::REMAIN, 12),
        (p::RETIRED, 13),
        (p::EIP, 10),
    ] {
        local(out, l, offset, value);
    }
    out.c()
        .i32_const(0)
        .local_set(l.at(CALLS))
        .block(BlockType::Empty)
        .loop_(BlockType::Empty)
        .local_get(l.at(CALLS))
        .i32_const(1)
        .i32_add()
        .local_tee(l.at(CALLS))
        .i32_const(3)
        .i32_gt_u();
    reject(out);
    out.c()
        .local_get(0)
        .global_get(link::GLOBAL_COUNT + 1)
        .local_get(l.request())
        .local_get(l.at(POINTER))
        .call(recover)
        .local_tee(l.at(STATUS))
        .i32_eqz()
        .br_if(1)
        .local_get(l.at(STATUS))
        .i32_const(2)
        .i32_gt_u();
    reject(out);
    leaves(out, state);
    out.c().br(0).end().end();
    for (role, &target) in p::OUTPUT_LOCALS.iter().enumerate() {
        out.c()
            .local_get(l.at(POINTER))
            .i64_load(mem(u64::from(p::OUTPUT_MASK), 0))
            .i64_const((1u64 << role) as i64)
            .i64_and()
            .i64_eqz()
            .i32_eqz()
            .if_(BlockType::Empty)
            .local_get(l.at(POINTER))
            .i64_load(mem(u64::from(p::OUTPUTS + 8 * role as u32), 0));
        match p::output_type(target) {
            p::OutputType::I32 => {
                out.c().i32_wrap_i64();
            }
            p::OutputType::I64 => {}
            p::OutputType::F64 => {
                out.c().f64_reinterpret_i64();
            }
        }
        out.c().local_set(target).end();
    }
    put(out, l, p::STATE, p::IDLE);
    out.c()
        .local_get(l.at(POINTER))
        .i64_const(0)
        .i64_store(mem(u64::from(p::OUTPUT_MASK), 0));
    out.c().end().i32_const(0).local_set(l.request()).end();
}
