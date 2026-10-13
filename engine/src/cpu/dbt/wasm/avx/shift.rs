use super::VectorLayout;
use crate::cpu::x86::avx::{ShiftCount, ShiftInstruction, VectorRegister};
use wasm_encoder::{InstructionSink, MemArg};
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
    };
    code.local_get(locals.state);
    super::super::simd::shift_value(code, instruction.operation(), immediate, locals.sources);
    code.v128_store(vector(layout, instruction.destination(), false));
    code.local_get(locals.state)
        .v128_const(0)
        .v128_store(vector(layout, instruction.destination(), true));
}
#[cfg(test)]
#[path = "shift/wasm.rs"]
mod tests;
