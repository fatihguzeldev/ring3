use super::*;
use crate::cpu::dbt::wasm::helpers::protocol;
use crate::cpu::dbt::wasm::imports::Imports;
use crate::cpu::x86::Register32;

pub(in crate::cpu::dbt::wasm) fn call(
    code: &mut InstructionSink<'_>,
    width: FarWidth,
    target: FarPointer,
    return_eip: u32,
    imports: Imports,
    exit_depth: u32,
) {
    capture_target(code, width, target, imports, exit_depth);
    let index = imports
        .far_call
        .expect("far call atomic helper")
        .emit_binding(code);
    let esp = register(Register32::Esp);
    code.local_get(RESULT)
        .local_get(ADDRESS)
        .i32_const(i32::from(width == FarWidth::Word))
        .i32_const(return_eip as i32)
        .local_get(esp)
        .call(index)
        .local_set(HELPER_STATUS);
    gate::is_packet(code, cpu_result::ResultContext::from(imports));
    code.if_(BlockType::Empty);
    gate::call_result(
        code,
        exit_depth + 1,
        cpu_result::ResultContext::from(imports),
    );
    code.else_();
    call_result(
        code,
        width.slot_bytes(),
        exit_depth + 1,
        cpu_result::ResultContext::from(imports),
    );
    target_echo(code, exit_depth + 1);
    super::super::stack::pointer::advance(code, -((2 * width.slot_bytes()) as i32));
    code.end();
    code.local_get(RESULT).local_set(EIP);
}

// the descriptor and stack phases have distinct status/length contracts. The common
// validator still owns envelope, canonical detail fields and transport-status handling.
fn call_result(
    code: &mut InstructionSink<'_>,
    slot: u32,
    exit_depth: u32,
    context: cpu_result::ResultContext,
) {
    code.local_get(HELPER_STATUS)
        .i32_eqz()
        .local_get(HELPER_STATUS)
        .i32_const(11)
        .i32_eq()
        .i32_or()
        .if_(BlockType::Empty);
    protocol::reset_checks(code);
    protocol::helper_field(code, 16);
    code.local_set(RHS).i32_const(1);
    for status in [0, 1, 2, 3, 6, 7] {
        code.local_get(RHS).i32_const(status).i32_ne().i32_and();
    }
    protocol::add_check(code);
    for status in [3, 6] {
        code.local_get(RHS)
            .i32_const(status)
            .i32_eq()
            .if_(BlockType::Empty);
        protocol::helper_field(code, 24);
        code.i32_eqz();
        if status == 3 {
            code.i32_eqz();
        }
        protocol::helper_field(code, 24);
        code.local_get(ADDRESS).i32_const(!3).i32_and().i32_ne();
        if status == 3 {
            code.i32_and();
        } else {
            code.i32_or();
        }
        protocol::add_check(code);
        code.end();
    }
    code.local_get(RHS)
        .i32_const(7)
        .i32_eq()
        .if_(BlockType::Empty);
    protocol::expect_field(code, 24, 0);
    code.end();
    protocol::protocol_if_bad(code);
    code.end();
    protocol::exit_if_failed(code, exit_depth);

    // the common validator checks the selected linear slot, including base-addition overflow.
    protocol::helper_field(code, 16);
    code.i32_const(1).i32_eq();
    protocol::helper_field(code, 16);
    code.i32_const(7).i32_eq().i32_or().if_(BlockType::Empty);
    super::super::stack::pointer::offset(code, register(Register32::Esp), -(slot as i32));
    super::super::segments::base(code, crate::cpu::x86::SegmentRegister::Ss);
    code.i32_add().local_set(ADDRESS);
    protocol::helper_field(code, 28);
    code.local_get(ADDRESS)
        .i32_sub()
        .i32_const(slot as i32 - 1)
        .i32_gt_u()
        .if_(BlockType::Empty);
    super::super::stack::pointer::offset(code, register(Register32::Esp), -((2 * slot) as i32));
    super::super::segments::base(code, crate::cpu::x86::SegmentRegister::Ss);
    code.i32_add().local_set(ADDRESS).end();
    cpu_result::validate(code, slot, 2, true, exit_depth + 1, context);
    code.else_();
    cpu_result::validate(code, 0, 0, true, exit_depth + 1, context);
    code.end();
}
