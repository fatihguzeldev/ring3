use super::super::{Output, mem};
use crate::cpu::core::{
    fp_pool as p,
    region::{
        BlockId,
        values::{EdgePlan, FrameStateId, ProofKind, RegionPlan, TagRecipe},
    },
};
use wasm_encoder::{BlockType, Function, InstructionSink, ValType};

pub(super) const SELECTED: u32 = 109;
pub(super) const LOW: u32 = 110;
pub(super) const HIGH: u32 = 118;
pub(super) const EXACT_SUFFIX: u32 = 126;

pub(super) fn input_cell(out: &Output<'_>, logical: u8) -> u8 {
    out.plan
        .shape(out.before_frame.expect("numeric input has a frame"))
        .cells[logical as usize]
        .0
}
pub(super) fn result_cell(out: &Output<'_>) -> u8 {
    out.plan
        .fp_output_cell(out.after_frame.expect("numeric result has a frame"))
        .expect("numeric result has bounded storage")
        .0
}

fn metadata(out: &mut Output<'_>, frame: FrameStateId) {
    let metadata = out.plan.metadata(frame).clone();
    if let Some((pc, opcode)) = metadata.instruction {
        out.c()
            .i32_const(pc as i32)
            .local_set(91)
            .i32_const(i32::from(opcode))
            .local_set(93)
            .local_get(96)
            .i32_load16_u(mem(102, 1))
            .local_set(94);
    }
    if let Some(data) = metadata.data {
        out.c()
            .local_get(96)
            .i32_load16_u(mem(
                100 + data.address.resolved_segment().index() as u64 * 2,
                1,
            ))
            .local_set(95);
    }
}

/// Prefix exits record a compact exact frame. The common epilogue owns the one
/// selective commit; no payload-spill body is duplicated at every prefix.
pub(in crate::cpu::core::emitter) fn emit_frame(out: &mut Output<'_>, frame: FrameStateId) {
    if out.native.is_empty() {
        return;
    }
    out.c()
        .local_get(84)
        .local_get(EXACT_SUFFIX)
        .i32_eqz()
        .i32_and()
        .local_get(SELECTED)
        .i32_eqz()
        .i32_eqz()
        .i32_and()
        .if_(BlockType::Empty);
    metadata(out, frame);
    out.c()
        .i32_const(frame.0 as i32 + 1)
        .local_set(SELECTED)
        .end();
}

pub(in crate::cpu::core::emitter) fn normalize_frame(out: &mut Output<'_>) {
    let plan = out.plan;
    let numeric = !out.native.is_empty();
    emit_normalizer(plan, out.c().raw(), numeric);
}

// Header/value admission is deliberately lazy. A SIMD/processor/scalar bridge
// executes no x87 imports. A first x87 use imports the actual pool and validates
// only the values that the instruction really consumes.
pub(in crate::cpu::core::emitter) fn enter_block(out: &mut Output<'_>, _block: BlockId) {
    if out.native.is_empty() {
        return;
    }
    out.c()
        .local_get(SELECTED)
        .i32_eqz()
        .local_get(84)
        .i32_eqz()
        .i32_or()
        .local_get(EXACT_SUFFIX)
        .i32_or()
        .if_(BlockType::Empty);
    for local in [
        84,
        85,
        86,
        super::pool::LOADED,
        super::pool::ADMITTED,
        EXACT_SUFFIX,
    ] {
        out.c().i32_const(0).local_set(local);
    }
    out.c().i32_const(-1).local_set(SELECTED).end();
}

fn physical_tags(out: &mut Output<'_>, tags: [TagRecipe; 8], writes: u8) {
    if writes == 0 {
        return;
    }
    out.c().local_get(89).local_set(42);
    for physical in 0..8u8 {
        if writes & (1 << physical) == 0 {
            continue;
        }
        out.c()
            .local_get(90)
            .i32_const(i32::from(physical))
            .i32_add()
            .i32_const(7)
            .i32_and()
            .i32_const(1)
            .i32_shl()
            .local_set(38)
            .local_get(42)
            .i32_const(3)
            .local_get(38)
            .i32_shl()
            .i32_const(-1)
            .i32_xor()
            .i32_and();
        emit_tag(out.c().raw(), tags[physical as usize], HOT);
        out.c().local_get(38).i32_shl().i32_or().local_set(42);
    }
    out.c().local_get(42).local_set(89);
}

/// Internal CFG edges transfer only differing cells in locals. There is no pool
/// traffic, raw80 reconstruction, or generic eight-slot normalization here.
pub(in crate::cpu::core::emitter) fn emit_edge(out: &mut Output<'_>, edge: &EdgePlan) {
    if out.native.is_empty() {
        return;
    }
    let t = &edge.fp_transfer;
    out.c()
        .local_get(84)
        .local_get(EXACT_SUFFIX)
        .i32_eqz()
        .i32_and()
        .if_(BlockType::Empty);
    metadata(out, edge.frame);
    let mut tags = [TagRecipe::Empty; 8];
    let mut writes = 0u8;
    for logical in 0..8 {
        let physical = (i32::from(t.top_delta) + logical as i32).rem_euclid(8) as usize;
        tags[physical] = t.tags[logical];
        if t.tag_writes & (1 << logical) != 0 {
            writes |= 1 << physical;
        }
    }
    physical_tags(out, tags, writes);
    let copies: Vec<_> = t
        .sources
        .iter()
        .enumerate()
        .filter(|(i, c)| *i != usize::from(c.0))
        .collect();
    if !copies.is_empty() {
        // Simultaneous stack snapshots preserve cycles and duplicate aliases.
        for base in [76, LOW, HIGH] {
            for &(_, cell) in &copies {
                out.c().local_get(base + u32::from(cell.0));
            }
            for &(target, _) in copies.iter().rev() {
                out.c().local_set(base + target as u32);
            }
        }
        for source in [85, super::pool::LOADED, super::pool::ADMITTED] {
            let preserved = if source == super::pool::ADMITTED {
                256
            } else {
                0
            };
            out.c().local_get(source).i32_const(preserved).i32_and();
            for (target, cell) in t.sources.iter().enumerate() {
                out.c()
                    .local_get(source)
                    .i32_const(i32::from(cell.0))
                    .i32_shr_u()
                    .i32_const(1)
                    .i32_and()
                    .i32_const(target as i32)
                    .i32_shl()
                    .i32_or();
            }
            out.c().local_set(source);
        }
    }
    let delta = i32::from(t.top_delta).rem_euclid(8);
    if delta != 0 {
        out.c()
            .local_get(86)
            .i32_const(delta)
            .i32_shr_u()
            .local_get(86)
            .i32_const(8 - delta)
            .i32_shl()
            .i32_or()
            .i32_const(255)
            .i32_and();
    } else {
        out.c().local_get(86);
    }
    out.c()
        .i32_const(i32::from(t.written))
        .i32_or()
        .local_set(86);
    if delta != 0 {
        out.c()
            .local_get(90)
            .i32_const(delta)
            .i32_add()
            .i32_const(7)
            .i32_and()
            .local_set(90);
    }
    out.c().end().i32_const(-1).local_set(SELECTED);
}

#[derive(Clone, Copy)]
struct Layout {
    native: u32,
    low: u32,
    high: u32,
    valid: u32,
    tag: u32,
    top: u32,
    cell: u32,
}
const HOT: Layout = Layout {
    native: 76,
    low: LOW,
    high: HIGH,
    valid: 85,
    tag: 89,
    top: 90,
    cell: 39,
};
const KERNEL: Layout = Layout {
    native: 0,
    low: 8,
    high: 16,
    valid: 24,
    tag: 26,
    top: 28,
    cell: 46,
};

pub(super) fn tag(out: &mut Output<'_>, logical: u8) {
    let recipe = out
        .plan
        .shape(out.before_frame.expect("tag has a frame"))
        .tags[logical as usize];
    emit_tag(out.c().raw(), recipe, HOT);
}

fn emit_tag(c: &mut InstructionSink<'_>, recipe: TagRecipe, r: Layout) {
    match recipe {
        TagRecipe::Entry(index) => {
            c.local_get(r.tag)
                .local_get(r.top)
                .i32_const(i32::from(index))
                .i32_add()
                .i32_const(7)
                .i32_and()
                .i32_const(1)
                .i32_shl()
                .i32_shr_u()
                .i32_const(3)
                .i32_and();
        }
        TagRecipe::Empty => {
            c.i32_const(3);
        }
        TagRecipe::Zero => {
            c.i32_const(1);
        }
        TagRecipe::Normal => {
            c.i32_const(0);
        }
        TagRecipe::Special => {
            c.i32_const(2);
        }
        TagRecipe::Value(cell) => {
            c.local_get(r.valid)
                .i32_const(1 << cell.0)
                .i32_and()
                .if_(BlockType::Result(ValType::I32))
                .local_get(r.native + u32::from(cell.0))
                .f64_const(0.0.into())
                .f64_eq()
                .else_()
                .local_get(r.low + u32::from(cell.0))
                .i64_eqz()
                .local_get(r.high + u32::from(cell.0))
                .i32_const(0x7fff)
                .i32_and()
                .i32_eqz()
                .i32_and()
                .if_(BlockType::Result(ValType::I32))
                .i32_const(1)
                .else_()
                .local_get(r.high + u32::from(cell.0))
                .i32_const(0x7fff)
                .i32_and()
                .i32_const(1)
                .i32_sub()
                .i32_const(0x7ffe)
                .i32_lt_u()
                .local_get(r.low + u32::from(cell.0))
                .i64_const(0)
                .i64_lt_s()
                .i32_and()
                .if_(BlockType::Result(ValType::I32))
                .i32_const(0)
                .else_()
                .i32_const(2)
                .end()
                .end()
                .end();
        }
    }
}

fn select_cell(c: &mut InstructionSink<'_>, base: u32, index: u32, ty: ValType) {
    fn select(
        c: &mut InstructionSink<'_>,
        base: u32,
        index: u32,
        ty: ValType,
        first: u32,
        count: u32,
    ) {
        if count == 1 {
            c.local_get(base + first);
            return;
        }
        let middle = first + count / 2;
        c.local_get(index)
            .i32_const(middle as i32)
            .i32_lt_u()
            .if_(BlockType::Result(ty));
        select(c, base, index, ty, first, count / 2);
        c.else_();
        select(c, base, index, ty, middle, count / 2);
        c.end();
    }
    select(c, base, index, ty, 0, 8);
}

fn selected_tag(c: &mut InstructionSink<'_>) {
    c.local_get(47)
        .i32_const(8)
        .i32_lt_u()
        .if_(BlockType::Result(ValType::I32))
        .local_get(26)
        .local_get(28)
        .local_get(47)
        .i32_add()
        .i32_const(7)
        .i32_and()
        .i32_const(1)
        .i32_shl()
        .i32_shr_u()
        .i32_const(3)
        .i32_and()
        .else_()
        .local_get(47)
        .i32_const(12)
        .i32_lt_u()
        .if_(BlockType::Result(ValType::I32))
        .i32_const(0x87)
        .local_get(47)
        .i32_const(8)
        .i32_sub()
        .i32_const(1)
        .i32_shl()
        .i32_shr_u()
        .i32_const(3)
        .i32_and()
        .else_()
        .local_get(47)
        .i32_const(12)
        .i32_sub()
        .local_set(KERNEL.cell)
        .local_get(24)
        .local_get(KERNEL.cell)
        .i32_shr_u()
        .i32_const(1)
        .i32_and()
        .if_(BlockType::Result(ValType::I32));
    select_cell(c, 0, KERNEL.cell, ValType::F64);
    c.f64_const(0.0.into()).f64_eq().else_();
    select_cell(c, 8, KERNEL.cell, ValType::I64);
    c.i64_eqz();
    select_cell(c, 16, KERNEL.cell, ValType::I32);
    c.i32_const(0x7fff)
        .i32_and()
        .i32_eqz()
        .i32_and()
        .if_(BlockType::Result(ValType::I32))
        .i32_const(1)
        .else_();
    select_cell(c, 16, KERNEL.cell, ValType::I32);
    c.i32_const(0x7fff)
        .i32_and()
        .i32_const(1)
        .i32_sub()
        .i32_const(0x7ffe)
        .i32_lt_u();
    select_cell(c, 8, KERNEL.cell, ValType::I64);
    c.i64_const(0)
        .i64_lt_s()
        .i32_and()
        .if_(BlockType::Result(ValType::I32))
        .i32_const(0)
        .else_()
        .i32_const(2)
        .end()
        .end()
        .end()
        .end()
        .end();
}

fn descriptor(plan: &RegionPlan, frame: FrameStateId) -> (i64, i64) {
    let shape = plan.shape(frame);
    let mut low = 0u64;
    let mut high = u64::from(shape.written)
        | (u64::from(shape.tag_written) << 8)
        | (u64::from(shape.top_delta as u8 & 7) << 16);
    for logical in 0..8 {
        if shape.tag_written & (1 << logical) != 0
            && let TagRecipe::Value(cell) = shape.tags[logical]
        {
            assert_eq!(
                cell, shape.cells[logical],
                "a written tag classifies its own payload"
            );
            assert_ne!(
                shape.written & (1 << logical),
                0,
                "value tag writes accompany payload writes"
            );
        }
        let tag = match shape.tags[logical] {
            TagRecipe::Entry(i) => i,
            TagRecipe::Empty => 8,
            TagRecipe::Zero => 9,
            TagRecipe::Normal => 10,
            TagRecipe::Special => 11,
            TagRecipe::Value(c) => 12 + c.0,
        };
        low |= u64::from(shape.cells[logical].0 | tag << 3) << (logical * 8);
        let payload = plan.frame(frame).fp.payload[logical];
        let mut certificates = 1u64;
        for &id in plan.proofs_at(frame) {
            let proof = plan.proof(id);
            if proof.conditional.is_some()
                || proof.epoch != plan.frame(frame).fp.control_epoch
                || !proof.value.is_some_and(|v| payload.contains(v))
            {
                continue;
            }
            certificates |= match proof.kind {
                ProofKind::BoundedF64 => 2,
                ProofKind::Native24 => 4,
                _ => 0,
            };
        }
        high |= certificates << (24 + logical * 3);
    }
    (low as i64, high as i64)
}

fn select_descriptor(
    c: &mut InstructionSink<'_>,
    plan: &RegionPlan,
    selected: u32,
    low: u32,
    high: u32,
) {
    let mut unique = Vec::new();
    let mut index = std::collections::HashMap::new();
    let mut targets = Vec::new();
    for frame in 0..plan.frames().len() {
        let pair = descriptor(plan, FrameStateId(frame));
        let next = unique.len() as u32;
        let id = *index.entry(pair).or_insert_with(|| {
            unique.push(pair);
            next
        });
        targets.push(id);
    }
    let count = unique.len() as u32;
    c.block(BlockType::Empty).block(BlockType::Empty);
    for _ in &unique {
        c.block(BlockType::Empty);
    }
    c.local_get(selected)
        .i32_const(1)
        .i32_sub()
        .br_table(targets, count);
    for (i, (descriptor_low, descriptor_high)) in unique.into_iter().enumerate() {
        c.end()
            .i64_const(descriptor_low)
            .local_set(low)
            .i64_const(descriptor_high)
            .local_set(high)
            .br(count - i as u32);
    }
    c.end().unreachable().end();
}

pub(in crate::cpu::core::emitter) fn descriptor_parameters() -> Vec<ValType> {
    vec![ValType::I32]
}

pub(in crate::cpu::core::emitter) fn descriptor_results() -> Vec<ValType> {
    vec![ValType::I64, ValType::I64]
}

pub(in crate::cpu::core::emitter) fn emit_descriptor(plan: &RegionPlan, numeric: bool) -> Function {
    let mut f = Function::new([(2, ValType::I64)]);
    let mut c = f.instructions();
    if numeric {
        select_descriptor(&mut c, plan, 0, 1, 2);
    }
    c.local_get(1).local_get(2).end();
    f
}

pub(in crate::cpu::core::emitter) fn normalizer_parameters() -> Vec<ValType> {
    snapshot_types().chain([ValType::I32]).collect()
}
fn snapshot_types() -> impl Iterator<Item = ValType> {
    [ValType::F64; 8]
        .into_iter()
        .chain([ValType::I64; 8])
        .chain([ValType::I32; 21])
        .chain([ValType::F64; 3])
}
pub(in crate::cpu::core::emitter) fn normalizer_results() -> Vec<ValType> {
    Vec::new()
}
pub(in crate::cpu::core::emitter) fn normalizer_kernel_parameters() -> Vec<ValType> {
    snapshot_types().chain([ValType::I64; 2]).collect()
}
pub(in crate::cpu::core::emitter) fn normalizer_kernel_results() -> Vec<ValType> {
    Vec::new()
}

fn rotate_mask(c: &mut InstructionSink<'_>, delta: u32) {
    c.local_get(43)
        .local_get(delta)
        .i32_shl()
        .local_get(43)
        .i32_const(8)
        .local_get(delta)
        .i32_sub()
        .i32_shr_u()
        .i32_or()
        .i32_const(255)
        .i32_and()
        .local_set(43);
}
fn slot(c: &mut InstructionSink<'_>) {
    c.local_get(43)
        .i32_ctz()
        .local_set(44)
        .local_get(44)
        .local_get(51)
        .i32_sub()
        .i32_const(7)
        .i32_and()
        .local_set(45)
        .local_get(28)
        .local_get(44)
        .i32_add()
        .i32_const(7)
        .i32_and()
        .local_set(55);
}
fn next_slot(c: &mut InstructionSink<'_>) {
    c.local_get(43)
        .local_get(43)
        .i32_const(1)
        .i32_sub()
        .i32_and()
        .local_tee(43)
        .br_if(0);
}

/// Source-independent selective commit. It visits changed physical bits only;
/// untouched ghosts stay in the pool and no raw80 canonicalization occurs.
pub(in crate::cpu::core::emitter) fn emit_normalizer_kernel() -> Function {
    let mut f = Function::new([(16, ValType::I32)]);
    let mut c = f.instructions();
    c.local_get(29)
        .i32_const(p::BANK_OFFSET as i32)
        .i32_add()
        .local_set(42)
        .local_get(41)
        .i64_const(16)
        .i64_shr_u()
        .i32_wrap_i64()
        .i32_const(7)
        .i32_and()
        .local_set(51)
        .local_get(28)
        .local_get(51)
        .i32_add()
        .i32_const(7)
        .i32_and()
        .local_set(52)
        .local_get(26)
        .local_set(48)
        .local_get(41)
        .i64_const(8)
        .i64_shr_u()
        .i32_wrap_i64()
        .i32_const(255)
        .i32_and()
        .local_set(43);
    rotate_mask(&mut c, 51);
    c.local_get(43)
        .if_(BlockType::Empty)
        .loop_(BlockType::Empty);
    slot(&mut c);
    c.local_get(40)
        .local_get(45)
        .i64_extend_i32_u()
        .i64_const(3)
        .i64_shl()
        .i64_shr_u()
        .i32_wrap_i64()
        .i32_const(3)
        .i32_shr_u()
        .i32_const(31)
        .i32_and()
        .local_set(47);
    selected_tag(&mut c);
    c.local_set(56)
        .local_get(55)
        .i32_const(1)
        .i32_shl()
        .local_set(57)
        .local_get(48)
        .i32_const(3)
        .local_get(57)
        .i32_shl()
        .i32_const(-1)
        .i32_xor()
        .i32_and()
        .local_get(56)
        .local_get(57)
        .i32_shl()
        .i32_or()
        .local_set(48);
    next_slot(&mut c);
    c.end().end();
    c.local_get(41)
        .i32_wrap_i64()
        .i32_const(255)
        .i32_and()
        .local_set(43);
    rotate_mask(&mut c, 51);
    c.local_get(43)
        .local_get(25)
        .i32_or()
        .i32_const(255)
        .i32_and()
        .local_tee(43)
        .local_set(54)
        .local_get(43)
        .if_(BlockType::Empty)
        .loop_(BlockType::Empty);
    slot(&mut c);
    c.local_get(40)
        .local_get(45)
        .i64_extend_i32_u()
        .i64_const(3)
        .i64_shl()
        .i64_shr_u()
        .i32_wrap_i64()
        .i32_const(7)
        .i32_and()
        .local_set(46)
        .local_get(42)
        .local_get(55)
        .i32_const(5)
        .i32_shl()
        .i32_add()
        .i32_const(p::RECORDS as i32)
        .i32_add()
        .local_set(49)
        .local_get(24)
        .local_get(46)
        .i32_shr_u()
        .i32_const(1)
        .i32_and()
        .local_tee(50)
        .if_(BlockType::Empty)
        .local_get(49);
    select_cell(&mut c, 0, 46, ValType::F64);
    c.f64_store(mem(p::NATIVE, 3)).else_().local_get(49);
    select_cell(&mut c, 8, 46, ValType::I64);
    c.i64_store(mem(p::LOW, 3)).local_get(49);
    select_cell(&mut c, 16, 46, ValType::I32);
    c.i32_store(mem(p::HIGH, 2))
        .end()
        .local_get(49)
        .local_get(50)
        .i32_store(mem(p::VALID, 2))
        .local_get(49)
        .local_get(41)
        .local_get(45)
        .i32_const(3)
        .i32_mul()
        .i32_const(24)
        .i32_add()
        .i64_extend_i32_u()
        .i64_shr_u()
        .i32_wrap_i64()
        .i32_const(7)
        .i32_and()
        .i32_const(0)
        .local_get(50)
        .select()
        .i32_store(mem(p::PROOFS, 2))
        .local_get(49)
        .local_get(30)
        .i32_store(mem(p::PROOF_CONTROL, 2));
    next_slot(&mut c);
    c.end().end();
    c.local_get(54).local_set(43);
    rotate_mask(&mut c, 28);
    c.local_get(42)
        .local_get(42)
        .i32_load(mem(p::CHANGED, 2))
        .local_get(43)
        .i32_or()
        .i32_store(mem(p::CHANGED, 2));
    for (offset, local) in [
        (p::CONTROL, 30),
        (p::STATUS, 27),
        (p::TAG, 48),
        (p::TOP, 52),
        (p::FIP, 31),
        (p::FDP, 32),
        (p::OPCODE, 33),
        (p::CS, 34),
        (p::DS, 35),
        (p::PENDING, 36),
    ] {
        c.local_get(42).local_get(local);
        if offset == p::STATUS {
            c.i32_const(!0x3800).i32_and();
        }
        c.i32_store(mem(offset, 2));
    }
    for (offset, local) in [
        (p::PENDING_LEFT, 37),
        (p::PENDING_RIGHT, 38),
        (p::PENDING_RESULT, 39),
    ] {
        c.local_get(42).local_get(local).f64_store(mem(offset, 3));
    }
    c.end();
    f
}

pub(in crate::cpu::core::emitter) fn emit_normalizer_adapter(
    _plan: &RegionPlan,
    numeric: bool,
    kernel: u32,
    descriptor: u32,
) -> Function {
    let mut f = Function::new([(2, ValType::I64)]);
    if numeric {
        let mut c = f.instructions();
        c.local_get(40).call(descriptor).local_set(42).local_set(41);
        for local in 0..40 {
            c.local_get(local);
        }
        c.local_get(41).local_get(42).call(kernel).end();
    } else {
        f.instructions().end();
    }
    f
}

pub(in crate::cpu::core::emitter) fn emit_normalizer(
    _plan: &RegionPlan,
    c: &mut InstructionSink<'_>,
    numeric: bool,
) {
    if !numeric {
        return;
    }
    c.local_get(84)
        .local_get(EXACT_SUFFIX)
        .i32_eqz()
        .i32_and()
        .local_get(SELECTED)
        .i32_const(0)
        .i32_gt_s()
        .i32_and()
        .if_(BlockType::Empty);
    for local in (76..84)
        .chain(LOW..LOW + 8)
        .chain(HIGH..HIGH + 8)
        .chain([85, 86, 89, 88, 90, 96, 87, 91, 92, 93, 94, 95, 100])
        .chain([101, 102, 103])
    {
        c.local_get(local);
    }
    c.local_get(SELECTED)
        .call(super::super::support::owner_internal(10, numeric));
    for local in [
        84,
        85,
        86,
        super::pool::LOADED,
        super::pool::ADMITTED,
        SELECTED,
    ] {
        c.i32_const(0).local_set(local);
    }
    c.end();
}

const INLINE_LOW: u32 = 155;
const INLINE_HIGH: u32 = 156;
const INLINE_MASK: u32 = 158;
const INLINE_OFFSET: u32 = 159;
const INLINE_LOGICAL: u32 = 160;
const INLINE_CELL: u32 = 161;
const INLINE_TAG_CODE: u32 = 162;
const INLINE_TAG: u32 = 163;
const INLINE_DELTA: u32 = 164;
const INLINE_TOP: u32 = 165;
const INLINE_CHANGED: u32 = 166;
const INLINE_PHYSICAL: u32 = 167;
const INLINE_SHIFT: u32 = 168;
const INLINE_VALUE_TAG: u32 = 169;
const INLINE_PROOFS: u32 = 170;
const INLINE_TAG_MASK: u32 = 171;

fn inline_rotate_mask(c: &mut InstructionSink<'_>, delta: u32) {
    c.local_get(INLINE_MASK)
        .local_get(delta)
        .i32_shl()
        .local_get(INLINE_MASK)
        .i32_const(8)
        .local_get(delta)
        .i32_sub()
        .i32_shr_u()
        .i32_or()
        .i32_const(255)
        .i32_and()
        .local_set(INLINE_MASK);
}

fn inline_slot(c: &mut InstructionSink<'_>) {
    c.local_get(INLINE_MASK)
        .i32_ctz()
        .local_set(INLINE_OFFSET)
        .local_get(INLINE_OFFSET)
        .local_get(INLINE_DELTA)
        .i32_sub()
        .i32_const(7)
        .i32_and()
        .local_set(INLINE_LOGICAL)
        .local_get(90)
        .local_get(INLINE_OFFSET)
        .i32_add()
        .i32_const(7)
        .i32_and()
        .local_set(INLINE_PHYSICAL)
        .local_get(INLINE_LOW)
        .local_get(INLINE_LOGICAL)
        .i64_extend_i32_u()
        .i64_const(3)
        .i64_shl()
        .i64_shr_u()
        .i32_wrap_i64()
        .i32_const(3)
        .i32_shr_u()
        .i32_const(31)
        .i32_and()
        .local_set(INLINE_TAG_CODE);
}

fn inline_next_slot(c: &mut InstructionSink<'_>) {
    c.local_get(INLINE_MASK)
        .local_get(INLINE_MASK)
        .i32_const(1)
        .i32_sub()
        .i32_and()
        .local_tee(INLINE_MASK)
        .br_if(0);
}

fn inline_set_tag(c: &mut InstructionSink<'_>) {
    c.local_get(INLINE_PHYSICAL)
        .i32_const(1)
        .i32_shl()
        .local_set(INLINE_SHIFT)
        .local_get(INLINE_TAG)
        .i32_const(3)
        .local_get(INLINE_SHIFT)
        .i32_shl()
        .i32_const(-1)
        .i32_xor()
        .i32_and()
        .local_get(INLINE_VALUE_TAG)
        .local_get(INLINE_SHIFT)
        .i32_shl()
        .i32_or()
        .local_set(INLINE_TAG);
}

// Each case reads only one live storage cell. There is no all-cell argument
// snapshot and the shared writers never inspect the caller's other values.
fn inline_write_cell(c: &mut InstructionSink<'_>, native: u32, raw: u32) {
    c.block(BlockType::Result(ValType::I32))
        .block(BlockType::Empty);
    for _ in 0..8 {
        c.block(BlockType::Empty);
    }
    c.local_get(INLINE_CELL).br_table(0..8, 8);
    for cell in 0..8u32 {
        c.end()
            .local_get(85)
            .i32_const(1 << cell)
            .i32_and()
            .if_(BlockType::Result(ValType::I32))
            .local_get(super::pool::POINTER)
            .local_get(INLINE_PHYSICAL)
            .local_get(76 + cell)
            .local_get(INLINE_PROOFS)
            .local_get(87)
            .call(native)
            .else_()
            .local_get(super::pool::POINTER)
            .local_get(INLINE_PHYSICAL)
            .local_get(LOW + cell)
            .local_get(HIGH + cell)
            .call(raw);
        c.end().br(8 - cell);
    }
    c.end().unreachable().end();
}

/// The single in-run commit point reads its own live locals. Only a frame ID
/// crosses the descriptor call; ordinary links never form the cold 40-value ABI.
pub(in crate::cpu::core::emitter) fn emit_inline_commit(out: &mut Output<'_>) {
    if out.native.is_empty() {
        return;
    }
    let descriptor = out.internal(14);
    let native = out.internal(12);
    let raw = out.internal(13);
    let mut c = out.c();
    let c = c.raw();
    c.local_get(84)
        .local_get(EXACT_SUFFIX)
        .i32_eqz()
        .i32_and()
        .local_get(SELECTED)
        .i32_const(0)
        .i32_gt_s()
        .i32_and()
        .if_(BlockType::Empty)
        .local_get(SELECTED)
        .call(descriptor)
        .local_set(INLINE_HIGH)
        .local_set(INLINE_LOW)
        .local_get(INLINE_HIGH)
        .i64_const(16)
        .i64_shr_u()
        .i32_wrap_i64()
        .i32_const(7)
        .i32_and()
        .local_set(INLINE_DELTA)
        .local_get(90)
        .local_get(INLINE_DELTA)
        .i32_add()
        .i32_const(7)
        .i32_and()
        .local_set(INLINE_TOP)
        .local_get(89)
        .local_set(INLINE_TAG)
        .local_get(INLINE_HIGH)
        .i64_const(8)
        .i64_shr_u()
        .i32_wrap_i64()
        .i32_const(255)
        .i32_and()
        .local_set(INLINE_MASK);
    inline_rotate_mask(c, INLINE_DELTA);
    c.local_get(INLINE_MASK)
        .local_set(INLINE_TAG_MASK)
        .local_get(INLINE_MASK)
        .if_(BlockType::Empty)
        .loop_(BlockType::Empty);
    inline_slot(c);
    c.local_get(INLINE_TAG_CODE)
        .i32_const(12)
        .i32_lt_u()
        .if_(BlockType::Empty)
        .local_get(INLINE_TAG_CODE)
        .i32_const(8)
        .i32_lt_u()
        .if_(BlockType::Result(ValType::I32))
        .local_get(89)
        .local_get(90)
        .local_get(INLINE_TAG_CODE)
        .i32_add()
        .i32_const(7)
        .i32_and()
        .i32_const(1)
        .i32_shl()
        .i32_shr_u()
        .i32_const(3)
        .i32_and()
        .else_()
        .i32_const(0x87)
        .local_get(INLINE_TAG_CODE)
        .i32_const(8)
        .i32_sub()
        .i32_const(1)
        .i32_shl()
        .i32_shr_u()
        .i32_const(3)
        .i32_and()
        .end()
        .local_set(INLINE_VALUE_TAG);
    inline_set_tag(c);
    c.end();
    inline_next_slot(c);
    c.end().end();
    c.local_get(INLINE_HIGH)
        .i32_wrap_i64()
        .i32_const(255)
        .i32_and()
        .local_set(INLINE_MASK);
    inline_rotate_mask(c, INLINE_DELTA);
    c.local_get(INLINE_MASK)
        .local_get(86)
        .i32_or()
        .i32_const(255)
        .i32_and()
        .local_tee(INLINE_MASK)
        .local_set(INLINE_CHANGED)
        .local_get(INLINE_MASK)
        .if_(BlockType::Empty)
        .loop_(BlockType::Empty);
    inline_slot(c);
    c.local_get(INLINE_LOW)
        .local_get(INLINE_LOGICAL)
        .i64_extend_i32_u()
        .i64_const(3)
        .i64_shl()
        .i64_shr_u()
        .i32_wrap_i64()
        .i32_const(7)
        .i32_and()
        .local_set(INLINE_CELL)
        .local_get(INLINE_HIGH)
        .local_get(INLINE_LOGICAL)
        .i32_const(3)
        .i32_mul()
        .i32_const(24)
        .i32_add()
        .i64_extend_i32_u()
        .i64_shr_u()
        .i32_wrap_i64()
        .i32_const(7)
        .i32_and()
        .local_set(INLINE_PROOFS);
    inline_write_cell(c, native, raw);
    c.local_set(INLINE_VALUE_TAG)
        .local_get(INLINE_TAG_MASK)
        .i32_const(1)
        .local_get(INLINE_OFFSET)
        .i32_shl()
        .i32_and()
        .i32_eqz()
        .i32_eqz()
        .local_get(INLINE_TAG_CODE)
        .i32_const(12)
        .i32_ge_u()
        .i32_and()
        .if_(BlockType::Empty);
    inline_set_tag(c);
    c.end();
    inline_next_slot(c);
    c.end().end();
    c.local_get(INLINE_CHANGED).local_set(INLINE_MASK);
    inline_rotate_mask(c, 90);
    c.local_get(super::pool::POINTER)
        .local_get(super::pool::POINTER)
        .i32_load(mem(p::CHANGED, 2))
        .local_get(INLINE_MASK)
        .i32_or()
        .i32_store(mem(p::CHANGED, 2));
    for (offset, local) in [
        (p::CONTROL, 87),
        (p::STATUS, 88),
        (p::TAG, INLINE_TAG),
        (p::TOP, INLINE_TOP),
        (p::FIP, 91),
        (p::FDP, 92),
        (p::OPCODE, 93),
        (p::CS, 94),
        (p::DS, 95),
        (p::PENDING, 100),
    ] {
        c.local_get(super::pool::POINTER).local_get(local);
        if offset == p::STATUS {
            c.i32_const(!0x3800).i32_and();
        }
        c.i32_store(mem(offset, 2));
    }
    for (offset, local) in [
        (p::PENDING_LEFT, 101),
        (p::PENDING_RIGHT, 102),
        (p::PENDING_RESULT, 103),
    ] {
        c.local_get(super::pool::POINTER)
            .local_get(local)
            .f64_store(mem(offset, 3));
    }
    for local in [
        84,
        85,
        86,
        super::pool::LOADED,
        super::pool::ADMITTED,
        SELECTED,
    ] {
        c.i32_const(0).local_set(local);
    }
    c.end();
}

pub(super) fn load_raw(out: &mut Output<'_>, logical: u8, low: u32, high: u32) {
    super::pool::ensure_input(out, logical);
    let cell = u32::from(input_cell(out, logical));
    out.c()
        .local_get(85)
        .i32_const(1 << cell)
        .i32_and()
        .if_(BlockType::Empty)
        .local_get(76 + cell)
        .i64_reinterpret_f64()
        .local_set(47)
        .local_get(47)
        .i64_const(i64::MAX)
        .i64_and()
        .i64_eqz()
        .if_(BlockType::Empty)
        .i64_const(0)
        .local_set(low)
        .i32_const(0)
        .local_set(high)
        .else_()
        .local_get(47)
        .i64_const(0x000f_ffff_ffff_ffff)
        .i64_and()
        .i64_const(0x0010_0000_0000_0000)
        .i64_or()
        .i64_const(11)
        .i64_shl()
        .local_set(low)
        .local_get(47)
        .i64_const(52)
        .i64_shr_u()
        .i32_wrap_i64()
        .i32_const(2047)
        .i32_and()
        .i32_const(15360)
        .i32_add()
        .local_set(high)
        .end()
        .local_get(high)
        .local_get(47)
        .i64_const(48)
        .i64_shr_u()
        .i32_wrap_i64()
        .i32_const(0x8000)
        .i32_and()
        .i32_or()
        .local_set(high)
        .else_()
        .local_get(LOW + cell)
        .local_set(low)
        .local_get(HIGH + cell)
        .local_set(high)
        .end();
}

pub(super) fn write_raw_sign(out: &mut Output<'_>, absolute: bool) {
    let source = u32::from(input_cell(out, 0));
    let destination = u32::from(result_cell(out));
    out.c()
        .local_get(LOW + source)
        .local_set(LOW + destination)
        .local_get(HIGH + source);
    if absolute {
        out.c().i32_const(0x7fff).i32_and();
    } else {
        out.c().i32_const(0x8000).i32_xor();
    }
    out.c()
        .local_set(HIGH + destination)
        .local_get(85)
        .i32_const(!(1 << destination))
        .i32_and()
        .local_set(85);
    super::pool::mark_loaded(out, destination as u8);
}
