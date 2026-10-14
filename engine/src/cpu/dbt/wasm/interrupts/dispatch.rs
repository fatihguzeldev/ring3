use wasm_encoder::{BlockType, InstructionSink};

use super::super::{locals::*, memory};
use crate::cpu::x86::Register32;

pub(in crate::cpu::dbt::wasm) fn dispatch(
    code: &mut InstructionSink<'_>,
    vector: u8,
    return_eip: u32,
    imports: memory::Imports,
    exit_depth: u32,
) {
    let helper = imports
        .software_interrupt
        .expect("interrupt gate helper")
        .emit_binding(code);
    code.i32_const(i32::from(vector))
        .i32_const(return_eip as i32)
        .local_get(register(Register32::Esp))
        .local_get(FLAGS)
        .call(helper)
        .local_set(HELPER_STATUS);
    super::packet::validate(
        code,
        vector,
        imports
            .software_interrupt_flags
            .expect("interrupt flags capture"),
        exit_depth,
    );
    super::super::cpu_result::value(code);
    code.local_set(EIP)
        .local_get(FLAGS)
        .i32_const(!0x0003_4100)
        .i32_and()
        .local_set(FLAGS);
    memory::helper_field(code, 36);
    code.i32_const(0x100)
        .i32_and()
        .if_(BlockType::Empty)
        .local_get(FLAGS)
        .i32_const(!0x200)
        .i32_and()
        .local_set(FLAGS)
        .end()
        .i32_const(0)
        .local_set(SNAPSHOT_TF);
}
