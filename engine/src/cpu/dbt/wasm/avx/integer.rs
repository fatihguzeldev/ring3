use super::VectorLayout;
use crate::cpu::x86::avx::{IntegerInstruction, MoveSource, VectorRegister};
use wasm_encoder::{InstructionSink, MemArg};

#[derive(Clone, Copy)]
pub(super) struct IntegerLocals {
    pub state: u32,
    pub operand: u32,
    pub scratch: [u32; 2],
}

fn arg(offset: u32) -> MemArg {
    MemArg {
        offset: u64::from(offset),
        align: 0,
        memory_index: 0,
    }
}

fn vector_arg(layout: VectorLayout, register: VectorRegister, upper: bool) -> MemArg {
    arg((if upper { layout.upper } else { layout.lower }) + register.index() as u32 * 16)
}

fn load(
    code: &mut InstructionSink<'_>,
    source: MoveSource,
    layout: VectorLayout,
    locals: IntegerLocals,
) {
    match source {
        MoveSource::Register(register) => {
            code.local_get(locals.state)
                .v128_load(vector_arg(layout, register, false));
        }
        MoveSource::Memory => {
            code.local_get(locals.operand).v128_load(arg(0));
        }
    }
}

// caller preflights full guest operands and supplies disjoint state/staging.
pub(super) fn emit(
    code: &mut InstructionSink<'_>,
    instruction: IntegerInstruction,
    layout: VectorLayout,
    locals: IntegerLocals,
) {
    code.local_get(locals.state);
    if let Some(first) = instruction.first() {
        load(code, MoveSource::Register(first), layout, locals);
    }
    load(code, instruction.second(), layout, locals);
    super::super::simd::integer_lanes(code, instruction.operation(), locals.scratch);
    code.v128_store(vector_arg(layout, instruction.destination(), false));
    code.local_get(locals.state)
        .v128_const(0)
        .v128_store(vector_arg(layout, instruction.destination(), true));
}

#[cfg(test)]
#[path = "integer/wasm.rs"]
mod tests;
