use wasm_encoder::{BlockType, InstructionSink, ValType};

use super::super::super::locals::*;
use super::{field, memory};
use crate::{
    abi::{
        X86_INTEGER_PROFILE,
        x86::{
            GATHER_ACCESS_OFFSET, GATHER_DETAIL_OFFSET, GATHER_FAULT_ADDRESS_OFFSET,
            GATHER_PAGING_RESULT_VERSION, GATHER_RESULT_VERSION,
        },
    },
    cpu::x86::SegmentRegister,
};

const PAGE_FAULT: i32 = 9;

pub(super) fn validate_kind(code: &mut InstructionSink<'_>, segment_status: i32) {
    code.local_get(RHS)
        .i32_const(PAGE_FAULT)
        .i32_eq()
        .local_get(HELPER_STATUS)
        .i32_const(crate::abi::memory_helper::CPU_PAGE_FAULT_TRANSPORT as i32)
        .i32_eq()
        .i32_xor();
    memory::add_check(code);
    field(code, 4);
    code.i32_const(i32::from(GATHER_RESULT_VERSION) | i32::from(X86_INTEGER_PROFILE) << 16)
        .i32_eq()
        .if_(BlockType::Empty);
    code.local_get(RHS)
        .i32_eqz()
        .i32_eqz()
        .local_get(RHS)
        .i32_const(1)
        .i32_ne()
        .i32_and()
        .local_get(RHS)
        .i32_const(segment_status)
        .i32_ne()
        .i32_and();
    memory::add_check(code);
    code.else_();
    memory::expect_field(
        code,
        4,
        u32::from(GATHER_PAGING_RESULT_VERSION) | u32::from(X86_INTEGER_PROFILE) << 16,
    );
    code.local_get(RHS).i32_const(PAGE_FAULT).i32_ne();
    memory::add_check(code);
    code.end();
}

pub(super) fn is_memory(code: &mut InstructionSink<'_>) {
    code.local_get(RHS)
        .i32_const(1)
        .i32_eq()
        .local_get(RHS)
        .i32_const(PAGE_FAULT)
        .i32_eq()
        .i32_or();
}

pub(super) fn validate_memory_fields(code: &mut InstructionSink<'_>) {
    memory::expect_field(code, GATHER_ACCESS_OFFSET as u64, 1);
    code.local_get(RHS)
        .i32_const(PAGE_FAULT)
        .i32_eq()
        .if_(BlockType::Empty);
    // gather reports a user read fault without an instruction-fetch bit.
    field(code, GATHER_DETAIL_OFFSET);
    code.i32_const(7).i32_gt_u();
    memory::add_check(code);
    field(code, GATHER_DETAIL_OFFSET);
    code.i32_const(6).i32_and().i32_const(4).i32_ne();
    memory::add_check(code);
    code.else_();
    field(code, GATHER_DETAIL_OFFSET);
    code.i32_const(1).i32_sub().i32_const(1).i32_gt_u();
    memory::add_check(code);
    code.end();
}

pub(super) fn operand_exit(code: &mut InstructionSink<'_>, segment: SegmentRegister, width: usize) {
    code.local_get(RHS).if_(BlockType::Empty);
    code.local_get(RHS)
        .i32_const(1)
        .i32_eq()
        .local_get(RHS)
        .i32_const(PAGE_FAULT)
        .i32_eq()
        .i32_or()
        .if_(BlockType::Empty);
    code.local_get(RHS)
        .i32_const(PAGE_FAULT)
        .i32_eq()
        .if_(BlockType::Result(ValType::I32))
        .i32_const(23)
        .else_()
        .i32_const(5)
        .end()
        .local_set(REASON);
    field(code, GATHER_DETAIL_OFFSET);
    code.local_set(DETAIL);
    field(code, GATHER_FAULT_ADDRESS_OFFSET);
    code.local_set(FAULT_ADDRESS);
    code.i32_const(1)
        .local_set(FAULT_ACCESS)
        .i32_const(width as i32)
        .local_set(FAULT_LENGTH);
    code.else_()
        .i32_const(if segment == SegmentRegister::Ss {
            15
        } else {
            11
        })
        .local_set(REASON)
        .i32_const(0)
        .local_set(DETAIL);
    for local in [FAULT_ADDRESS, FAULT_ACCESS, FAULT_LENGTH] {
        code.i32_const(0).local_set(local);
    }
    code.end().end();
}
