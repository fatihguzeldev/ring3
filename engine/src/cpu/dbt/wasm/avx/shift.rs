use super::VectorLayout;
use crate::cpu::x86::{
    avx::{MoveSource, ShiftCount, ShiftInstruction, VectorRegister, VectorWidth},
    simd::PackedKind as P,
};
use wasm_encoder::{BlockType, InstructionSink, MemArg, ValType};
#[derive(Clone, Copy)]
pub(super) struct ShiftLocals {
    pub state: u32,
    pub operand: u32,
    pub sources: [u32; 2],
}
fn arg(offset: u32) -> MemArg {
    MemArg {
        offset: u64::from(offset),
        align: 0,
        memory_index: 0,
    }
}
fn vector(layout: VectorLayout, r: VectorRegister, upper: bool) -> MemArg {
    arg(if upper { layout.upper } else { layout.lower } + r.index() as u32 * 16)
}
// caller preflights the exact guest count span before entering this lowerer.
pub(super) fn emit(
    code: &mut InstructionSink<'_>,
    instruction: ShiftInstruction,
    layout: VectorLayout,
    locals: ShiftLocals,
) {
    if let ShiftCount::Elements(count) = instruction.count() {
        emit_elements(code, instruction, count, layout, locals);
        return;
    }
    let [source, count] = locals.sources;
    code.local_get(locals.state)
        .v128_load(vector(layout, instruction.source(), false))
        .local_set(source);
    let immediate = match instruction.count() {
        ShiftCount::Immediate(n) => Some(n),
        ShiftCount::Register(r) => {
            code.local_get(locals.state)
                .v128_load(vector(layout, r, false))
                .local_set(count);
            None
        }
        ShiftCount::Memory => {
            code.local_get(locals.operand)
                .v128_load(arg(0))
                .local_set(count);
            None
        }
        ShiftCount::Elements(_) => unreachable!("element count dispatched above"),
    };
    code.local_get(locals.state);
    super::super::simd::shift_value(code, instruction.operation(), immediate, locals.sources);
    if instruction.width() == VectorWidth::Ymm {
        // keep the lower result pending while the upper data is captured; the
        // original xmm count stays in its local even if a register aliases.
        code.local_get(locals.state);
        code.local_get(locals.state)
            .v128_load(vector(layout, instruction.source(), true))
            .local_set(source);
        super::super::simd::shift_value(code, instruction.operation(), immediate, locals.sources);
        code.v128_store(vector(layout, instruction.destination(), true));
        code.v128_store(vector(layout, instruction.destination(), false));
    } else {
        code.v128_store(vector(layout, instruction.destination(), false));
        code.local_get(locals.state)
            .v128_const(0)
            .v128_store(vector(layout, instruction.destination(), true));
    }
}
fn extract(code: &mut InstructionSink<'_>, local: u32, bytes: u8, lane: u8) {
    code.local_get(local);
    match bytes {
        4 => {
            code.i32x4_extract_lane(lane);
        }
        8 => {
            code.i64x2_extract_lane(lane);
        }
        _ => unreachable!("validated element shift width"),
    }
}

fn elements_value(
    code: &mut InstructionSink<'_>,
    instruction: ShiftInstruction,
    locals: ShiftLocals,
) {
    let [source, count] = locals.sources;
    let bytes = instruction.operation().lane_bytes();
    let kind = instruction.operation().lane_kind().unwrap();
    code.v128_const(0);
    for lane in 0..16 / bytes {
        // test the complete unsigned element before wasm's scalar shift masks it.
        extract(code, count, bytes, lane);
        if bytes == 4 {
            code.i32_const(32).i32_ge_u();
        } else {
            code.i64_const(64).i64_ge_u();
        }
        code.if_(BlockType::Result(if bytes == 4 {
            ValType::I32
        } else {
            ValType::I64
        }));
        if kind == P::ShiftRightSigned {
            extract(code, source, bytes, lane);
            code.i32_const(31).i32_shr_s();
        } else if bytes == 4 {
            code.i32_const(0);
        } else {
            code.i64_const(0);
        }
        code.else_();
        extract(code, source, bytes, lane);
        extract(code, count, bytes, lane);
        match (kind, bytes) {
            (P::ShiftLeft, 4) => {
                code.i32_shl();
            }
            (P::ShiftRight, 4) => {
                code.i32_shr_u();
            }
            (P::ShiftRightSigned, 4) => {
                code.i32_shr_s();
            }
            (P::ShiftLeft, 8) => {
                code.i64_shl();
            }
            (P::ShiftRight, 8) => {
                code.i64_shr_u();
            }
            _ => unreachable!("validated element shift kind"),
        }
        code.end();
        if bytes == 4 {
            code.i32x4_replace_lane(lane);
        } else {
            code.i64x2_replace_lane(lane);
        }
    }
}

fn emit_elements(
    code: &mut InstructionSink<'_>,
    instruction: ShiftInstruction,
    counts: MoveSource,
    layout: VectorLayout,
    locals: ShiftLocals,
) {
    let [source, count] = locals.sources;
    for upper in [false, true]
        .into_iter()
        .take(instruction.width().bytes() / 16)
    {
        code.local_get(locals.state)
            .v128_load(vector(layout, instruction.source(), upper))
            .local_set(source);
        match counts {
            MoveSource::Register(r) => {
                code.local_get(locals.state)
                    .v128_load(vector(layout, r, upper));
            }
            MoveSource::Memory => {
                code.local_get(locals.operand)
                    .v128_load(arg(if upper { 16 } else { 0 }));
            }
        }
        code.local_set(count).local_get(locals.state);
        elements_value(code, instruction, locals);
        // the lower pointer/result stay pending until both upper inputs and the
        // upper result are captured, so source/destination aliases remain safe.
        if upper {
            code.v128_store(vector(layout, instruction.destination(), true));
            code.v128_store(vector(layout, instruction.destination(), false));
        }
    }
    if instruction.width() == VectorWidth::Xmm {
        code.v128_store(vector(layout, instruction.destination(), false));
        code.local_get(locals.state)
            .v128_const(0)
            .v128_store(vector(layout, instruction.destination(), true));
    }
}

#[cfg(test)]
#[path = "shift/wasm.rs"]
mod tests;
