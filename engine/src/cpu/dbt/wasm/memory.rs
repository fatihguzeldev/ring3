use wasm_encoder::{BlockType, InstructionSink};

use crate::cpu::x86::ir::{EffectiveAddress, SmallWidth, Value32};
use crate::cpu::x86::{SegmentAccess, SegmentRegister};

use super::locals::*;

mod byte_exchange;
mod byte_store;
mod cpu_scalar;
mod exchange;
mod narrow;
mod word_exchange;
mod word_store;

use super::{
    helpers::protocol::{
        add_check, exit_if_failed, expect_field, helper_field, infrastructure, protocol_if_bad,
        reset_checks,
    },
    imports::Imports,
};

pub(super) fn bit_test_set_result(
    code: &mut InstructionSink<'_>,
    address: EffectiveAddress,
    index: crate::cpu::x86::Register32,
    imports: Imports,
    exit_depth: u32,
) {
    exchange::bit_test_set_result(code, address, index, imports, exit_depth);
}

pub(super) fn bit_test_reset_result(
    code: &mut InstructionSink<'_>,
    address: EffectiveAddress,
    index: crate::cpu::x86::Register32,
    imports: Imports,
    exit_depth: u32,
) {
    exchange::bit_test_reset_result(code, address, index, imports, exit_depth);
}

pub(super) fn atomic_or_result(
    code: &mut InstructionSink<'_>,
    address: EffectiveAddress,
    mask: u32,
    imports: Imports,
    exit_depth: u32,
) {
    exchange::atomic_or_result(code, address, mask, imports, exit_depth);
}

pub(super) fn atomic_and_result(
    code: &mut InstructionSink<'_>,
    address: EffectiveAddress,
    mask: u32,
    imports: Imports,
    exit_depth: u32,
) {
    exchange::atomic_and_result(code, address, mask, imports, exit_depth);
}

pub(super) fn atomic_xor_result(
    code: &mut InstructionSink<'_>,
    address: EffectiveAddress,
    mask: u32,
    imports: Imports,
    exit_depth: u32,
) {
    exchange::atomic_xor_result(code, address, mask, imports, exit_depth);
}

pub(super) fn atomic_or_register_result(
    code: &mut InstructionSink<'_>,
    address: EffectiveAddress,
    source: crate::cpu::x86::Register32,
    imports: Imports,
    exit_depth: u32,
) {
    exchange::atomic_or_register_result(code, address, source, imports, exit_depth);
}

pub(super) fn atomic_and_register_result(
    code: &mut InstructionSink<'_>,
    address: EffectiveAddress,
    source: crate::cpu::x86::Register32,
    imports: Imports,
    exit_depth: u32,
) {
    exchange::atomic_and_register_result(code, address, source, imports, exit_depth);
}

pub(super) fn atomic_xor_register_result(
    code: &mut InstructionSink<'_>,
    address: EffectiveAddress,
    source: crate::cpu::x86::Register32,
    imports: Imports,
    exit_depth: u32,
) {
    exchange::atomic_xor_register_result(code, address, source, imports, exit_depth);
}

pub(super) fn exchange_add_result(
    code: &mut InstructionSink<'_>,
    address: EffectiveAddress,
    source: crate::cpu::x86::Register32,
    imports: Imports,
    exit_depth: u32,
) {
    exchange::exchange_add_result(code, address, source, imports, exit_depth);
}

pub(super) fn exchange_result(
    code: &mut InstructionSink<'_>,
    address: EffectiveAddress,
    source: crate::cpu::x86::Register32,
    imports: Imports,
    exit_depth: u32,
) {
    exchange::exchange_result(code, address, source, imports, exit_depth);
}

pub(super) fn compare_exchange_result(
    code: &mut InstructionSink<'_>,
    address: EffectiveAddress,
    source: crate::cpu::x86::Register32,
    imports: Imports,
    exit_depth: u32,
) {
    exchange::compare_exchange_result(code, address, source, imports, exit_depth);
}

pub(super) fn exchange_byte_result(
    code: &mut InstructionSink<'_>,
    address: EffectiveAddress,
    imports: Imports,
    exit_depth: u32,
) {
    byte_exchange::exchange_result(code, address, imports, exit_depth);
}

pub(super) fn exchange_add_byte_result(
    code: &mut InstructionSink<'_>,
    address: EffectiveAddress,
    imports: Imports,
    exit_depth: u32,
) {
    byte_exchange::exchange_add_result(code, address, imports, exit_depth);
}

pub(super) fn compare_exchange_byte_result(
    code: &mut InstructionSink<'_>,
    address: EffectiveAddress,
    imports: Imports,
    exit_depth: u32,
) {
    byte_exchange::compare_exchange_result(code, address, imports, exit_depth);
}

pub(super) fn exchange_word_result(
    code: &mut InstructionSink<'_>,
    address: EffectiveAddress,
    imports: Imports,
    exit_depth: u32,
) {
    word_exchange::exchange_result(code, address, imports, exit_depth);
}

pub(super) fn exchange_add_word_result(
    code: &mut InstructionSink<'_>,
    address: EffectiveAddress,
    imports: Imports,
    exit_depth: u32,
) {
    word_exchange::exchange_add_result(code, address, imports, exit_depth);
}

pub(super) fn compare_exchange_word_result(
    code: &mut InstructionSink<'_>,
    address: EffectiveAddress,
    imports: Imports,
    exit_depth: u32,
) {
    word_exchange::compare_exchange_result(code, address, imports, exit_depth);
}

pub(super) fn store_byte(
    code: &mut InstructionSink<'_>,
    address: EffectiveAddress,
    source: crate::cpu::x86::ir::ByteValue,
    imports: Imports,
    exit_depth: u32,
) {
    byte_store::store(code, address, source, imports, exit_depth);
}

pub(super) fn store_byte_result(
    code: &mut InstructionSink<'_>,
    address: EffectiveAddress,
    imports: Imports,
    exit_depth: u32,
) {
    byte_store::store_result(code, address, imports, exit_depth);
}

pub(super) fn store_word_result(
    code: &mut InstructionSink<'_>,
    address: EffectiveAddress,
    imports: Imports,
    exit_depth: u32,
) {
    word_store::store_result(code, address, imports, exit_depth);
}

pub(super) fn store_byte_result_at_address(
    code: &mut InstructionSink<'_>,
    imports: Imports,
    exit_depth: u32,
) {
    byte_store::store_result_at_address(code, imports, exit_depth);
}

pub(super) fn store_word_result_at_address(
    code: &mut InstructionSink<'_>,
    imports: Imports,
    exit_depth: u32,
) {
    word_store::store_result_at_address(code, imports, exit_depth);
}

pub(super) fn load_narrow_value(
    code: &mut InstructionSink<'_>,
    address: EffectiveAddress,
    width: SmallWidth,
    imports: Imports,
    exit_depth: u32,
) {
    narrow::load_value(code, address, width, imports, exit_depth);
}

pub(super) fn load_narrow_at_address(
    code: &mut InstructionSink<'_>,
    width: SmallWidth,
    imports: Imports,
    exit_depth: u32,
) {
    narrow::load_at_address(code, width, imports, exit_depth);
}

pub(super) fn load(
    code: &mut InstructionSink<'_>,
    address: EffectiveAddress,
    destination: crate::cpu::x86::Register32,
    imports: Imports,
    exit_depth: u32,
) {
    checked_address(code, address, 4, SegmentAccess::Read, imports, exit_depth);
    code.local_get(ADDRESS)
        .call(imports.read.expect("prepared memory load has an import"))
        .local_set(HELPER_STATUS);
    super::code_fetch::check_context(code, exit_depth);
    cpu_scalar::validate(
        code,
        4,
        crate::abi::memory_helper::CpuScalarOperation::Read,
        imports.paging_configured,
    );
    exit_if_failed(code, exit_depth);
    helper_field(code, 20);
    code.local_set(register(destination));
}

pub(super) fn store(
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

pub(super) fn push_memory(
    code: &mut InstructionSink<'_>,
    source: EffectiveAddress,
    imports: Imports,
    exit_depth: u32,
) {
    checked_address(code, source, 4, SegmentAccess::Read, imports, exit_depth);
    super::stack::pointer::check(
        code,
        crate::cpu::x86::Register32::Esp,
        -4,
        4,
        SegmentAccess::Write,
        imports,
        exit_depth,
    );
    load_result(code, source, imports, exit_depth);
    super::stack::pointer::push_result(code, 4, imports, exit_depth);
}

pub(super) fn pop_memory(
    code: &mut InstructionSink<'_>,
    destination: EffectiveAddress,
    imports: Imports,
    exit_depth: u32,
) {
    pop_memory_width(code, destination, 4, imports, exit_depth);
}

fn checked_pop_destination(
    code: &mut InstructionSink<'_>,
    destination: EffectiveAddress,
    width: u32,
    imports: Imports,
    exit_depth: u32,
) {
    if destination.base == Some(crate::cpu::x86::Register32::Esp) {
        // wrapped B16 POP follows the declared Ring3 recomposed-ESP destination policy.
        code.i32_const(destination.displacement as i32);
        super::stack::pointer::assigned(
            code,
            register(crate::cpu::x86::Register32::Esp),
            width as i32,
        );
        code.i32_add();
        if let Some(index) = destination.index {
            code.local_get(register(index))
                .i32_const(i32::from(destination.scale))
                .i32_mul()
                .i32_add();
        }
        if destination.address_size == crate::cpu::x86::ir::AddressSize::Bits16 {
            code.i32_const(0xffff).i32_and();
        }
        checked_offset(
            code,
            destination.resolved_segment(),
            width,
            SegmentAccess::Write,
            imports,
            exit_depth,
        );
    } else {
        checked_address(
            code,
            destination,
            width,
            SegmentAccess::Write,
            imports,
            exit_depth,
        );
    }
}

fn pop_memory_width(
    code: &mut InstructionSink<'_>,
    destination: EffectiveAddress,
    width: u32,
    imports: Imports,
    exit_depth: u32,
) {
    super::stack::pointer::check(
        code,
        crate::cpu::x86::Register32::Esp,
        0,
        width,
        SegmentAccess::Read,
        imports,
        exit_depth,
    );
    checked_pop_destination(code, destination, width, imports, exit_depth);
    super::stack::pointer::load_result(code, width, imports, exit_depth);
    checked_pop_destination(code, destination, width, imports, exit_depth);
    if width == 2 {
        store_word_result_at_address(code, imports, exit_depth);
    } else {
        store_result_at_address(code, imports, exit_depth);
    }
    super::stack::pointer::advance(code, width as i32);
}

pub(super) fn push_word_result(code: &mut InstructionSink<'_>, imports: Imports, exit_depth: u32) {
    super::stack::pointer::push_result(code, 2, imports, exit_depth);
}

pub(super) fn pop_word_register(
    code: &mut InstructionSink<'_>,
    destination: crate::cpu::x86::Register32,
    imports: Imports,
    exit_depth: u32,
) {
    let esp = crate::cpu::x86::Register32::Esp;
    super::stack::pointer::load(code, esp, 0, 2, imports, exit_depth);
    if destination == esp {
        super::stack::pointer::assigned(code, register(esp), 2);
    } else {
        code.local_get(register(destination));
    }
    code.i32_const(!0xffff)
        .i32_and()
        .i32_or()
        .local_set(register(destination));
    if destination != esp {
        super::stack::pointer::advance(code, 2);
    }
}

pub(super) fn pop_word_memory(
    code: &mut InstructionSink<'_>,
    destination: EffectiveAddress,
    imports: Imports,
    exit_depth: u32,
) {
    pop_memory_width(code, destination, 2, imports, exit_depth);
}

pub(super) fn load_result(
    code: &mut InstructionSink<'_>,
    source: EffectiveAddress,
    imports: Imports,
    exit_depth: u32,
) {
    checked_address(code, source, 4, SegmentAccess::Read, imports, exit_depth);
    load_result_at_address(code, imports, exit_depth);
}

pub(super) fn load_modify_result(
    code: &mut InstructionSink<'_>,
    source: EffectiveAddress,
    imports: Imports,
    exit_depth: u32,
) {
    checked_address(code, source, 4, SegmentAccess::Write, imports, exit_depth);
    load_modify_at_address(code, imports, exit_depth);
}

pub(super) fn load_narrow_for_modify(
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

pub(super) fn load_narrow_modify_at_address(
    code: &mut InstructionSink<'_>,
    width: SmallWidth,
    imports: Imports,
    exit_depth: u32,
) {
    narrow::load_at_address_with_intent(code, width, imports, exit_depth, true);
}

pub(super) fn load_modify_at_address(
    code: &mut InstructionSink<'_>,
    imports: Imports,
    exit_depth: u32,
) {
    load_at_address_with_intent(code, imports, exit_depth, true);
}

pub(super) fn load_result_at_address(
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
    super::code_fetch::check_context(code, exit_depth);
    cpu_scalar::validate(
        code,
        4,
        if paging_modify {
            crate::abi::memory_helper::CpuScalarOperation::Modify
        } else {
            crate::abi::memory_helper::CpuScalarOperation::Read
        },
        imports.paging_configured,
    );
    exit_if_failed(code, exit_depth);
    helper_field(code, 20);
    code.local_set(RESULT);
}

pub(super) fn store_result(
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

pub(super) fn store_result_at_address(
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

pub(super) fn pop_register(
    code: &mut InstructionSink<'_>,
    destination: crate::cpu::x86::Register32,
    imports: Imports,
    exit_depth: u32,
) {
    super::stack::pointer::load_result(code, 4, imports, exit_depth);
    code.local_get(RESULT).local_set(register(destination));
    if destination != crate::cpu::x86::Register32::Esp {
        super::stack::pointer::advance(code, 4);
    }
}

pub(super) fn pop_return(
    code: &mut InstructionSink<'_>,
    stack_adjust: u16,
    imports: Imports,
    exit_depth: u32,
) {
    super::stack::pointer::load_result(code, 4, imports, exit_depth);
    if let Some(limit) = imports.code_fetch_limit {
        super::code_fetch::check_value(code, RESULT, limit, exit_depth);
    }
    code.local_get(RESULT).local_set(EIP);
    super::stack::pointer::advance(code, 4 + i32::from(stack_adjust));
}

pub(super) fn exit_if_invalidated(
    code: &mut InstructionSink<'_>,
    exit_depth: u32,
    imports: Imports,
) {
    super::abi::debug_after_retirement(code, exit_depth, imports.detection, false);
    code.local_get(HELPER_STATUS)
        .i32_const(11)
        .i32_eq()
        .if_(BlockType::Empty)
        .i32_const(6)
        .local_set(REASON)
        .br(exit_depth + 1)
        .end();
}

pub(super) fn effective_address(code: &mut InstructionSink<'_>, address: EffectiveAddress) {
    address_value(code, address);
    code.local_set(ADDRESS);
}

pub(super) fn checked_address(
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
pub(super) fn checked_offset(
    code: &mut InstructionSink<'_>,
    segment: SegmentRegister,
    width: u32,
    access: SegmentAccess,
    imports: Imports,
    exit_depth: u32,
) {
    code.local_set(ADDRESS);
    super::segments::check_span(code, segment, width, access, imports, exit_depth);
    code.local_get(ADDRESS);
    super::segments::base(code, segment);
    code.i32_add().local_set(ADDRESS);
}

pub(super) fn address_value(code: &mut InstructionSink<'_>, address: EffectiveAddress) {
    offset_value(code, address);
    super::segments::base(code, address.resolved_segment());
    code.i32_add();
}

pub(super) fn offset_value(code: &mut InstructionSink<'_>, address: EffectiveAddress) {
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
