use super::super::{Output, mem};
use crate::cpu::core::{
    fp_pool as p,
    region::values::{FrameStateId, ProofKind, StorageCell},
};
use wasm_encoder::{BlockType, Function, ValType};

pub(super) const POINTER: u32 = 152;
pub(super) const LOADED: u32 = 153;
// Low8 certify the complete Native24InSingle domain11 for the current local
// value; bit256 independently certifies the admitted arithmetic control word.
pub(super) const ADMITTED: u32 = 154;

/// Initialize from canonical bytes only after a real canonical import. Linked
/// owners reuse the frame and never execute this eight-slot copy.
pub(super) fn initialize(out: &mut Output<'_>) {
    out.c()
        .local_get(96)
        .i32_const(p::BANK_OFFSET as i32)
        .i32_add()
        .local_set(POINTER)
        .local_get(POINTER)
        .i32_load(mem(p::ACTIVE, 2))
        .i32_eqz()
        .if_(BlockType::Empty);
    for (source, target, wide) in [
        (0, p::CONTROL, false),
        (2, p::STATUS, false),
        (4, p::TAG, false),
        (6, p::OPCODE, false),
        (8, p::FIP, true),
        (12, p::FDP, true),
        (16, p::CS, false),
        (18, p::DS, false),
    ] {
        out.c().local_get(POINTER).local_get(96);
        if wide {
            out.c().i32_load(mem(source, 2));
        } else {
            out.c().i32_load16_u(mem(source, 1));
        }
        out.c().i32_store(mem(target, 2));
    }
    out.c()
        .local_get(POINTER)
        .local_get(POINTER)
        .i32_load(mem(p::STATUS, 2))
        .i32_const(11)
        .i32_shr_u()
        .i32_const(7)
        .i32_and()
        .i32_store(mem(p::TOP, 2))
        .local_get(POINTER)
        .local_get(POINTER)
        .i32_load(mem(p::STATUS, 2))
        .i32_const(!0x3800)
        .i32_and()
        .i32_store(mem(p::STATUS, 2))
        .local_get(POINTER)
        .i32_const(0)
        .i32_store(mem(p::PENDING, 2))
        .local_get(POINTER)
        .i32_const(0)
        .i32_store(mem(p::CHANGED, 2))
        .i32_const(0)
        .local_set(38)
        .loop_(BlockType::Empty)
        .local_get(POINTER)
        .local_get(38)
        .i32_const(5)
        .i32_shl()
        .i32_add()
        .i32_const(p::RECORDS as i32)
        .i32_add()
        .local_set(39)
        .local_get(96)
        .local_get(38)
        .i32_const(10)
        .i32_mul()
        .i32_add()
        .i32_const(20)
        .i32_add()
        .local_set(40)
        .local_get(39)
        .local_get(40)
        .i64_load(mem(0, 0))
        .i64_store(mem(p::LOW, 3))
        .local_get(39)
        .local_get(40)
        .i32_load16_u(mem(8, 0))
        .i32_store(mem(p::HIGH, 2))
        .local_get(39)
        .i32_const(0)
        .i32_store(mem(p::VALID, 2))
        .local_get(39)
        .i32_const(0)
        .i32_store(mem(p::PROOFS, 2))
        .local_get(39)
        .i32_const(0)
        .i32_store(mem(p::PROOF_CONTROL, 2))
        .local_get(38)
        .i32_const(1)
        .i32_add()
        .local_tee(38)
        .i32_const(8)
        .i32_ne()
        .br_if(0)
        .end()
        .local_get(POINTER)
        .i32_const(1)
        .i32_store(mem(p::ACTIVE, 2))
        .end();
}

/// Only scheduled first uses reach this conditional entry adapter. Later
/// dominated uses carry an actual successful SSA admission, not another bit test.
pub(super) fn admit_input(out: &mut Output<'_>, logical: u8, kind: ProofKind) {
    let cell = super::frame::input_cell(out, logical);
    // The low eight bits certify the mode-correlated arithmetic domain, not
    // merely a loaded or exact-f64 lane. Native24 is stronger under PC53 and
    // must still validate its own certificate instead of consuming this bit.
    let cached = kind == ProofKind::Native24InSingle;
    if cached {
        out.c()
            .local_get(ADMITTED)
            .i32_const(1 << cell)
            .i32_and()
            .i32_eqz()
            .if_(BlockType::Empty);
    }
    super::direct::admit_loaded(out, logical, kind, u32::from(cached));
    if matches!(kind, ProofKind::Native24 | ProofKind::Native24InSingle) {
        out.c()
            .local_get(ADMITTED)
            .i32_const(1 << cell)
            .i32_or()
            .local_set(ADMITTED);
    }
    if cached {
        out.c().end();
    }
}

pub(super) fn ensure_cell(
    out: &mut Output<'_>,
    frame: FrameStateId,
    logical: u8,
    cell: StorageCell,
) {
    let Some(origin) = out.plan.fp_input_origin(frame, logical) else {
        return;
    };
    let bit = 1i32 << cell.0;
    out.c()
        .local_get(LOADED)
        .i32_const(bit)
        .i32_and()
        .i32_eqz()
        .if_(BlockType::Empty)
        .local_get(POINTER)
        .local_get(90)
        .i32_const(i32::from(origin))
        .i32_add()
        .i32_const(7)
        .i32_and()
        .i32_const(5)
        .i32_shl()
        .i32_add()
        .i32_const(p::RECORDS as i32)
        .i32_add()
        .local_set(39)
        .local_get(39)
        .i32_load(mem(p::VALID, 2))
        .if_(BlockType::Empty)
        .local_get(39)
        .f64_load(mem(p::NATIVE, 3))
        .local_set(76 + u32::from(cell.0))
        .local_get(85)
        .i32_const(bit)
        .i32_or()
        .local_set(85)
        .else_()
        .local_get(39)
        .i64_load(mem(p::LOW, 3))
        .local_set(super::frame::LOW + u32::from(cell.0))
        .local_get(39)
        .i32_load(mem(p::HIGH, 2))
        .local_set(super::frame::HIGH + u32::from(cell.0))
        .local_get(85)
        .i32_const(!bit)
        .i32_and()
        .local_set(85)
        .end()
        .local_get(LOADED)
        .i32_const(bit)
        .i32_or()
        .local_set(LOADED)
        .end();
}

pub(super) fn mark_loaded(out: &mut Output<'_>, cell: u8) {
    let after = out
        .after_frame
        .expect("a produced payload has an after frame");
    let state = out.plan.frame(after);
    let admitted = (0..8).any(|logical| {
        out.plan.shape(after).cells[logical].0 == cell
            && out.plan.proofs_at(after).iter().any(|&id| {
                let proof = out.plan.proof(id);
                proof.conditional.is_none()
                    && proof.epoch == state.fp.control_epoch
                    && matches!(
                        proof.kind,
                        ProofKind::Native24 | ProofKind::Native24InSingle
                    )
                    && proof
                        .value
                        .is_some_and(|value| state.fp.payload[logical].contains(value))
            })
    });
    out.c()
        .local_get(LOADED)
        .i32_const(1 << cell)
        .i32_or()
        .local_set(LOADED)
        .local_get(ADMITTED)
        .i32_const(if admitted { 1 << cell } else { !(1 << cell) });
    if admitted {
        out.c().i32_or();
    } else {
        out.c().i32_and();
    }
    out.c().local_set(ADMITTED);
}

fn unchanged_input(out: &mut Output<'_>, logical: u8) {
    let origin = out
        .plan
        .fp_input_origin(out.before_frame.unwrap(), logical)
        .unwrap();
    // Internal CFG edges may carry a changed local while its old physical pool
    // record is still uncommitted. That record cannot certify the current value.
    out.c()
        .local_get(86)
        .i32_const(1 << origin)
        .i32_and()
        .i32_eqz();
}

fn input_pointer(out: &mut Output<'_>, logical: u8) -> bool {
    let frame = out.before_frame.expect("input certificate has a frame");
    let Some(origin) = out.plan.fp_input_origin(frame, logical) else {
        return false;
    };
    out.c()
        .local_get(POINTER)
        .local_get(90)
        .i32_const(i32::from(origin))
        .i32_add()
        .i32_const(7)
        .i32_and()
        .i32_const(5)
        .i32_shl()
        .i32_add()
        .i32_const(p::RECORDS as i32)
        .i32_add()
        .local_set(39);
    true
}

fn proof_bit(kind: ProofKind) -> i32 {
    match kind {
        ProofKind::ExactF64 => 1,
        ProofKind::BoundedF64 => 2,
        ProofKind::Native24 => 4,
        ProofKind::Native24InSingle => 8,
        _ => 0,
    }
}

pub(super) fn has_certificate(out: &mut Output<'_>, logical: u8, kind: ProofKind) {
    if !input_pointer(out, logical) {
        out.c().i32_const(0);
        return;
    }
    out.c()
        .local_get(39)
        .i32_load(mem(p::VALID, 2))
        .local_get(39)
        .i32_load(mem(p::PROOFS, 2))
        .i32_const(proof_bit(kind))
        .i32_and()
        .i32_eqz()
        .i32_eqz()
        .i32_and()
        .local_get(39)
        .i32_load(mem(p::PROOF_CONTROL, 2))
        .local_get(87)
        .i32_eq()
        .i32_and();
    unchanged_input(out, logical);
    out.c().i32_and();
}

pub(super) fn certify_input(out: &mut Output<'_>, logical: u8, kind: ProofKind) {
    if !input_pointer(out, logical) {
        return;
    }
    let cell = super::frame::input_cell(out, logical);
    unchanged_input(out, logical);
    out.c().if_(BlockType::Empty);
    out.c()
        .local_get(39)
        .local_get(76 + u32::from(cell))
        .f64_store(mem(p::NATIVE, 3))
        .local_get(39)
        .i32_const(1)
        .i32_store(mem(p::VALID, 2))
        .local_get(39)
        .local_get(39)
        .i32_load(mem(p::PROOFS, 2))
        .i32_const(0)
        .local_get(39)
        .i32_load(mem(p::PROOF_CONTROL, 2))
        .local_get(87)
        .i32_eq()
        .select()
        .i32_const(
            1 | proof_bit(kind)
                | if kind == ProofKind::Native24 {
                    10
                } else if kind == ProofKind::Native24InSingle {
                    2
                } else {
                    0
                },
        )
        .i32_or()
        .i32_store(mem(p::PROOFS, 2))
        .local_get(39)
        .local_get(87)
        .i32_store(mem(p::PROOF_CONTROL, 2))
        .end();
}

/// Status reads observe the deferred recipe without expanding any payload.
pub(super) fn status(out: &mut Output<'_>) {
    out.c()
        .local_get(37)
        .i32_const(p::BANK_OFFSET as i32)
        .i32_add()
        .local_set(POINTER)
        .local_get(POINTER)
        .i32_load(mem(p::ACTIVE, 2))
        .if_(BlockType::Result(ValType::I32))
        .local_get(POINTER)
        .i32_load(mem(p::PENDING, 2))
        .if_(BlockType::Empty);
    for offset in [p::PENDING, p::STATUS] {
        out.c().local_get(POINTER).i32_load(mem(offset, 2));
    }
    for offset in [p::PENDING_LEFT, p::PENDING_RIGHT, p::PENDING_RESULT] {
        out.c().local_get(POINTER).f64_load(mem(offset, 3));
    }
    let resolver = out.internal(8);
    out.c()
        .call(resolver)
        .local_set(41)
        .local_get(POINTER)
        .local_get(41)
        .i32_store(mem(p::STATUS, 2))
        .local_get(POINTER)
        .i32_const(0)
        .i32_store(mem(p::PENDING, 2))
        .end()
        .local_get(POINTER)
        .i32_load(mem(p::STATUS, 2))
        .i32_const(!0x3800)
        .i32_and()
        .local_get(POINTER)
        .i32_load(mem(p::TOP, 2))
        .i32_const(11)
        .i32_shl()
        .i32_or()
        .else_()
        .local_get(37)
        .i32_load16_u(mem(2, 1))
        .end();
}

/// True observation only. This function does not run at compiled graph edges.
/// The pool stores exact opaque bytes even for empty slots; native cells are
/// expanded once here, without changing their independent architectural tags.
pub(in crate::cpu::core::emitter) fn emit_materializer(resolver: u32) -> Function {
    let mut function = Function::new([(4, ValType::I32), (1, ValType::I64), (1, ValType::F64)]);
    let mut c = function.instructions();
    c.local_get(0)
        .i32_eqz()
        .if_(BlockType::Empty)
        .return_()
        .end()
        .local_get(0)
        .i32_const(p::BANK_OFFSET as i32)
        .i32_add()
        .local_set(1)
        .local_get(1)
        .i32_load(mem(p::ACTIVE, 2))
        .i32_eqz()
        .if_(BlockType::Empty)
        .return_()
        .end()
        .local_get(1)
        .i32_load(mem(p::PENDING, 2))
        .if_(BlockType::Empty);
    for offset in [p::PENDING, p::STATUS] {
        c.local_get(1).i32_load(mem(offset, 2));
    }
    for offset in [p::PENDING_LEFT, p::PENDING_RIGHT, p::PENDING_RESULT] {
        c.local_get(1).f64_load(mem(offset, 3));
    }
    c.call(resolver)
        .local_set(4)
        .local_get(1)
        .local_get(4)
        .i32_store(mem(p::STATUS, 2))
        .local_get(1)
        .i32_const(0)
        .i32_store(mem(p::PENDING, 2))
        .end();
    for slot in 0..8u64 {
        c.local_get(1)
            .i32_const((p::RECORDS + slot * p::RECORD_SIZE) as i32)
            .i32_add()
            .local_set(2)
            .local_get(2)
            .i32_load(mem(p::VALID, 2))
            .if_(BlockType::Empty)
            .local_get(2)
            .f64_load(mem(p::NATIVE, 3))
            .i64_reinterpret_f64()
            .local_tee(5)
            .i64_const(i64::MAX)
            .i64_and()
            .i64_eqz()
            .if_(BlockType::Empty)
            .local_get(0)
            .i64_const(0)
            .i64_store(mem(20 + slot * 10, 0))
            .i32_const(0)
            .local_set(3)
            .else_()
            .local_get(0)
            .local_get(5)
            .i64_const(0x000f_ffff_ffff_ffff)
            .i64_and()
            .i64_const(0x0010_0000_0000_0000)
            .i64_or()
            .i64_const(11)
            .i64_shl()
            .i64_store(mem(20 + slot * 10, 0))
            .local_get(5)
            .i64_const(52)
            .i64_shr_u()
            .i32_wrap_i64()
            .i32_const(2047)
            .i32_and()
            .i32_const(15360)
            .i32_add()
            .local_set(3)
            .end()
            .local_get(0)
            .local_get(3)
            .local_get(5)
            .i64_const(48)
            .i64_shr_u()
            .i32_wrap_i64()
            .i32_const(0x8000)
            .i32_and()
            .i32_or()
            .i32_store16(mem(28 + slot * 10, 0))
            .else_()
            .local_get(0)
            .local_get(2)
            .i64_load(mem(p::LOW, 3))
            .i64_store(mem(20 + slot * 10, 0))
            .local_get(0)
            .local_get(2)
            .i32_load(mem(p::HIGH, 2))
            .i32_store16(mem(28 + slot * 10, 0))
            .end();
    }
    for (target, source, wide) in [
        (0, p::CONTROL, false),
        (4, p::TAG, false),
        (6, p::OPCODE, false),
        (8, p::FIP, true),
        (12, p::FDP, true),
        (16, p::CS, false),
        (18, p::DS, false),
    ] {
        c.local_get(0).local_get(1).i32_load(mem(source, 2));
        if wide {
            c.i32_store(mem(target, 2));
        } else {
            c.i32_store16(mem(target, 1));
        }
    }
    c.local_get(0)
        .local_get(1)
        .i32_load(mem(p::STATUS, 2))
        .i32_const(!0x3800)
        .i32_and()
        .local_get(1)
        .i32_load(mem(p::TOP, 2))
        .i32_const(7)
        .i32_and()
        .i32_const(11)
        .i32_shl()
        .i32_or()
        .i32_store16(mem(2, 1))
        .local_get(1)
        .i32_const(0)
        .i32_store(mem(p::CHANGED, 2))
        .local_get(1)
        .i32_const(0)
        .i32_store(mem(p::ACTIVE, 2))
        .end();
    function
}
