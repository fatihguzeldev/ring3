use super::super::{locals::*, memory::Imports};
use crate::{
    abi::{
        arena::TRANSFER_OFFSET,
        tasks::TASK_CONTEXT_SIZE,
        x86::{LEGACY_FLAGS_MASK, USER_FLAGS_MASK, USER_FLAGS_MASK_V3},
    },
    cpu::x86::SegmentRegister,
};
use wasm_encoder::{
    BlockType, ConstExpr, GlobalSection, GlobalType, InstructionSink, MemArg, ValType,
};

const SELECTORS: u32 = 3;
const BASES: u32 = 9;
const CACHES: u32 = 15;
const LOADED: u32 = 21;
const REVISION: u32 = 22;

#[derive(Clone, Copy)]
pub(in crate::cpu::dbt::wasm) struct RawTaskContextImports {
    pub(in crate::cpu::dbt::wasm) read: u32,
    pub(in crate::cpu::dbt::wasm) status: u32,
    pub(in crate::cpu::dbt::wasm) scratch: u32,
}
pub(in crate::cpu::dbt::wasm) fn globals(globals: &mut GlobalSection, revision: u64) {
    for _ in 3..22 {
        globals.global(
            GlobalType {
                val_type: ValType::I32,
                mutable: true,
                shared: false,
            },
            &ConstExpr::i32_const(0),
        );
    }
    globals.global(
        GlobalType {
            val_type: ValType::I64,
            mutable: false,
            shared: false,
        },
        &ConstExpr::i64_const(revision as i64),
    );
}
pub(in crate::cpu::dbt::wasm) fn refresh(code: &mut InstructionSink<'_>, imports: Imports) {
    let Some(raw) = imports.raw_task_context else {
        return;
    };
    code.call(raw.read)
        .local_tee(raw.status)
        .i32_eqz()
        .i32_eqz();
    fail(code);
    code.local_get(STATE_PTR)
        .i64_extend_i32_u()
        .i64_const((TRANSFER_OFFSET + TASK_CONTEXT_SIZE) as i64)
        .i64_add()
        .local_get(MEMORY_BYTES)
        .i64_gt_u();
    fail(code);
    for (offset, value) in [
        (0, u32::from_le_bytes(*b"R3TC")),
        (4, 0x10001),
        (8, 184),
        (12, 0),
        (16, u32::from_le_bytes(*b"R3ST")),
        (24, 56),
    ] {
        field(code, offset);
        code.i32_const(value as i32).i32_ne();
        fail(code);
    }
    // a partial compiled unit is bound to one exact existing task revision.
    code.global_get(REVISION).i64_eqz();
    fail(code);
    code.local_get(STATE_PTR)
        .i64_load(arg(176, 3))
        .global_get(REVISION)
        .i64_ne();
    fail(code);
    field(code, 20);
    code.i32_const(0xffff0000u32 as i32)
        .i32_and()
        .i32_const(0x10000)
        .i32_ne();
    fail(code);
    field(code, 20);
    code.i32_const(0xffff)
        .i32_and()
        .local_tee(raw.status)
        .i32_const(1)
        .i32_sub()
        .i32_const(2)
        .i32_gt_u();
    fail(code);
    field(code, 28);
    code.i32_const(1).i32_gt_u();
    fail(code);
    field(code, 68);
    code.i32_const(2).i32_and().i32_eqz();
    fail(code);
    field(code, 68);
    code.local_get(raw.status)
        .i32_const(1)
        .i32_eq()
        .if_(BlockType::Result(ValType::I32))
        .i32_const(!LEGACY_FLAGS_MASK as i32)
        .else_()
        .local_get(raw.status)
        .i32_const(2)
        .i32_eq()
        .if_(BlockType::Result(ValType::I32))
        .i32_const(!USER_FLAGS_MASK as i32)
        .else_()
        .i32_const(!USER_FLAGS_MASK_V3 as i32)
        .end()
        .end()
        .i32_and();
    fail(code);
    field(code, 72);
    code.local_tee(raw.scratch).i32_const(!0x3f).i32_and();
    fail(code);
    code.local_get(raw.scratch).i32_const(2).i32_and().i32_eqz();
    fail(code);
    code.local_get(raw.scratch).global_set(LOADED);
    field(code, 76);
    code.i32_const(!0x7f).i32_and();
    fail(code);
    field(code, 76);
    code.i32_const(0x1f).i32_and().i32_const(0x1f).i32_ne();
    fail(code);
    field(code, 88);
    code.i32_const(0x3f03).i32_ne();
    fail(code);
    field(code, 92);
    code.i32_const(0xffff).i32_gt_u();
    fail(code);
    for index in 0..6 {
        field(code, 96 + index * 4);
        code.local_tee(raw.status).i32_const(0xffff).i32_gt_u();
        fail(code);
        code.local_get(raw.status).global_set(SELECTORS + index);
        field(code, 120 + index * 4);
        code.global_set(BASES + index);
        field(code, 144 + index * 4);
        code.global_set(CACHES + index);
        code.global_get(LOADED)
            .i32_const(1 << index)
            .i32_and()
            .i32_eqz()
            .if_(BlockType::Empty);
        code.global_get(BASES + index)
            .global_get(CACHES + index)
            .i32_or();
        fail(code);
        code.else_()
            .global_get(SELECTORS + index)
            .i32_const(0xfffc)
            .i32_and()
            .i32_eqz()
            .if_(BlockType::Empty);
        if index == 1 || index == 2 {
            code.i32_const(1);
            fail(code);
        }
        code.global_get(BASES + index)
            .global_get(CACHES + index)
            .i32_or();
        fail(code);
        code.else_();
        code.global_get(CACHES + index)
            .i32_const(0x94100000u32 as i32)
            .i32_and()
            .i32_const(0x90100000u32 as i32)
            .i32_ne();
        fail(code);
        match index {
            1 => {
                mask(code, index, 0x00800000, 0x00800000);
                code.global_get(SELECTORS + index)
                    .i32_const(3)
                    .i32_and()
                    .i32_const(3)
                    .i32_ne();
                fail(code);
                code.global_get(CACHES + index)
                    .i32_const(0x00400000)
                    .i32_and()
                    .i32_eqz()
                    .if_(BlockType::Empty);
                mask(code, index, 0x60000000, 0x60000000);
                code.end();
            }
            2 => {
                mask(code, index, 0x00a00000, 0x00200000);
                mask(code, index, 0x60000000, 0x60000000);
                code.global_get(SELECTORS + index)
                    .i32_const(3)
                    .i32_and()
                    .i32_const(3)
                    .i32_ne();
                fail(code);
            }
            _ => {
                code.global_get(CACHES + index)
                    .i32_const(0x00a00000)
                    .i32_and()
                    .i32_const(0x00800000)
                    .i32_eq();
                fail(code);
                code.global_get(CACHES + index)
                    .i32_const(0x00c00000)
                    .i32_and()
                    .i32_const(0x00c00000)
                    .i32_ne()
                    .if_(BlockType::Empty);
                mask(code, index, 0x60000000, 0x60000000);
                code.end();
            }
        }
        code.end().end();
    }
    field(code, 76);
    code.i32_const(0x20).i32_and().if_(BlockType::Empty);
    field(code, 92);
    code.i32_const(0xfffc).i32_and().i32_eqz();
    fail(code);
    field(code, 92);
    code.i32_const(4).i32_and();
    fail(code);
    field(code, 172);
    code.i32_const(0x9f00).i32_and().i32_const(0x8200).i32_ne();
    fail(code);
    code.else_();
    field(code, 168);
    field(code, 172);
    code.i32_or();
    fail(code);
    code.end();
    field(code, 80);
    code.local_tee(raw.status).i32_const(4).i32_gt_u();
    fail(code);
    field(code, 84);
    code.local_tee(raw.scratch).i32_const(0xfffc).i32_gt_u();
    fail(code);
    code.local_get(raw.scratch).i32_const(3).i32_and();
    fail(code);
    code.local_get(raw.status)
        .i32_eqz()
        .local_get(raw.status)
        .i32_const(4)
        .i32_eq()
        .i32_or()
        .if_(BlockType::Empty);
    code.local_get(raw.scratch);
    fail(code);
    code.else_()
        .local_get(raw.status)
        .i32_const(2)
        .i32_eq()
        .local_get(raw.status)
        .i32_const(3)
        .i32_eq()
        .i32_or()
        .if_(BlockType::Empty);
    code.local_get(raw.scratch).i32_eqz();
    fail(code);
    code.end().end();
}
pub(in crate::cpu::dbt::wasm) fn selector(
    code: &mut InstructionSink<'_>,
    segment: SegmentRegister,
) {
    code.global_get(SELECTORS + segment.index() as u32);
}
pub(in crate::cpu::dbt::wasm) fn base(code: &mut InstructionSink<'_>, segment: SegmentRegister) {
    code.global_get(BASES + segment.index() as u32);
}
pub(in crate::cpu::dbt::wasm) fn cache(code: &mut InstructionSink<'_>, segment: SegmentRegister) {
    code.global_get(CACHES + segment.index() as u32);
}
pub(in crate::cpu::dbt::wasm) fn selected_cache(
    code: &mut InstructionSink<'_>,
    selector_offset: u32,
    cache: bool,
) {
    for index in 0..6u32 {
        code.local_get(selector_offset)
            .i32_const((16 + index * 2) as i32)
            .i32_eq()
            .if_(BlockType::Result(ValType::I32))
            .global_get(if cache { CACHES } else { SELECTORS } + index)
            .else_();
    }
    code.i32_const(0);
    for _ in 0..6 {
        code.end();
    }
}
fn mask(code: &mut InstructionSink<'_>, index: u32, mask: u32, expected: u32) {
    code.global_get(CACHES + index)
        .i32_const(mask as i32)
        .i32_and()
        .i32_const(expected as i32)
        .i32_ne();
    fail(code);
}
fn field(code: &mut InstructionSink<'_>, offset: u32) {
    code.local_get(STATE_PTR).i32_load(arg(offset, 2));
}
fn arg(offset: u32, align: u32) -> MemArg {
    MemArg {
        offset: (TRANSFER_OFFSET as u32 + offset) as u64,
        align,
        memory_index: 0,
    }
}
fn fail(code: &mut InstructionSink<'_>) {
    code.if_(BlockType::Empty).i32_const(3).return_().end();
}

#[cfg(test)]
mod tests;
