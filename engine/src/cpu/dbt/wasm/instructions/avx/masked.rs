use crate::{
    cpu::{
        dbt::wasm::instructions::avx::VectorLayout,
        x86::avx::{MaskedInstruction, VectorRegister},
    },
    memory::Access,
};
use wasm_encoder::{BlockType, InstructionSink, MemArg};
#[derive(Clone, Copy)]
pub(super) struct MaskedLocals {
    pub state: u32,
    pub operand: u32,
    pub mask: [u32; 2],
    pub data: [u32; 2],
    pub result: [u32; 2],
}
fn arg(offset: u32) -> MemArg {
    MemArg {
        offset: u64::from(offset),
        align: 0,
        memory_index: 0,
    }
}
fn vector(layout: VectorLayout, r: VectorRegister, half: usize) -> MemArg {
    arg(if half == 0 {
        layout.lower
    } else {
        layout.upper
    } + r.index() as u32 * 16)
}
fn selected(code: &mut InstructionSink<'_>, locals: MaskedLocals, start: usize, size: usize) {
    code.local_get(locals.mask[start / 16]);
    match size {
        1 => {
            code.i8x16_extract_lane_s((start % 16) as u8)
                .i32_const(0)
                .i32_lt_s();
        }
        4 => {
            code.i32x4_extract_lane((start % 16 / 4) as u8)
                .i32_const(0)
                .i32_lt_s();
        }
        8 => {
            code.i64x2_extract_lane((start % 16 / 8) as u8)
                .i64_const(0)
                .i64_lt_s();
        }
        _ => unreachable!("typed masked element width"),
    }
}
// preflight runs for every selected element before any operand read or publication.
// the caller owns guest segment/page authority and a single store transaction.
pub(super) fn emit<F>(
    code: &mut InstructionSink<'_>,
    instruction: MaskedInstruction,
    layout: VectorLayout,
    locals: MaskedLocals,
    mut preflight: F,
) where
    F: FnMut(&mut InstructionSink<'_>, u32, u32, Access),
{
    let width = instruction.width().bytes();
    let size = instruction.lane_bytes();
    for half in 0..width / 16 {
        code.local_get(locals.state)
            .v128_load(vector(layout, instruction.mask(), half))
            .local_set(locals.mask[half]);
    }
    if let MaskedInstruction::Store { source, .. } | MaskedInstruction::StoreBytes { source, .. } =
        instruction
    {
        for half in 0..width / 16 {
            code.local_get(locals.state)
                .v128_load(vector(layout, source, half))
                .local_set(locals.data[half]);
        }
    }
    for start in (0..width).step_by(size) {
        selected(code, locals, start, size);
        code.if_(BlockType::Empty);
        preflight(code, start as u32, size as u32, instruction.access());
        code.end();
    }
    if let MaskedInstruction::Load { destination, .. } = instruction {
        for half in 0..2 {
            code.v128_const(0).local_set(locals.result[half]);
        }
        for start in (0..width).step_by(size) {
            selected(code, locals, start, size);
            code.if_(BlockType::Empty);
            code.local_get(locals.result[start / 16])
                .local_get(locals.operand);
            match size {
                4 => {
                    code.i32_load(arg(start as u32))
                        .i32x4_replace_lane((start % 16 / 4) as u8);
                }
                8 => {
                    code.i64_load(arg(start as u32))
                        .i64x2_replace_lane((start % 16 / 8) as u8);
                }
                _ => unreachable!("typed masked load width"),
            }
            code.local_set(locals.result[start / 16]).end();
        }
        for half in 0..2 {
            code.local_get(locals.state)
                .local_get(locals.result[half])
                .v128_store(vector(layout, destination, half));
        }
    } else {
        for start in (0..width).step_by(size) {
            selected(code, locals, start, size);
            code.if_(BlockType::Empty);
            code.local_get(locals.operand)
                .local_get(locals.data[start / 16]);
            match size {
                1 => {
                    code.i8x16_extract_lane_u((start % 16) as u8)
                        .i32_store8(arg(start as u32));
                }
                4 => {
                    code.v128_store32_lane(arg(start as u32), (start % 16 / 4) as u8);
                }
                8 => {
                    code.v128_store64_lane(arg(start as u32), (start % 16 / 8) as u8);
                }
                _ => unreachable!("typed masked store width"),
            }
            code.end();
        }
    }
}
#[cfg(test)]
#[path = "masked/wasm.rs"]
mod tests;
