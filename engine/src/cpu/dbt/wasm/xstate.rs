use wasm_encoder::{BlockType, InstructionSink};

use super::{cpu_result, locals::*, memory};
use crate::cpu::x86::{
    Register32, decode::DecodedInstruction, ir::Operation, xstate::XStateOperation,
};

pub(super) fn instruction(
    code: &mut InstructionSink<'_>,
    instruction: &DecodedInstruction,
    imports: memory::Imports,
    exit_depth: u32,
) -> bool {
    let Operation::XState(operation) = instruction.operation() else {
        unreachable!()
    };
    let (address, save) = match operation {
        XStateOperation::ReadControl => {
            code.local_get(register(Register32::Ecx))
                .if_(BlockType::Empty);
            code.i32_const(11).local_set(REASON);
            for local in [DETAIL, FAULT_ADDRESS, FAULT_ACCESS, FAULT_LENGTH] {
                code.i32_const(0).local_set(local);
            }
            code.br(exit_depth + 1).end();
            code.i32_const(crate::cpu::x86::capabilities::XCR0 as i32)
                .local_set(register(Register32::Eax))
                .i32_const(0)
                .local_set(register(Register32::Edx));
            return false;
        }
        XStateOperation::Save { address } => (address, true),
        XStateOperation::Restore { address } => (address, false),
    };
    memory::effective_address(code, *address);
    let helper = if save {
        imports.cpu_xsave
    } else {
        imports.cpu_xrstor
    }
    .expect("xstate helper import");
    let index = helper.emit_binding(code);
    code.local_get(ADDRESS)
        .local_get(register(Register32::Eax))
        .local_get(register(Register32::Edx))
        .call(index)
        .local_set(HELPER_STATUS);
    validate_context(code, save, exit_depth);
    cpu_result::validate(code, 576, if save { 3 } else { 1 }, save, exit_depth);
    save
}

fn range(code: &mut InstructionSink<'_>, start: i32, end: i32) {
    code.local_get(RHS)
        .i32_const(start)
        .i32_ge_u()
        .local_get(RHS)
        .i32_const(end)
        .i32_lt_u()
        .i32_and();
}

fn selected(code: &mut InstructionSink<'_>, bit: i32) {
    code.local_get(register(Register32::Eax))
        .i32_const(bit)
        .i32_and()
        .i32_eqz()
        .i32_eqz();
}

/// the generic packet envelope cannot authorize holes or unrequested components.
fn validate_context(code: &mut InstructionSink<'_>, save: bool, exit_depth: u32) {
    memory::reset_checks(code);
    memory::expect_field(code, 20, 0);
    memory::helper_field(code, 16);
    code.i32_const(3).i32_gt_u();
    memory::add_check(code);
    memory::helper_field(code, 16);
    code.i32_const(1).i32_le_u().if_(BlockType::Empty);
    code.local_get(ADDRESS).i32_const(63).i32_and();
    memory::add_check(code);
    code.end();
    memory::helper_field(code, 16);
    code.i32_const(3).i32_eq().if_(BlockType::Empty);
    memory::expect_field(code, 24, 0);
    code.end();
    memory::helper_field(code, 16);
    code.i32_const(1).i32_eq().if_(BlockType::Empty);
    memory::helper_field(code, 24);
    code.i32_const(3).i32_eq().if_(BlockType::Empty);
    // an overflowing header address faults before any selected payload access.
    memory::expect_field(code, 32, 1);
    code.else_();
    memory::helper_field(code, 28);
    code.local_get(ADDRESS).i32_sub().local_set(RHS);
    range(code, 512, if save { 520 } else { 536 });
    if save {
        memory::helper_field(code, 32);
        code.i32_const(2).i32_eq();
    }
    selected(code, 1);
    range(code, 0, 24);
    range(code, 32, 160);
    code.i32_or().i32_and();
    selected(code, 2);
    range(code, 24, if save { 32 } else { 28 });
    range(code, 160, 288);
    code.i32_or().i32_and().i32_or();
    if save {
        code.i32_and();
    }
    code.i32_or().i32_eqz();
    memory::add_check(code);
    code.end().end();
    memory::protocol_if_bad(code);
    memory::exit_if_failed(code, exit_depth);
}
