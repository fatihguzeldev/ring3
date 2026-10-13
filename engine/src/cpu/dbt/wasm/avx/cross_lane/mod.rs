use super::VectorLayout;
use crate::cpu::x86::avx::{
    BlockTarget, CrossLaneInstruction, MoveSource, VectorRegister, VectorWidth,
};
use wasm_encoder::{InstructionSink, MemArg};

#[derive(Clone, Copy)]
pub(super) struct CrossLaneLocals {
    pub state: u32,
    pub operand: u32,
    pub result: [u32; 2],
}
fn arg(offset: u32) -> MemArg {
    MemArg {
        offset: u64::from(offset),
        align: 0,
        memory_index: 0,
    }
}
fn vector_arg(layout: VectorLayout, register: VectorRegister, half: usize) -> MemArg {
    arg((if half == 0 {
        layout.lower
    } else {
        layout.upper
    }) + register.index() as u32 * 16)
}
fn load(
    code: &mut InstructionSink<'_>,
    layout: VectorLayout,
    locals: CrossLaneLocals,
    source: MoveSource,
    half: usize,
) {
    match source {
        MoveSource::Register(register) => {
            code.local_get(locals.state)
                .v128_load(vector_arg(layout, register, half));
        }
        MoveSource::Memory => {
            code.local_get(locals.operand)
                .v128_load(arg(half as u32 * 16));
        }
    }
}

// the caller owns exact operand preflight; both result halves are captured before publication.
pub(super) fn emit(
    code: &mut InstructionSink<'_>,
    operation: CrossLaneInstruction,
    layout: VectorLayout,
    locals: CrossLaneLocals,
) {
    use CrossLaneInstruction::*;
    if let Extract {
        source,
        destination,
        immediate,
    } = operation
    {
        load(
            code,
            layout,
            locals,
            MoveSource::Register(source),
            usize::from(immediate & 1),
        );
        code.local_set(locals.result[0]);
        match destination {
            BlockTarget::Register(register) => {
                code.local_get(locals.state)
                    .local_get(locals.result[0])
                    .v128_store(vector_arg(layout, register, 0));
                code.local_get(locals.state)
                    .v128_const(0)
                    .v128_store(vector_arg(layout, register, 1));
            }
            BlockTarget::Memory => {
                code.local_get(locals.operand)
                    .local_get(locals.result[0])
                    .v128_store(arg(0));
            }
        }
        return;
    }
    let (destination, width) = match operation {
        BroadcastSingle { destination, width } => {
            code.local_get(locals.operand)
                .i32_load(arg(0))
                .i32x4_splat()
                .local_set(locals.result[0]);
            if width == VectorWidth::Ymm {
                code.local_get(locals.result[0]).local_set(locals.result[1]);
            }
            (destination, width)
        }
        BroadcastDouble { destination } => {
            code.local_get(locals.operand)
                .i64_load(arg(0))
                .i64x2_splat()
                .local_set(locals.result[0]);
            code.local_get(locals.result[0]).local_set(locals.result[1]);
            (destination, VectorWidth::Ymm)
        }
        BroadcastBlock { destination } => {
            code.local_get(locals.operand)
                .v128_load(arg(0))
                .local_set(locals.result[0]);
            code.local_get(locals.result[0]).local_set(locals.result[1]);
            (destination, VectorWidth::Ymm)
        }
        Insert {
            destination,
            first,
            second,
            immediate,
        } => {
            for half in 0..2 {
                if half == usize::from(immediate & 1) {
                    load(code, layout, locals, second, 0);
                } else {
                    load(code, layout, locals, MoveSource::Register(first), half);
                }
                code.local_set(locals.result[half]);
            }
            (destination, VectorWidth::Ymm)
        }
        Permute {
            destination,
            first,
            second,
            immediate,
        } => {
            for half in 0..2 {
                let control = immediate >> (half * 4);
                if control & 8 != 0 {
                    code.v128_const(0);
                } else {
                    load(
                        code,
                        layout,
                        locals,
                        if control & 2 == 0 {
                            MoveSource::Register(first)
                        } else {
                            second
                        },
                        usize::from(control & 1),
                    );
                }
                code.local_set(locals.result[half]);
            }
            (destination, VectorWidth::Ymm)
        }
        Extract { .. } => unreachable!(),
    };
    for half in 0..2 {
        code.local_get(locals.state);
        if half == 0 || width == VectorWidth::Ymm {
            code.local_get(locals.result[half]);
        } else {
            code.v128_const(0);
        }
        code.v128_store(vector_arg(layout, destination, half));
    }
}

#[cfg(test)]
mod wasm;
