use crate::cpu::{
    dbt::wasm::instructions::avx::VectorLayout,
    x86::avx::{Element, MoveSource, PermuteInstruction, VectorRegister, VectorWidth, WordTarget},
};
use wasm_encoder::{InstructionSink, MemArg};
#[derive(Clone, Copy)]
pub(super) struct PermuteLayout {
    pub vectors: VectorLayout,
    pub general: u32,
}
#[derive(Clone, Copy)]
pub(super) struct PermuteLocals {
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
    layout: PermuteLayout,
    locals: PermuteLocals,
    source: MoveSource,
    half: usize,
) {
    match source {
        MoveSource::Register(register) => {
            code.local_get(locals.state)
                .v128_load(vector_arg(layout.vectors, register, half));
        }
        MoveSource::Memory => {
            code.local_get(locals.operand)
                .v128_load(arg(half as u32 * 16));
        }
    }
}
fn whole_indices(
    code: &mut InstructionSink<'_>,
    layout: PermuteLayout,
    locals: PermuteLocals,
    indices: VectorRegister,
    half: usize,
    source_half: usize,
) {
    load(code, layout, locals, MoveSource::Register(indices), half);
    let mask = std::array::from_fn(|byte| if byte % 4 == 0 { 7 } else { 0 });
    let repeat = std::array::from_fn(|byte| (byte / 4 * 4) as u8);
    let offsets = std::array::from_fn(|byte| (byte % 4) as u8);
    code.v128_const(i128::from_le_bytes(mask))
        .v128_and()
        .i32_const(2)
        .i32x4_shl()
        .v128_const(0)
        .i8x16_shuffle(repeat)
        .v128_const(i128::from_le_bytes(offsets))
        .i8x16_add();
    if source_half == 1 {
        // low-half indices wrap above 15 and swizzle to zero; high-half indices
        // become 0..15. the two swizzles therefore merge without overlap.
        code.i32_const(16).i8x16_splat().i8x16_sub();
    }
}

fn width(operation: PermuteInstruction) -> VectorWidth {
    use PermuteInstruction::*;
    match operation {
        Interleave { width, .. }
        | Shuffle { width, .. }
        | Immediate { width, .. }
        | Variable { width, .. }
        | Blend { width, .. }
        | BlendVariable { width, .. } => width,
        WholeDwords { .. } | WholeQwords { .. } => VectorWidth::Ymm,
        InsertSingle { .. } | ExtractSingle { .. } => VectorWidth::Xmm,
    }
}

// checked state/operand buffers are disjoint and the full requested guest span is
// preflighted by the caller. result halves are retained before any publication.
pub(super) fn emit(
    code: &mut InstructionSink<'_>,
    operation: PermuteInstruction,
    layout: PermuteLayout,
    locals: PermuteLocals,
) {
    use PermuteInstruction::*;
    if let ExtractSingle {
        source,
        destination,
        immediate,
    } = operation
    {
        match destination {
            WordTarget::Register(register) => {
                code.local_get(locals.state)
                    .i32_const((layout.general + register.index() as u32 * 4) as i32)
                    .i32_add();
            }
            WordTarget::Memory => {
                code.local_get(locals.operand);
            }
        }
        let mut source = vector_arg(layout.vectors, source, 0);
        source.offset += u64::from(immediate & 3) * 4;
        code.local_get(locals.state)
            .i32_load(source)
            .i32_store(arg(0));
        return;
    }
    let width = width(operation);
    let mut destination = None;
    for half in 0..if width == VectorWidth::Ymm { 2 } else { 1 } {
        let target = match operation {
            WholeDwords {
                destination,
                indices,
                source,
            } => {
                for source_half in 0..2 {
                    load(code, layout, locals, source, source_half);
                    whole_indices(code, layout, locals, indices, half, source_half);
                    code.i8x16_swizzle();
                }
                code.v128_or();
                destination
            }
            WholeQwords {
                destination,
                source,
                immediate,
            } => {
                load(code, layout, locals, source, 0);
                load(code, layout, locals, source, 1);
                let lanes = std::array::from_fn(|byte| {
                    let lane = half * 2 + byte / 8;
                    let selected = usize::from(immediate >> (lane * 2) & 3);
                    (selected * 8 + byte % 8) as u8
                });
                code.i8x16_shuffle(lanes);
                destination
            }
            Interleave {
                high,
                element,
                destination,
                first,
                second,
                ..
            } => {
                load(code, layout, locals, MoveSource::Register(first), half);
                load(code, layout, locals, second, half);
                let bytes = element.bytes();
                let lanes = std::array::from_fn(|byte| {
                    let lane = byte / bytes;
                    let source_lane = lane / 2 + if high { 8 / bytes } else { 0 };
                    (source_lane * bytes + byte % bytes + if lane % 2 == 0 { 0 } else { 16 }) as u8
                });
                code.i8x16_shuffle(lanes);
                destination
            }
            Shuffle {
                element,
                destination,
                first,
                second,
                immediate,
                ..
            } => {
                load(code, layout, locals, MoveSource::Register(first), half);
                load(code, layout, locals, second, half);
                let bytes = element.bytes();
                let lanes = std::array::from_fn(|byte| {
                    let lane = byte / bytes;
                    let selected = if element == Element::Single {
                        (immediate >> (lane * 2)) & 3
                    } else {
                        (immediate >> (half * 2 + lane)) & 1
                    };
                    (usize::from(selected) * bytes
                        + byte % bytes
                        + if lane < 8 / bytes { 0 } else { 16 }) as u8
                });
                code.i8x16_shuffle(lanes);
                destination
            }
            Immediate {
                element,
                destination,
                source,
                immediate,
                ..
            } => {
                load(code, layout, locals, source, half);
                code.v128_const(0);
                let bytes = element.bytes();
                let lanes = std::array::from_fn(|byte| {
                    let lane = byte / bytes;
                    let selected = if element == Element::Single {
                        (immediate >> (lane * 2)) & 3
                    } else {
                        (immediate >> (half * 2 + lane)) & 1
                    };
                    (usize::from(selected) * bytes + byte % bytes) as u8
                });
                code.i8x16_shuffle(lanes);
                destination
            }
            Variable {
                element,
                destination,
                source,
                control,
                ..
            } => {
                load(code, layout, locals, MoveSource::Register(source), half);
                load(code, layout, locals, control, half);
                let (mask, repeat, offsets): ([u8; 16], [u8; 16], [u8; 16]) = match element {
                    Element::Single => (
                        std::array::from_fn(|i| if i % 4 == 0 { 3 } else { 0 }),
                        std::array::from_fn(|i| (i / 4 * 4) as u8),
                        std::array::from_fn(|i| (i % 4) as u8),
                    ),
                    Element::Double => (
                        std::array::from_fn(|i| if i % 8 == 0 { 2 } else { 0 }),
                        std::array::from_fn(|i| (i / 8 * 8) as u8),
                        std::array::from_fn(|i| (i % 8) as u8),
                    ),
                };
                code.v128_const(i128::from_le_bytes(mask))
                    .v128_and()
                    .i32_const(2);
                match element {
                    Element::Single => {
                        code.i32x4_shl();
                    }
                    Element::Double => {
                        code.i64x2_shl();
                    }
                }
                code.v128_const(0)
                    .i8x16_shuffle(repeat)
                    .v128_const(i128::from_le_bytes(offsets))
                    .i8x16_add()
                    .i8x16_swizzle();
                destination
            }
            Blend {
                element,
                destination,
                first,
                second,
                immediate,
                ..
            } => {
                load(code, layout, locals, MoveSource::Register(first), half);
                load(code, layout, locals, second, half);
                let bytes = element.bytes();
                let lanes = std::array::from_fn(|byte| {
                    (byte
                        + if immediate & (1 << (half * 16 / bytes + byte / bytes)) == 0 {
                            0
                        } else {
                            16
                        }) as u8
                });
                code.i8x16_shuffle(lanes);
                destination
            }
            BlendVariable {
                element,
                destination,
                first,
                second,
                mask,
                ..
            } => {
                load(code, layout, locals, second, half);
                load(code, layout, locals, MoveSource::Register(first), half);
                load(code, layout, locals, MoveSource::Register(mask), half);
                code.i32_const(if element == Element::Single { 31 } else { 63 });
                match element {
                    Element::Single => {
                        code.i32x4_shr_s();
                    }
                    Element::Double => {
                        code.i64x2_shr_s();
                    }
                }
                code.v128_bitselect();
                destination
            }
            InsertSingle {
                destination,
                first,
                second,
                immediate,
            } => {
                load(code, layout, locals, MoveSource::Register(first), 0);
                match second {
                    MoveSource::Register(_) => {
                        load(code, layout, locals, second, 0);
                        code.i32x4_extract_lane(immediate >> 6);
                    }
                    MoveSource::Memory => {
                        code.local_get(locals.operand).i32_load(arg(0));
                    }
                }
                code.i32x4_replace_lane(immediate >> 4 & 3);
                let mask = std::array::from_fn(|byte| {
                    if immediate & (1 << (byte / 4)) == 0 {
                        0xff
                    } else {
                        0
                    }
                });
                code.v128_const(i128::from_le_bytes(mask)).v128_and();
                destination
            }
            ExtractSingle { .. } => unreachable!(),
        };
        destination = Some(target);
        code.local_set(locals.result[half]);
    }
    let destination = destination.unwrap();
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

#[cfg(test)]
#[path = "permute/wasm.rs"]
mod tests;
