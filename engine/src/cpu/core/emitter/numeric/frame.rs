use super::super::{Output, mem};
use crate::cpu::core::region::{
    BlockId,
    values::{
        EdgePlan, FpShape, FrameStateId, InputField, ProofKind, RegionPlan, TagRecipe, ValueKind,
    },
};
use wasm_encoder::{BlockType, InstructionSink, ValType};

pub(super) const SELECTED: u32 = 109;
pub(super) const LOW: u32 = 110;
pub(super) const HIGH: u32 = 118;
pub(super) const EXACT_SUFFIX: u32 = 126;

pub(super) fn input_cell(out: &Output<'_>, logical: u8) -> u8 {
    out.plan
        .shape(
            out.before_frame
                .expect("numeric instruction has a before frame"),
        )
        .cells[logical as usize]
        .0
}

pub(super) fn result_cell(out: &Output<'_>) -> u8 {
    out.plan
        .fp_output_cell(
            out.after_frame
                .expect("numeric instruction has an after frame"),
        )
        .expect("numeric result owns a bounded output cell")
        .0
}

pub(in crate::cpu::core::emitter) fn emit_frame(out: &mut Output<'_>, frame: FrameStateId) {
    if out.native.is_empty() {
        return;
    }
    let shape = out.plan.shape_id(frame).0 as i32 + 1;
    let metadata = out.plan.metadata(frame).clone();
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
    out.c().i32_const(shape).local_set(SELECTED).end();
}

pub(in crate::cpu::core::emitter) fn normalize_frame(out: &mut Output<'_>) {
    let plan = out.plan;
    let numeric = !out.native.is_empty();
    emit_normalizer(plan, &mut out.c(), numeric);
}

pub(in crate::cpu::core::emitter) fn emit_edge(out: &mut Output<'_>, edge: &EdgePlan) {
    if out.native.is_empty() {
        return;
    }
    emit_frame(out, edge.frame);
    if edge.fp_rebase {
        normalize_frame(out);
    } else {
        out.c().i32_const(-1).local_set(SELECTED);
    }
}

pub(in crate::cpu::core::emitter) fn enter_block(out: &mut Output<'_>, block: BlockId) {
    if out.native.is_empty() {
        return;
    }
    // Imports may come from public canonical state, a linked unit, or a helper suffix.
    // The private raw bank is stable until an observation; opaque cells are loaded once.
    out.c()
        .local_get(SELECTED)
        .i32_eqz()
        .local_get(84)
        .i32_eqz()
        .i32_or()
        .local_get(EXACT_SUFFIX)
        .i32_or()
        .local_tee(40)
        .if_(BlockType::Empty);
    out.c().block(BlockType::Empty).block(BlockType::Empty);
    super::span::enter(out);
    for cell in 0..8 {
        out.c()
            .local_get(96)
            .local_get(90)
            .i32_const(cell)
            .i32_add()
            .i32_const(7)
            .i32_and()
            .i32_const(10)
            .i32_mul()
            .i32_add()
            .i32_const(20)
            .i32_add()
            .local_set(39)
            .local_get(39)
            .i64_load(mem(0, 0))
            .local_set(LOW + cell as u32)
            .local_get(39)
            .i32_load16_u(mem(8, 0))
            .local_set(HIGH + cell as u32);
    }
    out.c()
        .i32_const(0)
        .local_set(EXACT_SUFFIX)
        .i32_const(-1)
        .local_set(SELECTED)
        .br(1)
        .end()
        .i32_const(1)
        .local_set(EXACT_SUFFIX)
        .end()
        .end();
    let contract = out.plan.block(block).contract.clone();
    if !contract.guards.is_empty() {
        out.c()
            .local_get(40)
            .if_(BlockType::Empty)
            .block(BlockType::Empty)
            .block(BlockType::Empty)
            .local_get(EXACT_SUFFIX)
            .br_if(0);
        for guard in contract.guards {
            let proof = out.plan.proof(guard).clone();
            if proof.kind == ProofKind::Control {
                super::direct::control_guard(out, 0);
                continue;
            }
            let logical = proof.value.and_then(|value| {
                out.plan
                    .storage(value)
                    .and_then(|storage| {
                        out.plan
                            .shape(contract.frame)
                            .cells
                            .iter()
                            .position(|cell| *cell == storage.cell)
                            .map(|index| index as u8)
                    })
                    .or_else(|| match out.plan.value(value).kind {
                        ValueKind::Input {
                            field: InputField::FpTag(index),
                            ..
                        } => Some(index),
                        _ => None,
                    })
            });
            let Some(logical) = logical else {
                out.c().br(0);
                continue;
            };
            match proof.kind {
                ProofKind::ExactF64 | ProofKind::Native24 | ProofKind::BoundedF64 => {
                    super::direct::contract_value(out, logical, proof.kind, 0);
                }
                ProofKind::Occupied | ProofKind::Empty => {
                    emit_tag(&mut out.c(), TagRecipe::Entry(logical), HOT);
                    out.c().i32_const(3);
                    if proof.kind == ProofKind::Occupied {
                        out.c().i32_eq();
                    } else {
                        out.c().i32_ne();
                    }
                    out.c().br_if(0);
                }
                ProofKind::Control => unreachable!(),
            }
        }
        out.c().i32_const(0).local_set(EXACT_SUFFIX).br(1).end();
        // A failed speculative entry contract is not an architectural fault. Preserve
        // imported dirty values before handing the suffix to canonical semantics.
        super::span::invalidate(out);
        out.c().i32_const(1).local_set(EXACT_SUFFIX).end().end();
    }
}

pub(super) fn tag(out: &mut Output<'_>, logical: u8) {
    let recipe = out
        .plan
        .shape(out.before_frame.expect("tag has a before frame"))
        .tags[logical as usize];
    emit_tag(&mut out.c(), recipe, HOT);
}

#[derive(Clone, Copy)]
struct Layout {
    native: u32,
    low: u32,
    high: u32,
    valid: u32,
    dirty: u32,
    tag: u32,
    status: u32,
    top: u32,
    bank: u32,
    selected: u32,
    top_tmp: u32,
    tag_tmp: u32,
    valid_tmp: u32,
    dirty_tmp: u32,
    high_tmp: u32,
    pointer_tmp: u32,
}

const HOT: Layout = Layout {
    native: 76,
    low: LOW,
    high: HIGH,
    valid: 85,
    dirty: 86,
    tag: 89,
    status: 88,
    top: 90,
    bank: 96,
    selected: SELECTED,
    top_tmp: 38,
    tag_tmp: 42,
    valid_tmp: 45,
    dirty_tmp: 44,
    high_tmp: 39,
    pointer_tmp: 40,
};

const KERNEL: Layout = Layout {
    native: 0,
    low: 8,
    high: 16,
    valid: 24,
    dirty: 25,
    tag: 26,
    status: 27,
    top: 28,
    bank: 29,
    selected: 30,
    top_tmp: 31,
    tag_tmp: 32,
    valid_tmp: 33,
    dirty_tmp: 34,
    high_tmp: 35,
    pointer_tmp: 36,
};

const DESCRIPTOR_LOW: u32 = 37;
const DESCRIPTOR_HIGH: u32 = 38;
const CELL: u32 = 39;
const LOGICAL: u32 = 40;
const TAG_CODE: u32 = 41;
const SLOT_EFFECTS: u32 = 42;

fn emit_tag(code: &mut InstructionSink<'_>, recipe: TagRecipe, r: Layout) {
    match recipe {
        TagRecipe::Entry(index) => {
            code.local_get(r.tag)
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
            code.i32_const(3);
        }
        TagRecipe::Zero => {
            code.i32_const(1);
        }
        TagRecipe::Normal => {
            code.i32_const(0);
        }
        TagRecipe::Special => {
            code.i32_const(2);
        }
        TagRecipe::Value(cell) => {
            code.local_get(r.valid)
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

fn descriptor(shape: &FpShape) -> (i64, i64) {
    let mut low = 0u64;
    let mut high = 0u64;
    for logical in 0..8 {
        let cell = shape.cells[logical].0;
        let tag = match shape.tags[logical] {
            TagRecipe::Entry(index) => index,
            TagRecipe::Empty => 8,
            TagRecipe::Zero => 9,
            TagRecipe::Normal => 10,
            TagRecipe::Special => 11,
            TagRecipe::Value(cell) => 12 + cell.0,
        };
        debug_assert!(cell < 8 && tag < 20);
        low |= u64::from(cell | tag << 3) << (logical * 8);
        let inherited = shape.inherited_dirty[logical].unwrap_or(8);
        debug_assert!(inherited <= 8);
        let mut effects = inherited | ((shape.written >> logical) & 1) << 4;
        if logical == 0 {
            effects |= (shape.top_delta as u8 & 7) << 5;
        }
        high |= u64::from(effects) << (logical * 8);
    }
    (low as i64, high as i64)
}

fn select_descriptor(code: &mut InstructionSink<'_>, shapes: &[FpShape], selected: u32) {
    let count = shapes.len() as u32;
    code.block(BlockType::Empty).block(BlockType::Empty);
    for _ in shapes {
        code.block(BlockType::Empty);
    }
    code.local_get(selected)
        .i32_const(1)
        .i32_sub()
        .br_table(0..count, count);
    for (index, shape) in shapes.iter().enumerate() {
        let (low, high) = descriptor(shape);
        code.end()
            .i64_const(low)
            .local_set(DESCRIPTOR_LOW)
            .i64_const(high)
            .local_set(DESCRIPTOR_HIGH)
            .br(count - index as u32);
    }
    code.end().unreachable().end();
}

fn select_cell(code: &mut InstructionSink<'_>, base: u32, index: u32, ty: ValType) {
    fn select(
        code: &mut InstructionSink<'_>,
        base: u32,
        index: u32,
        ty: ValType,
        first: u32,
        count: u32,
    ) {
        if count == 1 {
            code.local_get(base + first);
            return;
        }
        let middle = first + count / 2;
        code.local_get(index)
            .i32_const(middle as i32)
            .i32_lt_u()
            .if_(BlockType::Result(ty));
        select(code, base, index, ty, first, count / 2);
        code.else_();
        select(code, base, index, ty, middle, count / 2);
        code.end();
    }
    select(code, base, index, ty, 0, 8);
}

fn descriptor_byte(code: &mut InstructionSink<'_>, descriptor: u32) {
    code.local_get(descriptor)
        .local_get(LOGICAL)
        .i64_extend_i32_u()
        .i64_const(3)
        .i64_shl()
        .i64_shr_u()
        .i32_wrap_i64()
        .i32_const(255)
        .i32_and();
}

fn selected_value_tag(code: &mut InstructionSink<'_>, r: Layout) {
    code.local_get(r.valid)
        .local_get(r.high_tmp)
        .i32_shr_u()
        .i32_const(1)
        .i32_and()
        .if_(BlockType::Result(ValType::I32));
    select_cell(code, r.native, r.high_tmp, ValType::F64);
    code.f64_const(0.0.into()).f64_eq().else_();
    select_cell(code, r.low, r.high_tmp, ValType::I64);
    code.i64_eqz();
    select_cell(code, r.high, r.high_tmp, ValType::I32);
    code.i32_const(0x7fff)
        .i32_and()
        .i32_eqz()
        .i32_and()
        .if_(BlockType::Result(ValType::I32))
        .i32_const(1)
        .else_();
    select_cell(code, r.high, r.high_tmp, ValType::I32);
    code.i32_const(0x7fff)
        .i32_and()
        .i32_const(1)
        .i32_sub()
        .i32_const(0x7ffe)
        .i32_lt_u();
    select_cell(code, r.low, r.high_tmp, ValType::I64);
    code.i64_const(0)
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

fn selected_tag(code: &mut InstructionSink<'_>, r: Layout) {
    code.local_get(TAG_CODE)
        .i32_const(8)
        .i32_lt_u()
        .if_(BlockType::Result(ValType::I32))
        .local_get(r.tag)
        .local_get(r.top)
        .local_get(TAG_CODE)
        .i32_add()
        .i32_const(7)
        .i32_and()
        .i32_const(1)
        .i32_shl()
        .i32_shr_u()
        .i32_const(3)
        .i32_and()
        .else_()
        .local_get(TAG_CODE)
        .i32_const(12)
        .i32_lt_u()
        .if_(BlockType::Result(ValType::I32))
        .i32_const(0x87)
        .local_get(TAG_CODE)
        .i32_const(8)
        .i32_sub()
        .i32_const(1)
        .i32_shl()
        .i32_shr_u()
        .i32_const(3)
        .i32_and()
        .else_()
        .local_get(TAG_CODE)
        .i32_const(12)
        .i32_sub()
        .local_set(r.high_tmp);
    selected_value_tag(code, r);
    code.end().end();
}

fn normalize_descriptor(code: &mut InstructionSink<'_>, r: Layout) {
    code.local_get(r.top)
        .local_get(DESCRIPTOR_HIGH)
        .i64_const(5)
        .i64_shr_u()
        .i32_wrap_i64()
        .i32_const(7)
        .i32_and()
        .i32_add()
        .i32_const(7)
        .i32_and()
        .local_set(r.top_tmp)
        .i32_const(0)
        .local_set(r.tag_tmp)
        .i32_const(0)
        .local_set(r.valid_tmp)
        .i32_const(0)
        .local_set(r.dirty_tmp)
        .i32_const(0)
        .local_set(LOGICAL)
        .loop_(BlockType::Empty);
    descriptor_byte(code, DESCRIPTOR_LOW);
    code.local_tee(CELL)
        .i32_const(3)
        .i32_shr_u()
        .local_set(TAG_CODE)
        .local_get(CELL)
        .i32_const(7)
        .i32_and()
        .local_set(CELL);
    descriptor_byte(code, DESCRIPTOR_HIGH);
    code.i32_const(31).i32_and().local_set(SLOT_EFFECTS);
    selected_tag(code, r);
    code.local_get(r.top_tmp)
        .local_get(LOGICAL)
        .i32_add()
        .i32_const(7)
        .i32_and()
        .i32_const(1)
        .i32_shl()
        .i32_shl()
        .local_get(r.tag_tmp)
        .i32_or()
        .local_set(r.tag_tmp)
        .local_get(r.valid)
        .local_get(CELL)
        .i32_shr_u()
        .i32_const(1)
        .i32_and()
        .local_get(LOGICAL)
        .i32_shl()
        .local_get(r.valid_tmp)
        .i32_or()
        .local_set(r.valid_tmp)
        .local_get(SLOT_EFFECTS)
        .i32_const(15)
        .i32_and()
        .i32_const(8)
        .i32_lt_u()
        .if_(BlockType::Empty)
        .local_get(r.dirty)
        .local_get(SLOT_EFFECTS)
        .i32_const(15)
        .i32_and()
        .i32_shr_u()
        .i32_const(1)
        .i32_and()
        .local_get(LOGICAL)
        .i32_shl()
        .local_get(r.dirty_tmp)
        .i32_or()
        .local_set(r.dirty_tmp)
        .end()
        .local_get(SLOT_EFFECTS)
        .i32_const(16)
        .i32_and()
        .if_(BlockType::Empty)
        .i32_const(1)
        .local_get(LOGICAL)
        .i32_shl()
        .local_get(r.dirty_tmp)
        .i32_or()
        .local_set(r.dirty_tmp)
        .local_get(r.valid)
        .i32_const(1)
        .local_get(CELL)
        .i32_shl()
        .i32_and()
        .i32_eqz()
        .if_(BlockType::Empty)
        .local_get(r.bank)
        .local_get(r.top_tmp)
        .local_get(LOGICAL)
        .i32_add()
        .i32_const(7)
        .i32_and()
        .i32_const(10)
        .i32_mul()
        .i32_add()
        .i32_const(20)
        .i32_add()
        .local_set(r.pointer_tmp)
        .local_get(r.pointer_tmp);
    select_cell(code, r.low, CELL, ValType::I64);
    code.i64_store(mem(0, 0)).local_get(r.pointer_tmp);
    select_cell(code, r.high, CELL, ValType::I32);
    code.i32_store16(mem(8, 0))
        .end()
        .end()
        .local_get(LOGICAL)
        .i32_const(1)
        .i32_add()
        .local_tee(LOGICAL)
        .i32_const(8)
        .i32_ne()
        .br_if(0)
        .end();
    code.local_get(r.valid_tmp)
        .local_set(r.valid)
        .local_get(r.dirty_tmp)
        .local_get(r.valid_tmp)
        .i32_and()
        .local_set(r.dirty)
        .local_get(r.tag_tmp)
        .local_set(r.tag)
        .local_get(r.top_tmp)
        .local_set(r.top)
        .local_get(r.status)
        .i32_const(!0x3800)
        .i32_and()
        .local_get(r.top_tmp)
        .i32_const(11)
        .i32_shl()
        .i32_or()
        .local_set(r.status);
}

pub(in crate::cpu::core::emitter) fn normalizer_parameters() -> Vec<ValType> {
    [ValType::F64; 8]
        .into_iter()
        .chain([ValType::I64; 8])
        .chain([ValType::I32; 15])
        .collect()
}

pub(in crate::cpu::core::emitter) fn normalizer_results() -> Vec<ValType> {
    [ValType::F64; 8]
        .into_iter()
        .chain([ValType::I64; 8])
        .chain([ValType::I32; 14])
        .collect()
}

fn arguments(r: Layout) -> Vec<u32> {
    (r.native..r.native + 8)
        .chain(r.low..r.low + 8)
        .chain(r.high..r.high + 8)
        .chain([r.valid, r.dirty, r.tag, r.status, r.top, r.bank, r.selected])
        .collect()
}

fn results(r: Layout) -> Vec<u32> {
    (r.native..r.native + 8)
        .chain(r.low..r.low + 8)
        .chain(r.high..r.high + 8)
        .chain([r.valid, r.dirty, r.tag, r.status, r.top, r.selected])
        .collect()
}

pub(in crate::cpu::core::emitter) fn emit_normalizer_function(
    plan: &RegionPlan,
    numeric: bool,
) -> wasm_encoder::Function {
    // Original payload parameters 0..23 are immutable snapshots. The descriptor
    // kernel computes metadata first; result selection can then alias or cycle
    // those inputs without destructive copies or a memory-backed payload pool.
    let mut function =
        wasm_encoder::Function::new([(6, ValType::I32), (2, ValType::I64), (4, ValType::I32)]);
    let mut code = function.instructions();
    if numeric {
        select_descriptor(&mut code, plan.shapes(), KERNEL.selected);
        normalize_descriptor(&mut code, KERNEL);
        for (base, ty) in [
            (KERNEL.native, ValType::F64),
            (KERNEL.low, ValType::I64),
            (KERNEL.high, ValType::I32),
        ] {
            for logical in 0..8 {
                code.local_get(DESCRIPTOR_LOW)
                    .i64_const(logical * 8)
                    .i64_shr_u()
                    .i32_wrap_i64()
                    .i32_const(7)
                    .i32_and()
                    .local_set(CELL);
                select_cell(&mut code, base, CELL, ty);
            }
        }
        for local in [
            KERNEL.valid,
            KERNEL.dirty,
            KERNEL.tag,
            KERNEL.status,
            KERNEL.top,
        ] {
            code.local_get(local);
        }
        code.i32_const(-1);
    } else {
        code.i32_const(-1).local_set(KERNEL.selected);
        for local in results(KERNEL) {
            code.local_get(local);
        }
    }
    code.end();
    function
}

pub(in crate::cpu::core::emitter) fn emit_normalizer(
    _plan: &RegionPlan,
    code: &mut InstructionSink<'_>,
    numeric: bool,
) {
    if !numeric {
        return;
    }
    code.local_get(84)
        .local_get(EXACT_SUFFIX)
        .i32_eqz()
        .i32_and()
        .local_get(SELECTED)
        .i32_const(0)
        .i32_gt_s()
        .i32_and()
        .if_(BlockType::Empty);
    for local in arguments(HOT) {
        code.local_get(local);
    }
    code.call(10 + u32::from(numeric));
    for local in results(HOT).into_iter().rev() {
        code.local_set(local);
    }
    code.end();
}

pub(super) fn load_raw(out: &mut Output<'_>, logical: u8, low: u32, high: u32) {
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
}
