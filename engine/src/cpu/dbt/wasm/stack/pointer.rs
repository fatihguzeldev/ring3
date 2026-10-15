use super::super::{
    locals::{RESULT, STATE_PTR, register},
    memory,
};
use crate::cpu::dbt::wasm::imports::Imports;
use crate::{
    abi::{arena::SEGMENTS_OFFSET, x86::SEGMENTS_CACHE_OFFSETS},
    cpu::x86::{Register32, SegmentAccess, SegmentRegister, ir::SmallWidth},
};
use wasm_encoder::{BlockType, InstructionSink, MemArg, ValType};

pub(in crate::cpu::dbt::wasm) fn default32(code: &mut InstructionSink<'_>) {
    code.local_get(STATE_PTR)
        .i32_load16_u(MemArg {
            offset: (SEGMENTS_OFFSET + 4) as u64,
            align: 1,
            memory_index: 0,
        })
        .i32_const(3)
        .i32_lt_u()
        .if_(BlockType::Result(ValType::I32));
    code.i32_const(1)
        .else_()
        .local_get(STATE_PTR)
        .i32_load(MemArg {
            offset: (SEGMENTS_OFFSET + SEGMENTS_CACHE_OFFSETS[2]) as u64,
            align: 2,
            memory_index: 0,
        })
        .i32_const(0x0200_0000)
        .i32_and()
        .i32_eqz()
        .i32_eqz()
        .end();
}

pub(in crate::cpu::dbt::wasm) fn offset(code: &mut InstructionSink<'_>, source: u32, delta: i32) {
    default32(code);
    code.if_(BlockType::Result(ValType::I32));
    code.local_get(source).i32_const(delta).i32_add();
    code.else_()
        .local_get(source)
        .i32_const(delta)
        .i32_add()
        .i32_const(0xffff)
        .i32_and()
        .end();
}

pub(in crate::cpu::dbt::wasm) fn assigned(code: &mut InstructionSink<'_>, source: u32, delta: i32) {
    default32(code);
    code.if_(BlockType::Result(ValType::I32));
    code.local_get(source).i32_const(delta).i32_add();
    code.else_()
        .local_get(register(Register32::Esp))
        .i32_const(!0xffff)
        .i32_and()
        .local_get(source)
        .i32_const(delta)
        .i32_add()
        .i32_const(0xffff)
        .i32_and()
        .i32_or()
        .end();
}

pub(in crate::cpu::dbt::wasm) fn advance(code: &mut InstructionSink<'_>, delta: i32) {
    assigned(code, register(Register32::Esp), delta);
    code.local_set(register(Register32::Esp));
}

pub(in crate::cpu::dbt::wasm) fn check(
    code: &mut InstructionSink<'_>,
    source: Register32,
    delta: i32,
    width: u32,
    access: SegmentAccess,
    imports: Imports,
    exit_depth: u32,
) {
    offset(code, register(source), delta);
    memory::checked_offset(
        code,
        SegmentRegister::Ss,
        width,
        access,
        imports,
        exit_depth,
    );
}

pub(in crate::cpu::dbt::wasm) fn load(
    code: &mut InstructionSink<'_>,
    source: Register32,
    delta: i32,
    width: u32,
    imports: Imports,
    exit_depth: u32,
) {
    check(
        code,
        source,
        delta,
        width,
        SegmentAccess::Read,
        imports,
        exit_depth,
    );
    if width == 2 {
        memory::load_narrow_at_address(code, SmallWidth::Word, imports, exit_depth);
    } else {
        memory::load_result_at_address(code, imports, exit_depth);
        code.local_get(RESULT);
    }
}

pub(in crate::cpu::dbt::wasm) fn load_result(
    code: &mut InstructionSink<'_>,
    width: u32,
    imports: Imports,
    exit_depth: u32,
) {
    load(code, Register32::Esp, 0, width, imports, exit_depth);
    code.local_set(RESULT);
}

pub(in crate::cpu::dbt::wasm) fn store_result(
    code: &mut InstructionSink<'_>,
    delta: i32,
    width: u32,
    imports: Imports,
    exit_depth: u32,
) {
    check(
        code,
        Register32::Esp,
        delta,
        width,
        SegmentAccess::Write,
        imports,
        exit_depth,
    );
    if width == 2 {
        memory::store_word_result_at_address(code, imports, exit_depth);
    } else {
        memory::store_result_at_address(code, imports, exit_depth);
    }
}

pub(in crate::cpu::dbt::wasm) fn push_result(
    code: &mut InstructionSink<'_>,
    width: u32,
    imports: Imports,
    exit_depth: u32,
) {
    store_result(code, -(width as i32), width, imports, exit_depth);
    advance(code, -(width as i32));
}
