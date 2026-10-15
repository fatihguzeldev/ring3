use crate::cpu::dbt::wasm::{helpers::integer as integer_helper, imports::Imports};
use crate::cpu::{
    dbt::wasm::{
        instructions::memory,
        locals::{ADDRESS, FLAGS, LHS, RESULT, STATE_PTR, register},
    },
    x86::{
        Register32, SegmentQuery,
        ir::{Location32, SmallWidth, Value32},
    },
};
use wasm_encoder::{BlockType, InstructionSink};
pub(in crate::cpu::dbt::wasm) fn verify(
    code: &mut InstructionSink<'_>,
    source: Value32,
    write: bool,
    imports: Imports,
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
    integer_helper::payload(code);
    code.local_get(STATE_PTR)
        .local_get(RESULT)
        .i32_store(integer_helper::transfer(0));
    code.i32_const(0).local_set(ADDRESS);
    integer_helper::call(code, 0x30001 + u32::from(write), 0, imports, exit_depth);
    code.local_get(STATE_PTR)
        .i32_load(integer_helper::transfer(4));
    crate::cpu::dbt::wasm::helpers::result::flags(code, 0x40, exit_depth);
}

pub(in crate::cpu::dbt::wasm) fn query(
    code: &mut InstructionSink<'_>,
    kind: SegmentQuery,
    source: Value32,
    destination: Register32,
    word: bool,
    imports: Imports,
    exit_depth: u32,
) {
    match source {
        Value32::Register(source) => {
            code.local_get(register(source));
        }
        Value32::Memory(address) => {
            memory::load_narrow_value(code, address, SmallWidth::Word, imports, exit_depth);
        }
        Value32::Immediate(_) => unreachable!("descriptor queries have a selector source"),
    }
    code.i32_const(0xffff).i32_and().local_set(RESULT);
    integer_helper::payload(code);
    code.local_get(STATE_PTR)
        .local_get(RESULT)
        .i32_store(integer_helper::transfer(0));
    code.i32_const(0).local_set(ADDRESS);
    let descriptor = match kind {
        SegmentQuery::AccessRights => 0x30003,
        SegmentQuery::Limit => 0x30004,
    };
    integer_helper::call(code, descriptor, 0, imports, exit_depth);
    code.local_get(STATE_PTR)
        .i32_load(integer_helper::transfer(4));
    crate::cpu::dbt::wasm::helpers::result::flags(code, 0x40, exit_depth);
    // source and helper validation precede either architectural destination publication.
    code.local_get(FLAGS)
        .i32_const(0x40)
        .i32_and()
        .if_(BlockType::Empty);
    if word {
        code.local_get(register(destination))
            .i32_const(!0xffff)
            .i32_and();
    }
    code.local_get(STATE_PTR)
        .i32_load(integer_helper::transfer(0));
    if word {
        code.i32_const(0xffff).i32_and().i32_or();
    }
    code.local_set(register(destination)).end();
}

pub(in crate::cpu::dbt::wasm) fn adjust_privilege(
    code: &mut InstructionSink<'_>,
    destination: Location32,
    source: Register32,
    imports: Imports,
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
            integer_helper::payload(code);
            code.local_get(STATE_PTR)
                .local_get(register(source))
                .i32_const(0xffff)
                .i32_and()
                .i32_store(integer_helper::transfer(0));
            code.local_get(STATE_PTR);
            memory::offset_value(code, address);
            code.local_tee(ADDRESS)
                .i32_store(integer_helper::transfer(8));
            code.local_get(STATE_PTR)
                .i32_const(segment.index() as i32)
                .i32_store(integer_helper::transfer(12));
            code.local_get(ADDRESS);
            crate::cpu::dbt::wasm::instructions::segments::base(code, segment);
            code.i32_add().local_set(ADDRESS);
            integer_helper::call(code, 0x30000, 2, imports, exit_depth);
            code.local_get(STATE_PTR)
                .i32_load(integer_helper::transfer(4));
            crate::cpu::dbt::wasm::helpers::result::flags(code, 0x40, exit_depth);
            true
        }
    }
}
