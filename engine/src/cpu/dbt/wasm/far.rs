use super::{cpu_result, locals::*, memory};
use crate::{
    abi::arena::TRANSFER_OFFSET,
    cpu::x86::{FarWidth, ir::FarPointer},
};
use wasm_encoder::{BlockType, InstructionSink, MemArg};

pub(super) fn jump(
    code: &mut InstructionSink<'_>,
    width: FarWidth,
    target: FarPointer,
    imports: memory::Imports,
    exit_depth: u32,
) {
    match target {
        FarPointer::Immediate { offset, selector } => {
            code.i32_const(offset as i32)
                .local_set(RESULT)
                .i32_const(i32::from(selector))
                .local_set(ADDRESS);
        }
        FarPointer::Memory(address) => {
            let length = width.pointer_bytes();
            memory::checked_address(
                code,
                address,
                length,
                crate::cpu::x86::SegmentAccess::Read,
                imports,
                exit_depth,
            );
            code.local_get(ADDRESS)
                .i32_const(length as i32)
                .i32_const(1)
                .call(imports.cpu_read.expect("far pointer read import"))
                .local_set(HELPER_STATUS);
            cpu_result::validate(code, length, 1, false, exit_depth);
            code.local_get(STATE_PTR);
            match width {
                FarWidth::Word => {
                    code.i32_load16_u(transfer(0));
                }
                FarWidth::Dword => {
                    code.i32_load(transfer(0));
                }
            }
            code.local_set(RESULT)
                .local_get(STATE_PTR)
                .i32_load16_u(transfer(length - 2))
                .local_set(ADDRESS);
        }
    }
    let index = imports
        .far_jump
        .expect("far jump has a helper")
        .emit_binding(code);
    code.local_get(RESULT)
        .local_get(ADDRESS)
        .i32_const(i32::from(width == FarWidth::Word))
        .call(index)
        .local_set(HELPER_STATUS);
    descriptor_result(code, exit_depth);
    cpu_result::validate(code, 0, 0, false, exit_depth);
    // length-zero successful validation preserves the captured offset in RESULT.
    // enforce an exact target echo before publishing architectural EIP.
    memory::reset_checks(code);
    cpu_result::value(code);
    code.local_get(RESULT).i32_ne();
    memory::add_check(code);
    memory::protocol_if_bad(code);
    memory::exit_if_failed(code, exit_depth);
    code.local_get(RESULT).local_set(EIP);
}

fn transfer(offset: u32) -> MemArg {
    MemArg {
        offset: (TRANSFER_OFFSET as u32 + offset).into(),
        align: 0,
        memory_index: 0,
    }
}

// source memory faults belong to the earlier pointer-read helper. A descriptor
// result can report only success, #GP, or #NP, with a matching selector detail.
fn descriptor_result(code: &mut InstructionSink<'_>, exit_depth: u32) {
    // preserve the common validator's nonzero transport-status failure path.
    code.local_get(HELPER_STATUS)
        .i32_eqz()
        .if_(BlockType::Empty);
    memory::reset_checks(code);
    memory::helper_field(code, 16);
    code.local_set(RHS)
        .local_get(RHS)
        .i32_eqz()
        .i32_eqz()
        .local_get(RHS)
        .i32_const(3)
        .i32_ne()
        .i32_and()
        .local_get(RHS)
        .i32_const(6)
        .i32_ne()
        .i32_and();
    memory::add_check(code);
    code.local_get(RHS)
        .i32_const(3)
        .i32_eq()
        .if_(BlockType::Empty);
    memory::helper_field(code, 24);
    code.i32_eqz().i32_eqz();
    memory::helper_field(code, 24);
    code.local_get(ADDRESS)
        .i32_const(!3)
        .i32_and()
        .i32_ne()
        .i32_and();
    memory::add_check(code);
    code.end();
    code.local_get(RHS)
        .i32_const(6)
        .i32_eq()
        .if_(BlockType::Empty);
    memory::helper_field(code, 24);
    code.i32_eqz();
    memory::helper_field(code, 24);
    code.local_get(ADDRESS)
        .i32_const(!3)
        .i32_and()
        .i32_ne()
        .i32_or();
    memory::add_check(code);
    code.end();
    memory::protocol_if_bad(code);
    code.end();
    memory::exit_if_failed(code, exit_depth);
}
