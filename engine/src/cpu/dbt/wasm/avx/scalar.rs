use super::VectorLayout;
use crate::cpu::x86::{
    Register32,
    avx::{ScalarInstruction, VectorRegister, VectorWidth, WordSource, WordTarget},
    simd::{ScalarOperation, ScalarWidth},
};
use wasm_encoder::{InstructionSink, MemArg};
#[derive(Clone, Copy)]
pub(super) struct ScalarLayout {
    pub vectors: VectorLayout,
    pub general: u32,
}
#[derive(Clone, Copy)]
pub(super) struct ScalarLocals {
    pub state: u32,
    pub operand: u32,
    pub vector: u32,
    pub scalar: u32,
}
fn arg(offset: u32) -> MemArg {
    MemArg {
        offset: u64::from(offset),
        align: 0,
        memory_index: 0,
    }
}
fn vector(layout: ScalarLayout, r: VectorRegister, upper: bool) -> MemArg {
    arg(if upper {
        layout.vectors.upper
    } else {
        layout.vectors.lower
    } + r.index() as u32 * 16)
}
fn general(layout: ScalarLayout, r: Register32) -> MemArg {
    arg(layout.general + r.index() as u32 * 4)
}
fn load_memory(code: &mut InstructionSink<'_>, width: ScalarWidth) {
    match width {
        ScalarWidth::Byte => {
            code.i32_load8_u(arg(0));
        }
        ScalarWidth::Word => {
            code.i32_load16_u(arg(0));
        }
        ScalarWidth::Dword => {
            code.i32_load(arg(0));
        }
    }
}
// caller preflights exact read/write authority before any state/RAM publication.
pub(super) fn emit(
    code: &mut InstructionSink<'_>,
    instruction: ScalarInstruction,
    layout: ScalarLayout,
    locals: ScalarLocals,
) {
    match instruction {
        ScalarInstruction::Insert {
            width,
            destination,
            merge,
            source,
            immediate,
        } => {
            code.local_get(locals.state)
                .v128_load(vector(layout, merge, false))
                .local_set(locals.vector);
            match source {
                WordSource::Register(r) => {
                    code.local_get(locals.state).i32_load(general(layout, r));
                }
                WordSource::Memory => {
                    code.local_get(locals.operand);
                    load_memory(code, width);
                }
            }
            code.local_set(locals.scalar)
                .local_get(locals.state)
                .local_get(locals.vector)
                .local_get(locals.scalar);
            super::super::simd::scalar_value(code, ScalarOperation::Insert { width, immediate });
            code.v128_store(vector(layout, destination, false));
            code.local_get(locals.state)
                .v128_const(0)
                .v128_store(vector(layout, destination, true));
        }
        ScalarInstruction::Extract {
            width,
            source,
            destination,
            immediate,
        } => {
            code.local_get(locals.state)
                .v128_load(vector(layout, source, false));
            super::super::simd::scalar_value(code, ScalarOperation::Extract { width, immediate });
            code.local_set(locals.scalar);
            match destination {
                WordTarget::Register(r) => {
                    code.local_get(locals.state)
                        .local_get(locals.scalar)
                        .i32_store(general(layout, r));
                }
                WordTarget::Memory => {
                    code.local_get(locals.operand).local_get(locals.scalar);
                    match width {
                        ScalarWidth::Byte => {
                            code.i32_store8(arg(0));
                        }
                        ScalarWidth::Word => {
                            code.i32_store16(arg(0));
                        }
                        ScalarWidth::Dword => {
                            code.i32_store(arg(0));
                        }
                    }
                }
            }
        }
        ScalarInstruction::Mask {
            width,
            source,
            destination,
        } => {
            code.local_get(locals.state)
                .v128_load(vector(layout, source, false));
            super::super::simd::scalar_value(code, ScalarOperation::MaskBytes);
            if width == VectorWidth::Ymm {
                code.local_get(locals.state)
                    .v128_load(vector(layout, source, true));
                super::super::simd::scalar_value(code, ScalarOperation::MaskBytes);
                code.i32_const(16).i32_shl().i32_or();
            }
            code.local_set(locals.scalar);
            code.local_get(locals.state)
                .local_get(locals.scalar)
                .i32_store(general(layout, destination));
        }
    }
}
#[cfg(test)]
#[path = "scalar/wasm.rs"]
mod tests;
