use crate::cpu::x86::avx::{
    LogicalInstruction, LogicalOperation, MoveSource, VectorRegister, VectorWidth,
};
use wasm_encoder::{InstructionSink, MemArg};

use super::VectorLayout;

fn offset(base: u32, register: VectorRegister) -> MemArg {
    MemArg {
        offset: u64::from(base) + register.index() as u64 * 16,
        align: 0,
        memory_index: 0,
    }
}

// caller supplies a checked exact16/32-byte staging span for memory sources.
pub(super) fn emit(
    code: &mut InstructionSink<'_>,
    operation: LogicalInstruction,
    layout: VectorLayout,
    state_pointer: u32,
    operand_pointer: u32,
    result_locals: [u32; 2],
) {
    let halves = if operation.width == VectorWidth::Ymm {
        2
    } else {
        1
    };
    for (half, base) in [layout.lower, layout.upper]
        .into_iter()
        .enumerate()
        .take(halves)
    {
        code.local_get(state_pointer)
            .v128_load(offset(base, operation.first));
        if operation.kind == LogicalOperation::AndNot {
            code.v128_not();
        }
        match operation.second {
            MoveSource::Register(register) => {
                code.local_get(state_pointer)
                    .v128_load(offset(base, register));
            }
            MoveSource::Memory => {
                code.local_get(operand_pointer).v128_load(MemArg {
                    offset: (half * 16) as u64,
                    align: 0,
                    memory_index: 0,
                });
            }
        }
        match operation.kind {
            LogicalOperation::And | LogicalOperation::AndNot => {
                code.v128_and();
            }
            LogicalOperation::Or => {
                code.v128_or();
            }
            LogicalOperation::Xor => {
                code.v128_xor();
            }
        }
        code.local_set(result_locals[half]);
    }
    // retain both computed halves before publishing any aliased destination.
    for (half, base) in [layout.lower, layout.upper].into_iter().enumerate() {
        code.local_get(state_pointer);
        if half < halves {
            code.local_get(result_locals[half]);
        } else {
            code.v128_const(0);
        }
        code.v128_store(offset(base, operation.destination));
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
#[path = "logical/memory/wasm.rs"]
mod memory;
