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
    native_tmp: u32,
    low_tmp: u32,
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
    native_tmp: 97,
    low_tmp: 46,
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
    native_tmp: 37,
    low_tmp: 38,
};

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

fn copy_cell(code: &mut InstructionSink<'_>, destination: u8, source: u8, r: Layout) {
    for (base, temporary) in [
        (r.native, r.native_tmp),
        (r.low, r.low_tmp),
        (r.high, r.high_tmp),
    ] {
        code.local_get(if source == 8 {
            temporary
        } else {
            base + u32::from(source)
        })
        .local_set(if destination == 8 {
            temporary
        } else {
            base + u32::from(destination)
        });
    }
}

fn permute(code: &mut InstructionSink<'_>, cells: [u8; 8], r: Layout) {
    let mut pending: Vec<_> = cells
        .into_iter()
        .enumerate()
        .filter_map(|(to, from)| (to as u8 != from).then_some((to as u8, from)))
        .collect();
    while !pending.is_empty() {
        if let Some(index) = pending
            .iter()
            .position(|(destination, _)| !pending.iter().any(|(_, source)| source == destination))
        {
            let (destination, source) = pending.remove(index);
            copy_cell(code, destination, source, r);
        } else {
            let saved = pending[0].0;
            copy_cell(code, 8, saved, r);
            for (_, source) in &mut pending {
                if *source == saved {
                    *source = 8;
                }
            }
        }
    }
}

fn normalize_shape(code: &mut InstructionSink<'_>, shape: &FpShape, r: Layout) {
    code.local_get(r.top)
        .i32_const(i32::from(shape.top_delta))
        .i32_add()
        .i32_const(7)
        .i32_and()
        .local_set(r.top_tmp)
        .i32_const(0)
        .local_set(r.tag_tmp)
        .i32_const(0)
        .local_set(r.valid_tmp)
        .i32_const(i32::from(shape.written))
        .local_set(r.dirty_tmp);
    for logical in 0..8 {
        let cell = shape.cells[logical].0;
        emit_tag(code, shape.tags[logical], r);
        code.local_get(r.top_tmp)
            .i32_const(logical as i32)
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
            .i32_const(i32::from(cell))
            .i32_shr_u()
            .i32_const(1)
            .i32_and()
            .i32_const(logical as i32)
            .i32_shl()
            .local_get(r.valid_tmp)
            .i32_or()
            .local_set(r.valid_tmp);
        if let Some(source) = shape.inherited_dirty[logical] {
            code.local_get(r.dirty)
                .i32_const(i32::from(source))
                .i32_shr_u()
                .i32_const(1)
                .i32_and()
                .i32_const(logical as i32)
                .i32_shl()
                .local_get(r.dirty_tmp)
                .i32_or()
                .local_set(r.dirty_tmp);
        }
        if shape.written & (1 << logical) != 0 {
            code.local_get(r.valid)
                .i32_const(1 << cell)
                .i32_and()
                .i32_eqz()
                .if_(BlockType::Empty)
                .local_get(r.bank)
                .local_get(r.top_tmp)
                .i32_const(logical as i32)
                .i32_add()
                .i32_const(7)
                .i32_and()
                .i32_const(10)
                .i32_mul()
                .i32_add()
                .i32_const(20)
                .i32_add()
                .local_set(r.pointer_tmp)
                .local_get(r.pointer_tmp)
                .local_get(r.low + u32::from(cell))
                .i64_store(mem(0, 0))
                .local_get(r.pointer_tmp)
                .local_get(r.high + u32::from(cell))
                .i32_store16(mem(8, 0))
                .end();
        }
    }
    permute(code, shape.cells.map(|cell| cell.0), r);
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
    let mut function =
        wasm_encoder::Function::new([(6, ValType::I32), (1, ValType::F64), (1, ValType::I64)]);
    let mut code = function.instructions();
    for (index, shape) in plan.shapes().iter().enumerate().filter(|_| numeric) {
        code.local_get(KERNEL.selected)
            .i32_const(index as i32 + 1)
            .i32_eq()
            .if_(BlockType::Empty);
        normalize_shape(&mut code, shape, KERNEL);
        code.end();
    }
    code.i32_const(-1).local_set(KERNEL.selected);
    for local in results(KERNEL) {
        code.local_get(local);
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
