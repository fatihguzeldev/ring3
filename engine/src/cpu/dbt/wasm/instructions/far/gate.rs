use crate::cpu::{
    dbt::wasm::{helpers::protocol, instructions::far::*},
    x86::{Register32, SegmentRegister},
};
pub(super) fn is_packet(code: &mut InstructionSink<'_>, context: cpu_result::ResultContext) {
    protocol::helper_field(code, 4);
    code.i32_const(0x10009).i32_eq();
    if context.paging_configured {
        cpu_result::paging::is_packet(code);
        code.i32_or();
    }
}

pub(super) fn jump_result(
    code: &mut InstructionSink<'_>,
    exit_depth: u32,
    context: cpu_result::ResultContext,
) {
    cpu_result::validate_gate(code, 0, false, exit_depth, context);
    cpu_result::value(code);
    code.local_set(RESULT);
}

pub(super) fn call_result(
    code: &mut InstructionSink<'_>,
    exit_depth: u32,
    context: cpu_result::ResultContext,
) {
    code.local_get(HELPER_STATUS)
        .i32_eqz()
        .i32_eqz()
        .local_get(HELPER_STATUS)
        .i32_const(11)
        .i32_ne()
        .i32_and();
    if context.paging_configured {
        code.local_get(HELPER_STATUS)
            .i32_const(23)
            .i32_ne()
            .i32_and();
    }
    code.if_(BlockType::Empty);
    protocol::infrastructure(code, 3);
    protocol::exit_if_failed(code, exit_depth + 1);
    code.end();
    protocol::helper_field(code, 36);
    code.i32_const(2).i32_eq().if_(BlockType::Empty);
    call_slot(code, 2, exit_depth + 1, context);
    code.else_();
    protocol::helper_field(code, 36);
    code.i32_const(4).i32_eq().if_(BlockType::Empty);
    call_slot(code, 4, exit_depth + 2, context);
    code.else_();
    protocol::infrastructure(code, 2);
    protocol::exit_if_failed(code, exit_depth + 2);
    code.end().end();
}

fn call_slot(
    code: &mut InstructionSink<'_>,
    slot: u32,
    exit_depth: u32,
    context: cpu_result::ResultContext,
) {
    // a page fault may refer to either selected slot. SS-limit exceptions carry no address.
    // use the loaded SS.B pointer independently of both gate width and target CS.D.
    crate::cpu::dbt::wasm::instructions::stack::pointer::offset(
        code,
        register(Register32::Esp),
        -(slot as i32),
    );
    crate::cpu::dbt::wasm::instructions::segments::base(code, SegmentRegister::Ss);
    code.i32_add().local_set(ADDRESS);
    protocol::helper_field(code, 28);
    code.local_get(ADDRESS)
        .i32_sub()
        .i32_const(slot as i32 - 1)
        .i32_gt_u()
        .if_(BlockType::Empty);
    crate::cpu::dbt::wasm::instructions::stack::pointer::offset(
        code,
        register(Register32::Esp),
        -((2 * slot) as i32),
    );
    crate::cpu::dbt::wasm::instructions::segments::base(code, SegmentRegister::Ss);
    code.i32_add().local_set(ADDRESS).end();
    cpu_result::validate_gate(code, slot, true, exit_depth, context);
    cpu_result::value(code);
    code.local_set(RESULT);
    crate::cpu::dbt::wasm::instructions::stack::pointer::advance(code, -((2 * slot) as i32));
}
