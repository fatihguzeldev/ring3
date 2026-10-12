use wasm_encoder::{BlockType, InstructionSink};

use super::{
    atomic,
    locals::{ADDRESS, FLAGS, LHS, RESULT, STATE_PTR, register},
    memory,
};
use crate::cpu::x86::{
    Register32,
    ir::{Location32, SmallWidth, Value32},
};

pub(super) fn verify(
    code: &mut InstructionSink<'_>,
    source: Value32,
    write: bool,
    imports: memory::Imports,
    exit_depth: u32,
) {
    match source {
        Value32::Register(source) => {
            code.local_get(register(source));
        }
        Value32::Memory(address) => {
            memory::load_narrow_value(code, address, SmallWidth::Word, imports, exit_depth);
        }
        Value32::Immediate(_) => unreachable!("segment verification has a selector source"),
    }
    code.i32_const(0xffff).i32_and().local_set(RESULT);
    atomic::payload(code);
    code.local_get(STATE_PTR)
        .local_get(RESULT)
        .i32_store(atomic::transfer(0));
    code.i32_const(0).local_set(ADDRESS);
    atomic::call(code, 0x30001 + u32::from(write), 0, imports, exit_depth);
    code.local_get(STATE_PTR).i32_load(atomic::transfer(4));
    super::cpu_result::flags(code, 0x40, exit_depth);
}

pub(super) fn adjust_privilege(
    code: &mut InstructionSink<'_>,
    destination: Location32,
    source: Register32,
    imports: memory::Imports,
    exit_depth: u32,
) -> bool {
    match destination {
        Location32::Register(destination) => {
            code.local_get(register(source))
                .i32_const(3)
                .i32_and()
                .local_set(RESULT);
            code.local_get(register(destination))
                .i32_const(3)
                .i32_and()
                .local_get(RESULT)
                .i32_lt_u()
                .local_set(LHS);
            code.local_get(LHS)
                .if_(BlockType::Empty)
                .local_get(register(destination))
                .i32_const(!3)
                .i32_and()
                .local_get(RESULT)
                .i32_or()
                .local_set(register(destination))
                .end();
            code.local_get(FLAGS)
                .i32_const(!0x40)
                .i32_and()
                .local_get(LHS)
                .i32_const(6)
                .i32_shl()
                .i32_or()
                .local_set(FLAGS);
            false
        }
        Location32::Memory(address) => {
            let segment = address
                .segment
                .expect("ARPL decoder retains the effective segment");
            atomic::payload(code);
            code.local_get(STATE_PTR)
                .local_get(register(source))
                .i32_const(0xffff)
                .i32_and()
                .i32_store(atomic::transfer(0));
            code.local_get(STATE_PTR);
            memory::offset_value(code, address);
            code.local_tee(ADDRESS).i32_store(atomic::transfer(8));
            code.local_get(STATE_PTR)
                .i32_const(segment.index() as i32)
                .i32_store(atomic::transfer(12));
            code.local_get(ADDRESS);
            super::segments::base(code, segment);
            code.i32_add().local_set(ADDRESS);
            atomic::call(code, 0x30000, 2, imports, exit_depth);
            code.local_get(STATE_PTR).i32_load(atomic::transfer(4));
            super::cpu_result::flags(code, 0x40, exit_depth);
            true
        }
    }
}
