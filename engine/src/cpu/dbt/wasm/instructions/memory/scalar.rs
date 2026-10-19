use super::{checked_address, cpu_scalar, narrow};
use crate::cpu::dbt::wasm::helpers::protocol::{exit_if_failed, helper_field};
use crate::cpu::dbt::wasm::{imports::Imports, locals::*};
use crate::cpu::x86::SegmentAccess;
use crate::cpu::x86::ir::{EffectiveAddress, SmallWidth, Value32};
use wasm_encoder::InstructionSink;

pub(in crate::cpu::dbt::wasm) fn load(
    code: &mut InstructionSink<'_>,
    address: EffectiveAddress,
    destination: crate::cpu::x86::Register32,
    imports: Imports,
    exit_depth: u32,
) {
    checked_address(code, address, 4, SegmentAccess::Read, imports, exit_depth);
    super::read_view::load(code, imports);
    crate::cpu::dbt::wasm::code_fetch::check_context(code, exit_depth);
    cpu_scalar::validate_read32(code, imports);
    exit_if_failed(code, exit_depth);
    helper_field(code, 20);
    code.local_set(register(destination));
}

pub(in crate::cpu::dbt::wasm) fn store(
    code: &mut InstructionSink<'_>,
    address: EffectiveAddress,
    source: Value32,
    imports: Imports,
    exit_depth: u32,
) {
    checked_address(code, address, 4, SegmentAccess::Write, imports, exit_depth);
    let index = imports
        .store
        .expect("prepared memory store has an import")
        .emit_binding(code);
    code.local_get(ADDRESS);
    match source {
        Value32::Register(source) => {
            code.local_get(register(source));
        }
        Value32::Immediate(value) => {
            code.i32_const(value as i32);
        }
        Value32::Memory(_) => unreachable!("prepared move cannot have two memory operands"),
    }
    code.call(index).local_set(HELPER_STATUS);
    cpu_scalar::validate(
        code,
        4,
        crate::abi::memory_helper::CpuScalarOperation::Store,
        imports.paging_configured,
    );
    exit_if_failed(code, exit_depth);
}

pub(in crate::cpu::dbt::wasm) fn load_result(
    code: &mut InstructionSink<'_>,
    source: EffectiveAddress,
    imports: Imports,
    exit_depth: u32,
) {
    checked_address(code, source, 4, SegmentAccess::Read, imports, exit_depth);
    load_result_at_address(code, imports, exit_depth);
}

pub(in crate::cpu::dbt::wasm) fn load_modify_result(
    code: &mut InstructionSink<'_>,
    source: EffectiveAddress,
    imports: Imports,
    exit_depth: u32,
) {
    checked_address(code, source, 4, SegmentAccess::Write, imports, exit_depth);
    load_modify_at_address(code, imports, exit_depth);
}

pub(in crate::cpu::dbt::wasm) fn load_narrow_for_modify(
    code: &mut InstructionSink<'_>,
    source: EffectiveAddress,
    width: SmallWidth,
    imports: Imports,
    exit_depth: u32,
) {
    let length = match width {
        SmallWidth::Byte => 1,
        SmallWidth::Word => 2,
    };
    checked_address(
        code,
        source,
        length,
        SegmentAccess::Write,
        imports,
        exit_depth,
    );
    load_narrow_modify_at_address(code, width, imports, exit_depth);
}

pub(in crate::cpu::dbt::wasm) fn load_narrow_modify_at_address(
    code: &mut InstructionSink<'_>,
    width: SmallWidth,
    imports: Imports,
    exit_depth: u32,
) {
    narrow::load_at_address_with_intent(code, width, imports, exit_depth, true);
}

pub(in crate::cpu::dbt::wasm) fn load_modify_at_address(
    code: &mut InstructionSink<'_>,
    imports: Imports,
    exit_depth: u32,
) {
    load_at_address_with_intent(code, imports, exit_depth, true);
}

pub(in crate::cpu::dbt::wasm) fn load_result_at_address(
    code: &mut InstructionSink<'_>,
    imports: Imports,
    exit_depth: u32,
) {
    load_at_address_with_intent(code, imports, exit_depth, false);
}

fn load_at_address_with_intent(
    code: &mut InstructionSink<'_>,
    imports: Imports,
    exit_depth: u32,
    modify: bool,
) {
    let paging_modify = modify && imports.paging_configured;
    if modify {
        code.local_get(ADDRESS)
            .call(
                (if paging_modify {
                    imports.read_modify
                } else {
                    imports.read
                })
                .expect("prepared memory operand has a read import"),
            )
            .local_set(HELPER_STATUS);
    } else {
        super::read_view::load(code, imports);
    }
    crate::cpu::dbt::wasm::code_fetch::check_context(code, exit_depth);
    if paging_modify {
        cpu_scalar::validate(
            code,
            4,
            crate::abi::memory_helper::CpuScalarOperation::Modify,
            imports.paging_configured,
        );
    } else {
        cpu_scalar::validate_read32(code, imports);
    }
    exit_if_failed(code, exit_depth);
    helper_field(code, 20);
    code.local_set(RESULT);
}

pub(in crate::cpu::dbt::wasm) fn store_result(
    code: &mut InstructionSink<'_>,
    destination: EffectiveAddress,
    imports: Imports,
    exit_depth: u32,
) {
    checked_address(
        code,
        destination,
        4,
        SegmentAccess::Write,
        imports,
        exit_depth,
    );
    store_result_at_address(code, imports, exit_depth);
}

pub(in crate::cpu::dbt::wasm) fn store_result_at_address(
    code: &mut InstructionSink<'_>,
    imports: Imports,
    exit_depth: u32,
) {
    let index = imports
        .store
        .expect("prepared memory operand has a store import")
        .emit_binding(code);
    code.local_get(ADDRESS)
        .local_get(RESULT)
        .call(index)
        .local_set(HELPER_STATUS);
    cpu_scalar::validate(
        code,
        4,
        crate::abi::memory_helper::CpuScalarOperation::Store,
        imports.paging_configured,
    );
    exit_if_failed(code, exit_depth);
}
