use crate::abi::memory_helper::CpuScalarOperation;
use wasm_encoder::InstructionSink;

use crate::cpu::x86::{
    Register32,
    ir::{ByteRegister, ByteValue, EffectiveAddress},
};

use super::{Imports, checked_address, exit_if_failed};
use crate::cpu::dbt::wasm::locals::*;

pub(super) fn store(
    code: &mut InstructionSink<'_>,
    address: EffectiveAddress,
    source: ByteValue,
    imports: Imports,
    exit_depth: u32,
) {
    checked_address(
        code,
        address,
        1,
        crate::cpu::x86::SegmentAccess::Write,
        imports,
        exit_depth,
    );
    let index = imports
        .store8
        .expect("prepared byte store has an import")
        .emit_binding(code);
    code.local_get(ADDRESS);
    match source {
        ByteValue::Immediate(value) => {
            code.i32_const(i32::from(value));
        }
        ByteValue::Register(source) => {
            let (parent, high) = match source {
                ByteRegister::Al => (Register32::Eax, false),
                ByteRegister::Cl => (Register32::Ecx, false),
                ByteRegister::Dl => (Register32::Edx, false),
                ByteRegister::Bl => (Register32::Ebx, false),
                ByteRegister::Ah => (Register32::Eax, true),
                ByteRegister::Ch => (Register32::Ecx, true),
                ByteRegister::Dh => (Register32::Edx, true),
                ByteRegister::Bh => (Register32::Ebx, true),
            };
            code.local_get(register(parent));
            if high {
                code.i32_const(8).i32_shr_u();
            }
            code.i32_const(0xff).i32_and();
        }
    }
    code.call(index).local_set(HELPER_STATUS);
    super::cpu_scalar::validate(code, 1, CpuScalarOperation::Store);
    exit_if_failed(code, exit_depth, imports.raw_task_context.is_some());
}

pub(super) fn store_result(
    code: &mut InstructionSink<'_>,
    address: EffectiveAddress,
    imports: Imports,
    exit_depth: u32,
) {
    checked_address(
        code,
        address,
        1,
        crate::cpu::x86::SegmentAccess::Write,
        imports,
        exit_depth,
    );
    store_result_at_address(code, imports, exit_depth);
}

pub(super) fn store_result_at_address(
    code: &mut InstructionSink<'_>,
    imports: Imports,
    exit_depth: u32,
) {
    let index = imports
        .store8
        .expect("prepared byte result store has an import")
        .emit_binding(code);
    code.local_get(ADDRESS)
        .local_get(RESULT)
        .call(index)
        .local_set(HELPER_STATUS);
    super::cpu_scalar::validate(code, 1, CpuScalarOperation::Store);
    exit_if_failed(code, exit_depth, imports.raw_task_context.is_some());
}
