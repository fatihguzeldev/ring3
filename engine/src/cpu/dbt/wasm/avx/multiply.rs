use super::VectorLayout;
use crate::cpu::x86::avx::{MoveSource, MultiplyInstruction, VectorRegister};
use wasm_encoder::{InstructionSink, MemArg};
#[derive(Clone, Copy)]
pub(super) struct MultiplyLocals {
    pub state: u32,
    pub operand: u32,
    pub sources: [u32; 2],
    pub scratch: [u32; 3],
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
// caller preflights the full guest operand before entering this lowerer.
pub(super) fn emit(
    code: &mut InstructionSink<'_>,
    instruction: MultiplyInstruction,
    layout: VectorLayout,
    locals: MultiplyLocals,
) {
    let [first, second] = locals.sources;
    if let Some(r) = instruction.first() {
        code.local_get(locals.state)
            .v128_load(vector(layout, r, false))
            .local_set(first);
    }
    match instruction.second() {
        MoveSource::Register(r) => {
            code.local_get(locals.state)
                .v128_load(vector(layout, r, false));
        }
        MoveSource::Memory => {
            code.local_get(locals.operand).v128_load(arg(0));
        }
    }
    code.local_set(second).local_get(locals.state);
    super::super::simd::multiply_value(
        code,
        instruction.operation(),
        locals.sources,
        locals.scratch,
    );
    code.v128_store(vector(layout, instruction.destination(), false));
    code.local_get(locals.state)
        .v128_const(0)
        .v128_store(vector(layout, instruction.destination(), true));
}
#[cfg(test)]
#[path = "multiply/wasm.rs"]
mod tests;
