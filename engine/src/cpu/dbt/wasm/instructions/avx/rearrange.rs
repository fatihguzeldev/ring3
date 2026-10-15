use crate::cpu::{
    dbt::wasm::instructions::avx::VectorLayout,
    x86::avx::{MoveSource, RearrangeInstruction, VectorRegister, VectorWidth},
};
use wasm_encoder::{InstructionSink, MemArg};
#[derive(Clone, Copy)]
pub(super) struct RearrangeLocals {
    pub state: u32,
    pub operand: u32,
    pub sources: [u32; 3],
}

fn arg(offset: u32) -> MemArg {
    MemArg {
        offset: u64::from(offset),
        align: 0,
        memory_index: 0,
    }
}
fn vector(layout: VectorLayout, register: VectorRegister, upper: bool) -> MemArg {
    arg(if upper { layout.upper } else { layout.lower } + register.index() as u32 * 16)
}
fn load(
    code: &mut InstructionSink<'_>,
    source: MoveSource,
    bytes: u8,
    layout: VectorLayout,
    locals: RearrangeLocals,
    upper: bool,
) {
    match source {
        MoveSource::Register(r) => {
            code.local_get(locals.state)
                .v128_load(vector(layout, r, upper));
        }
        MoveSource::Memory => {
            code.local_get(locals.operand);
            match bytes {
                2 => {
                    code.i32_load16_u(arg(0))
                        .i32x4_splat()
                        .v128_const(0xffff)
                        .v128_and();
                }
                4 => {
                    code.v128_load32_zero(arg(0));
                }
                8 => {
                    code.v128_load64_zero(arg(0));
                }
                16 => {
                    code.v128_load(arg(0));
                }
                32 => {
                    code.v128_load(arg(if upper { 16 } else { 0 }));
                }
                _ => unreachable!("validated rearrange width"),
            }
        }
    }
}

// caller preflights exact guest spans and supplies disjoint staging/state.
pub(super) fn emit(
    code: &mut InstructionSink<'_>,
    instruction: RearrangeInstruction,
    layout: VectorLayout,
    locals: RearrangeLocals,
) {
    let [first, second, mask] = locals.sources;
    if let Some(r) = instruction.first() {
        load(code, MoveSource::Register(r), 16, layout, locals, false);
        code.local_set(first);
    }
    load(
        code,
        instruction.second(),
        instruction.source_bytes(),
        layout,
        locals,
        false,
    );
    code.local_set(second);
    if let Some(r) = instruction.mask() {
        load(code, MoveSource::Register(r), 16, layout, locals, false);
        code.local_set(mask);
    }
    code.local_get(locals.state);
    crate::cpu::dbt::wasm::instructions::simd::rearrange_value(
        code,
        instruction.operation(),
        locals.sources,
    );
    if instruction.width() == VectorWidth::Ymm {
        if instruction.operation().kind().widening().is_some() {
            // widening consumes the next window in the captured low-xmm source.
            let offset = instruction.source_bytes() / 2;
            code.local_get(second)
                .v128_const(0)
                .i8x16_shuffle(std::array::from_fn(|byte| offset + byte as u8))
                .local_set(second);
        } else {
            // the lower result stays on the stack until all upper sources are captured.
            if let Some(r) = instruction.first() {
                load(code, MoveSource::Register(r), 16, layout, locals, true);
                code.local_set(first);
            }
            load(code, instruction.second(), 32, layout, locals, true);
            code.local_set(second);
            if let Some(r) = instruction.mask() {
                load(code, MoveSource::Register(r), 16, layout, locals, true);
                code.local_set(mask);
            }
        }
        code.local_get(locals.state);
        crate::cpu::dbt::wasm::instructions::simd::rearrange_value(
            code,
            instruction.operation(),
            locals.sources,
        );
        code.v128_store(vector(layout, instruction.destination(), true));
        code.v128_store(vector(layout, instruction.destination(), false));
        return;
    }
    code.v128_store(vector(layout, instruction.destination(), false));
    code.local_get(locals.state)
        .v128_const(0)
        .v128_store(vector(layout, instruction.destination(), true));
}

#[cfg(test)]
#[path = "rearrange/wasm.rs"]
mod tests;
