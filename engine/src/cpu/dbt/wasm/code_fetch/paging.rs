use super::super::{
    locals::*,
    memory::{self, Imports},
};
use crate::cpu::x86::{SegmentRegister, decode::CapturedCodeFault};
use wasm_encoder::{BlockType, InstructionSink};

pub(in crate::cpu::dbt::wasm) fn fault(
    code: &mut InstructionSink<'_>,
    fault: &CapturedCodeFault,
    imports: Imports,
) {
    let length = u32::from(fault.attempted_length);
    let import = imports
        .fetch_fault
        .expect("paging terminal has an execution helper")
        .emit_binding(code);
    code.i32_const(fault.pc.0 as i32)
        .i32_const(length as i32)
        .call(import)
        .local_set(HELPER_STATUS);
    code.local_get(HELPER_STATUS)
        .i32_eqz()
        .local_get(HELPER_STATUS)
        .i32_const(crate::abi::memory_helper::CPU_PAGE_FAULT_TRANSPORT as i32)
        .i32_eq()
        .i32_or()
        .i32_eqz()
        .if_(BlockType::Empty);
    memory::infrastructure(code, 3);
    code.else_();
    memory::reset_checks(code);
    for (offset, expected) in [
        (0, u32::from_le_bytes(*b"R3MH")),
        (8, 40),
        (12, 0),
        (20, 0),
        (36, length),
    ] {
        memory::expect_field(code, offset, expected);
    }
    memory::helper_field(code, 16);
    code.local_set(RHS)
        .local_get(RHS)
        .i32_const(9)
        .i32_eq()
        .local_get(HELPER_STATUS)
        .i32_const(crate::abi::memory_helper::CPU_PAGE_FAULT_TRANSPORT as i32)
        .i32_eq()
        .i32_xor();
    memory::add_check(code);
    memory::helper_field(code, 4);
    code.i32_const(0x10007).i32_eq().if_(BlockType::Empty);
    for offset in [24, 28, 32] {
        memory::expect_field(code, offset, 0);
    }
    code.local_get(RHS)
        .i32_eqz()
        .local_get(RHS)
        .i32_const(3)
        .i32_eq()
        .i32_or()
        .i32_eqz();
    memory::add_check(code);
    code.local_get(RHS)
        .i32_eqz()
        .if_(BlockType::Result(wasm_encoder::ValType::I32))
        .i32_const(3)
        .else_()
        .i32_const(11)
        .end()
        .local_set(REASON);
    for local in [DETAIL, FAULT_ADDRESS, FAULT_ACCESS, FAULT_LENGTH] {
        code.i32_const(0).local_set(local);
    }
    memory::protocol_if_bad(code);
    code.else_();
    memory::expect_field(code, 4, 0x1000f);
    super::super::segments::base(code, SegmentRegister::Cs);
    code.i32_const(fault.pc.0 as i32)
        .i32_add()
        .local_set(ADDRESS);
    memory::helper_field(code, 28);
    code.local_get(ADDRESS)
        .i32_sub()
        .i32_const(length as i32)
        .i32_ge_u();
    memory::add_check(code);
    memory::expect_field(code, 32, 3);
    code.local_get(RHS)
        .i32_const(9)
        .i32_eq()
        .if_(BlockType::Empty);
    memory::helper_field(code, 24);
    code.i32_const(20).i32_sub().i32_const(1).i32_gt_u();
    memory::add_check(code);
    code.i32_const(23).local_set(REASON);
    code.else_();
    memory::expect_field(code, 16, 1);
    memory::helper_field(code, 24);
    code.i32_const(1).i32_sub().i32_const(1).i32_gt_u();
    memory::add_check(code);
    code.i32_const(5).local_set(REASON);
    code.end();
    for (offset, local) in [
        (24, DETAIL),
        (28, FAULT_ADDRESS),
        (32, FAULT_ACCESS),
        (36, FAULT_LENGTH),
    ] {
        memory::helper_field(code, offset);
        code.local_set(local);
    }
    memory::protocol_if_bad(code);
    code.end().end();
}
