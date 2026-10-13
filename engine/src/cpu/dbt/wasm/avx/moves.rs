use super::VectorLayout;
use crate::cpu::x86::{
    Register32,
    avx::{
        DuplicateKind, Element, Half, HalfMerge, MoveInstruction, MoveSource, VectorRegister,
        VectorWidth, WordSource, WordTarget,
    },
};
use wasm_encoder::{InstructionSink, MemArg};

#[derive(Clone, Copy)]
pub(super) struct MoveLayout {
    pub vectors: VectorLayout,
    pub general: u32,
    pub mxcsr: u32,
}

#[derive(Clone, Copy)]
pub(super) struct MoveLocals {
    pub state: u32,
    pub operand: u32,
    pub result: [u32; 2],
    pub control: u32,
}

fn arg(offset: u32) -> MemArg {
    MemArg {
        offset: u64::from(offset),
        align: 0,
        memory_index: 0,
    }
}
fn vector_arg(layout: VectorLayout, register: VectorRegister, half: usize) -> MemArg {
    let base = if half == 0 {
        layout.lower
    } else {
        layout.upper
    };
    arg(base + register.index() as u32 * 16)
}
fn general_arg(layout: MoveLayout, register: Register32) -> MemArg {
    arg(layout.general + register.index() as u32 * 4)
}
fn load_vector(
    code: &mut InstructionSink<'_>,
    layout: MoveLayout,
    locals: MoveLocals,
    source: VectorRegister,
    half: usize,
) {
    code.local_get(locals.state)
        .v128_load(vector_arg(layout.vectors, source, half));
}
fn publish(
    code: &mut InstructionSink<'_>,
    layout: MoveLayout,
    locals: MoveLocals,
    destination: VectorRegister,
    width: VectorWidth,
) {
    for half in 0..2 {
        code.local_get(locals.state);
        if half == 0 || width == VectorWidth::Ymm {
            code.local_get(locals.result[half]);
        } else {
            code.v128_const(0);
        }
        code.v128_store(vector_arg(layout.vectors, destination, half));
    }
}
fn load_source(
    code: &mut InstructionSink<'_>,
    layout: MoveLayout,
    locals: MoveLocals,
    source: MoveSource,
    half: usize,
    narrow: bool,
) {
    match source {
        MoveSource::Register(register) => load_vector(code, layout, locals, register, half),
        MoveSource::Memory => {
            code.local_get(locals.operand);
            if narrow {
                code.v128_load64_zero(arg(0));
            } else {
                code.v128_load(arg(half as u32 * 16));
            }
        }
    }
}

// the caller preflights the full declared guest span and supplies disjoint checked
// state/operand buffers. no publication may precede that segment/page check. the
// invalid-control branch targets the caller's existing fault-delivery block.
pub(super) fn emit(
    code: &mut InstructionSink<'_>,
    operation: MoveInstruction,
    layout: MoveLayout,
    locals: MoveLocals,
    invalid_control_depth: u32,
) {
    use MoveInstruction::*;
    let destination = match operation {
        Copy {
            width,
            destination,
            source,
        } => {
            for half in 0..if width == VectorWidth::Ymm { 2 } else { 1 } {
                load_vector(code, layout, locals, source, half);
                code.local_set(locals.result[half]);
            }
            Some((destination, width))
        }
        Load {
            width, destination, ..
        } => {
            for half in 0..if width == VectorWidth::Ymm { 2 } else { 1 } {
                code.local_get(locals.operand)
                    .v128_load(arg(half as u32 * 16))
                    .local_set(locals.result[half]);
            }
            Some((destination, width))
        }
        Store { width, source, .. } => {
            let halves = if width == VectorWidth::Ymm { 2 } else { 1 };
            for half in 0..halves {
                load_vector(code, layout, locals, source, half);
                code.local_set(locals.result[half]);
            }
            for half in 0..halves {
                code.local_get(locals.operand)
                    .local_get(locals.result[half])
                    .v128_store(arg(half as u32 * 16));
            }
            None
        }
        Scalar {
            element,
            destination,
            merge,
            source,
        } => {
            load_vector(code, layout, locals, merge, 0);
            load_vector(code, layout, locals, source, 0);
            match element {
                Element::Single => {
                    code.i32x4_extract_lane(0).i32x4_replace_lane(0);
                }
                Element::Double => {
                    code.i64x2_extract_lane(0).i64x2_replace_lane(0);
                }
            }
            code.local_set(locals.result[0]);
            Some((destination, VectorWidth::Xmm))
        }
        LoadScalar {
            element,
            destination,
        } => {
            code.local_get(locals.operand);
            match element {
                Element::Single => {
                    code.v128_load32_zero(arg(0));
                }
                Element::Double => {
                    code.v128_load64_zero(arg(0));
                }
            }
            code.local_set(locals.result[0]);
            Some((destination, VectorWidth::Xmm))
        }
        StoreScalar { element, source } => {
            code.local_get(locals.operand);
            load_vector(code, layout, locals, source, 0);
            match element {
                Element::Single => {
                    code.v128_store32_lane(arg(0), 0);
                }
                Element::Double => {
                    code.v128_store64_lane(arg(0), 0);
                }
            }
            None
        }
        LoadHalf {
            half,
            destination,
            merge,
        } => {
            load_vector(code, layout, locals, merge, 0);
            code.local_get(locals.operand).v128_load64_zero(arg(0));
            let lanes = match half {
                Half::Low => [16, 17, 18, 19, 20, 21, 22, 23, 8, 9, 10, 11, 12, 13, 14, 15],
                Half::High => [0, 1, 2, 3, 4, 5, 6, 7, 16, 17, 18, 19, 20, 21, 22, 23],
            };
            code.i8x16_shuffle(lanes).local_set(locals.result[0]);
            Some((destination, VectorWidth::Xmm))
        }
        StoreHalf { half, source } => {
            code.local_get(locals.operand);
            load_vector(code, layout, locals, source, 0);
            code.v128_store64_lane(arg(0), if half == Half::Low { 0 } else { 1 });
            None
        }
        MergeHalf {
            kind,
            destination,
            merge,
            source,
        } => {
            load_vector(code, layout, locals, merge, 0);
            load_vector(code, layout, locals, source, 0);
            let lanes = match kind {
                HalfMerge::HighToLow => {
                    [24, 25, 26, 27, 28, 29, 30, 31, 8, 9, 10, 11, 12, 13, 14, 15]
                }
                HalfMerge::LowToHigh => [0, 1, 2, 3, 4, 5, 6, 7, 16, 17, 18, 19, 20, 21, 22, 23],
            };
            code.i8x16_shuffle(lanes).local_set(locals.result[0]);
            Some((destination, VectorWidth::Xmm))
        }
        Duplicate {
            kind,
            width,
            destination,
            source,
        } => {
            let lanes = match kind {
                DuplicateKind::SingleLow => [0, 1, 2, 3, 0, 1, 2, 3, 8, 9, 10, 11, 8, 9, 10, 11],
                DuplicateKind::SingleHigh => {
                    [4, 5, 6, 7, 4, 5, 6, 7, 12, 13, 14, 15, 12, 13, 14, 15]
                }
                DuplicateKind::DoubleLow => [0, 1, 2, 3, 4, 5, 6, 7, 0, 1, 2, 3, 4, 5, 6, 7],
            };
            for half in 0..if width == VectorWidth::Ymm { 2 } else { 1 } {
                load_source(
                    code,
                    layout,
                    locals,
                    source,
                    half,
                    kind == DuplicateKind::DoubleLow && width == VectorWidth::Xmm,
                );
                code.v128_const(0)
                    .i8x16_shuffle(lanes)
                    .local_set(locals.result[half]);
            }
            Some((destination, width))
        }
        DwordLoad {
            destination,
            source,
        } => {
            match source {
                WordSource::Memory => {
                    code.local_get(locals.operand).v128_load32_zero(arg(0));
                }
                WordSource::Register(register) => {
                    code.v128_const(0)
                        .local_get(locals.state)
                        .i32_load(general_arg(layout, register))
                        .i32x4_replace_lane(0);
                }
            }
            code.local_set(locals.result[0]);
            Some((destination, VectorWidth::Xmm))
        }
        DwordStore {
            source,
            destination,
        } => {
            match destination {
                WordTarget::Memory => {
                    code.local_get(locals.operand);
                }
                WordTarget::Register(register) => {
                    code.local_get(locals.state)
                        .i32_const((layout.general + register.index() as u32 * 4) as i32)
                        .i32_add();
                }
            }
            code.local_get(locals.state)
                .i32_load(vector_arg(layout.vectors, source, 0))
                .i32_store(arg(0));
            None
        }
        QwordLoad {
            destination,
            source,
        } => {
            match source {
                MoveSource::Memory => {
                    code.local_get(locals.operand).v128_load64_zero(arg(0));
                }
                MoveSource::Register(register) => {
                    code.local_get(locals.state).v128_load64_zero(vector_arg(
                        layout.vectors,
                        register,
                        0,
                    ));
                }
            }
            code.local_set(locals.result[0]);
            Some((destination, VectorWidth::Xmm))
        }
        QwordStore { source } => {
            code.local_get(locals.operand);
            load_vector(code, layout, locals, source, 0);
            code.v128_store64_lane(arg(0), 0);
            None
        }
        Mask {
            element,
            width,
            destination,
            source,
        } => {
            load_vector(code, layout, locals, source, 0);
            match element {
                Element::Single => {
                    code.i32x4_bitmask();
                }
                Element::Double => {
                    code.i64x2_bitmask();
                }
            }
            code.local_set(locals.control);
            if width == VectorWidth::Ymm {
                load_vector(code, layout, locals, source, 1);
                match element {
                    Element::Single => {
                        code.i32x4_bitmask();
                    }
                    Element::Double => {
                        code.i64x2_bitmask();
                    }
                }
                code.i32_const(if element == Element::Single { 4 } else { 2 })
                    .i32_shl()
                    .local_get(locals.control)
                    .i32_or()
                    .local_set(locals.control);
            }
            code.local_get(locals.state)
                .local_get(locals.control)
                .i32_store(general_arg(layout, destination));
            None
        }
        LoadControl => {
            code.local_get(locals.operand)
                .i32_load(arg(0))
                .local_tee(locals.control)
                .i32_const(!0xffff)
                .i32_and()
                .br_if(invalid_control_depth);
            code.local_get(locals.state)
                .local_get(locals.control)
                .i32_store(arg(layout.mxcsr));
            None
        }
        StoreControl => {
            code.local_get(locals.operand)
                .local_get(locals.state)
                .i32_load(arg(layout.mxcsr))
                .i32_store(arg(0));
            None
        }
        ZeroUpper | ZeroAll => {
            for index in 0..8 {
                if operation == ZeroAll {
                    code.local_get(locals.state)
                        .v128_const(0)
                        .v128_store(arg(layout.vectors.lower + index * 16));
                }
                code.local_get(locals.state)
                    .v128_const(0)
                    .v128_store(arg(layout.vectors.upper + index * 16));
            }
            None
        }
    };
    if let Some((destination, width)) = destination {
        publish(code, layout, locals, destination, width);
    }
}

#[cfg(test)]
mod tests;
