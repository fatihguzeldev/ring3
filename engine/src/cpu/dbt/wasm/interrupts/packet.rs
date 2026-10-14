use wasm_encoder::{BlockType, InstructionSink};

use super::super::{cpu_result, locals::*, memory, segments, stack};
use crate::cpu::x86::{Register32, SegmentRegister};

pub(super) fn validate(
    code: &mut InstructionSink<'_>,
    vector: u8,
    original_flags: u32,
    exit_depth: u32,
) {
    code.local_get(HELPER_STATUS)
        .i32_eqz()
        .i32_eqz()
        .local_get(HELPER_STATUS)
        .i32_const(11)
        .i32_ne()
        .i32_and()
        .if_(BlockType::Empty);
    memory::infrastructure(code, 3);
    memory::exit_if_failed(code, exit_depth + 1);
    code.end();
    memory::helper_field(code, 36);
    code.i32_const(0xff)
        .i32_and()
        .i32_eqz()
        .if_(BlockType::Empty);
    cpu_result::validate_interrupt(code, 0, vector, original_flags, exit_depth + 1);
    code.else_();
    memory::helper_field(code, 36);
    code.i32_const(0xff)
        .i32_and()
        .i32_const(2)
        .i32_eq()
        .if_(BlockType::Empty);
    frame(code, 2, vector, original_flags, exit_depth + 2);
    code.else_();
    memory::helper_field(code, 36);
    code.i32_const(0xff)
        .i32_and()
        .i32_const(4)
        .i32_eq()
        .if_(BlockType::Empty);
    frame(code, 4, vector, original_flags, exit_depth + 3);
    code.else_();
    memory::infrastructure(code, 2);
    memory::exit_if_failed(code, exit_depth + 3);
    code.end().end().end();
}

fn frame(
    code: &mut InstructionSink<'_>,
    slot: u32,
    vector: u8,
    original_flags: u32,
    exit_depth: u32,
) {
    // the fault can name any of the three modular linear slots; SS.B owns pointer wrapping.
    address(code, slot, 1);
    for index in [2, 3] {
        memory::helper_field(code, 28);
        code.local_get(ADDRESS)
            .i32_sub()
            .i32_const(slot as i32 - 1)
            .i32_gt_u()
            .if_(BlockType::Empty);
        address(code, slot, index);
        code.end();
    }
    cpu_result::validate_interrupt(code, slot, vector, original_flags, exit_depth);
    stack::pointer::advance(code, -((3 * slot) as i32));
}

fn address(code: &mut InstructionSink<'_>, slot: u32, index: u32) {
    stack::pointer::offset(code, register(Register32::Esp), -((index * slot) as i32));
    segments::base(code, SegmentRegister::Ss);
    code.i32_add().local_set(ADDRESS);
}
