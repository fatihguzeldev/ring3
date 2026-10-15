use crate::abi::memory_helper::CpuScalarOperation;
use crate::cpu::x86::ir::EffectiveAddress;
use wasm_encoder::InstructionSink;

use crate::cpu::dbt::wasm::{
    helpers::protocol::{exit_if_failed, helper_field},
    imports::Imports,
    instructions::memory::checked_address,
    locals::*,
};
pub(in crate::cpu::dbt::wasm) fn exchange_add_result(
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
    let index = imports
        .exchange_add8
        .expect("prepared locked byte exchange add has an import")
        .emit_binding(code);
    code.local_get(ADDRESS)
        .local_get(RESULT)
        .call(index)
        .local_set(HELPER_STATUS);
    super::cpu_scalar::validate(
        code,
        1,
        CpuScalarOperation::Modify,
        imports.paging_configured,
    );
    exit_if_failed(code, exit_depth);
    helper_field(code, 20);
    code.local_set(RESULT);
}

pub(in crate::cpu::dbt::wasm) fn exchange_result(
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
    let index = imports
        .exchange8
        .expect("prepared byte exchange has an import")
        .emit_binding(code);
    code.local_get(ADDRESS)
        .local_get(RESULT)
        .call(index)
        .local_set(HELPER_STATUS);
    super::cpu_scalar::validate(
        code,
        1,
        CpuScalarOperation::Modify,
        imports.paging_configured,
    );
    exit_if_failed(code, exit_depth);
    helper_field(code, 20);
    code.local_set(RESULT);
}

pub(in crate::cpu::dbt::wasm) fn compare_exchange_result(
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
    let index = imports
        .compare_exchange8
        .expect("prepared locked byte compare exchange has an import")
        .emit_binding(code);
    code.local_get(ADDRESS)
        .local_get(register(crate::cpu::x86::Register32::Eax))
        .i32_const(0xff)
        .i32_and()
        .local_get(RESULT)
        .call(index)
        .local_set(HELPER_STATUS);
    super::cpu_scalar::validate(
        code,
        1,
        CpuScalarOperation::Modify,
        imports.paging_configured,
    );
    exit_if_failed(code, exit_depth);
    helper_field(code, 20);
    code.local_set(RESULT);
}
