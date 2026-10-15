use super::{
    checked_address, checked_offset, load_result, store_result_at_address,
    store_word_result_at_address,
};
use crate::cpu::dbt::wasm::{imports::Imports, locals::*};
use crate::cpu::x86::SegmentAccess;
use crate::cpu::x86::ir::EffectiveAddress;
use wasm_encoder::InstructionSink;

pub(in crate::cpu::dbt::wasm) fn push_memory(
    code: &mut InstructionSink<'_>,
    source: EffectiveAddress,
    imports: Imports,
    exit_depth: u32,
) {
    checked_address(code, source, 4, SegmentAccess::Read, imports, exit_depth);
    crate::cpu::dbt::wasm::instructions::stack::pointer::check(
        code,
        crate::cpu::x86::Register32::Esp,
        -4,
        4,
        SegmentAccess::Write,
        imports,
        exit_depth,
    );
    load_result(code, source, imports, exit_depth);
    crate::cpu::dbt::wasm::instructions::stack::pointer::push_result(code, 4, imports, exit_depth);
}

pub(in crate::cpu::dbt::wasm) fn pop_memory(
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
        crate::cpu::dbt::wasm::instructions::stack::pointer::assigned(
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
    crate::cpu::dbt::wasm::instructions::stack::pointer::check(
        code,
        crate::cpu::x86::Register32::Esp,
        0,
        width,
        SegmentAccess::Read,
        imports,
        exit_depth,
    );
    checked_pop_destination(code, destination, width, imports, exit_depth);
    crate::cpu::dbt::wasm::instructions::stack::pointer::load_result(
        code, width, imports, exit_depth,
    );
    checked_pop_destination(code, destination, width, imports, exit_depth);
    if width == 2 {
        store_word_result_at_address(code, imports, exit_depth);
    } else {
        store_result_at_address(code, imports, exit_depth);
    }
    crate::cpu::dbt::wasm::instructions::stack::pointer::advance(code, width as i32);
}

pub(in crate::cpu::dbt::wasm) fn push_word_result(
    code: &mut InstructionSink<'_>,
    imports: Imports,
    exit_depth: u32,
) {
    crate::cpu::dbt::wasm::instructions::stack::pointer::push_result(code, 2, imports, exit_depth);
}

pub(in crate::cpu::dbt::wasm) fn pop_word_register(
    code: &mut InstructionSink<'_>,
    destination: crate::cpu::x86::Register32,
    imports: Imports,
    exit_depth: u32,
) {
    let esp = crate::cpu::x86::Register32::Esp;
    crate::cpu::dbt::wasm::instructions::stack::pointer::load(code, esp, 0, 2, imports, exit_depth);
    if destination == esp {
        crate::cpu::dbt::wasm::instructions::stack::pointer::assigned(code, register(esp), 2);
    } else {
        code.local_get(register(destination));
    }
    code.i32_const(!0xffff)
        .i32_and()
        .i32_or()
        .local_set(register(destination));
    if destination != esp {
        crate::cpu::dbt::wasm::instructions::stack::pointer::advance(code, 2);
    }
}

pub(in crate::cpu::dbt::wasm) fn pop_word_memory(
    code: &mut InstructionSink<'_>,
    destination: EffectiveAddress,
    imports: Imports,
    exit_depth: u32,
) {
    pop_memory_width(code, destination, 2, imports, exit_depth);
}

pub(in crate::cpu::dbt::wasm) fn pop_register(
    code: &mut InstructionSink<'_>,
    destination: crate::cpu::x86::Register32,
    imports: Imports,
    exit_depth: u32,
) {
    crate::cpu::dbt::wasm::instructions::stack::pointer::load_result(code, 4, imports, exit_depth);
    code.local_get(RESULT).local_set(register(destination));
    if destination != crate::cpu::x86::Register32::Esp {
        crate::cpu::dbt::wasm::instructions::stack::pointer::advance(code, 4);
    }
}

pub(in crate::cpu::dbt::wasm) fn pop_return(
    code: &mut InstructionSink<'_>,
    stack_adjust: u16,
    imports: Imports,
    exit_depth: u32,
) {
    crate::cpu::dbt::wasm::instructions::stack::pointer::load_result(code, 4, imports, exit_depth);
    if let Some(limit) = imports.code_fetch_limit {
        crate::cpu::dbt::wasm::code_fetch::check_value(code, RESULT, limit, exit_depth);
    }
    code.local_get(RESULT).local_set(EIP);
    crate::cpu::dbt::wasm::instructions::stack::pointer::advance(code, 4 + i32::from(stack_adjust));
}
