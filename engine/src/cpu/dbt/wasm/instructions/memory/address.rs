use crate::cpu::dbt::wasm::{imports::Imports, locals::*};
use crate::cpu::x86::ir::EffectiveAddress;
use crate::cpu::x86::{SegmentAccess, SegmentRegister};
use wasm_encoder::InstructionSink;

pub(in crate::cpu::dbt::wasm) fn effective_address(
    code: &mut InstructionSink<'_>,
    address: EffectiveAddress,
) {
    address_value(code, address);
    code.local_set(ADDRESS);
}

pub(in crate::cpu::dbt::wasm) fn checked_address(
    code: &mut InstructionSink<'_>,
    address: EffectiveAddress,
    width: u32,
    access: SegmentAccess,
    imports: Imports,
    exit_depth: u32,
) {
    offset_value(code, address);
    checked_offset(
        code,
        address.resolved_segment(),
        width,
        access,
        imports,
        exit_depth,
    );
}

/// consumes a logical offset, including any instruction-specific bit-index displacement.
pub(in crate::cpu::dbt::wasm) fn checked_offset(
    code: &mut InstructionSink<'_>,
    segment: SegmentRegister,
    width: u32,
    access: SegmentAccess,
    imports: Imports,
    exit_depth: u32,
) {
    code.local_set(ADDRESS);
    crate::cpu::dbt::wasm::instructions::segments::check_span(
        code, segment, width, access, imports, exit_depth,
    );
    code.local_get(ADDRESS);
    crate::cpu::dbt::wasm::instructions::segments::base(code, segment);
    code.i32_add().local_set(ADDRESS);
}

pub(in crate::cpu::dbt::wasm) fn address_value(
    code: &mut InstructionSink<'_>,
    address: EffectiveAddress,
) {
    offset_value(code, address);
    crate::cpu::dbt::wasm::instructions::segments::base(code, address.resolved_segment());
    code.i32_add();
}

pub(in crate::cpu::dbt::wasm) fn offset_value(
    code: &mut InstructionSink<'_>,
    address: EffectiveAddress,
) {
    code.i32_const(address.displacement as i32);
    if let Some(base) = address.base {
        code.local_get(register(base)).i32_add();
    }
    if let Some(index) = address.index {
        code.local_get(register(index))
            .i32_const(i32::from(address.scale))
            .i32_mul()
            .i32_add();
    }
    if address.address_size == crate::cpu::x86::ir::AddressSize::Bits16 {
        code.i32_const(0xffff).i32_and();
    }
}
