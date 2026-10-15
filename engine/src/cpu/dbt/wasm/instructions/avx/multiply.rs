use crate::cpu::{
    dbt::wasm::instructions::avx::VectorLayout,
    x86::avx::{MoveSource, MultiplyInstruction, VectorRegister, VectorWidth},
};
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
    for upper in [false, true]
        .into_iter()
        .take(instruction.width().bytes() / 16)
    {
        if let Some(r) = instruction.first() {
            code.local_get(locals.state)
                .v128_load(vector(layout, r, upper))
                .local_set(first);
        }
        match instruction.second() {
            MoveSource::Register(r) => {
                code.local_get(locals.state)
                    .v128_load(vector(layout, r, upper));
            }
            MoveSource::Memory => {
                code.local_get(locals.operand)
                    .v128_load(arg(if upper { 16 } else { 0 }));
            }
        }
        code.local_set(second).local_get(locals.state);
        crate::cpu::dbt::wasm::instructions::simd::multiply_value(
            code,
            instruction.operation_for_half(upper),
            locals.sources,
            locals.scratch,
        );
        // capture the upper inputs/result before either pending result is stored.
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
#[path = "multiply/wasm.rs"]
mod tests;
