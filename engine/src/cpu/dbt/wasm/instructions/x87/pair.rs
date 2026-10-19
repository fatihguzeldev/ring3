use crate::cpu::dbt::{
    X87_PAIR_COMPLETED_SHIFT, X87_PAIR_REPORTED_SHIFT, X87PairStep32,
    wasm::{
        helpers::{protocol, result},
        imports::Imports,
        locals::*,
    },
};
use wasm_encoder::{BlockType, InstructionSink};

#[derive(Clone, Copy)]
pub(in crate::cpu::dbt::wasm) struct PairLocals {
    limit: u32,
    first_address: u32,
    second_address: u32,
    completed: u32,
    reported_step: u32,
    progress: u32,
}

impl PairLocals {
    pub(in crate::cpu::dbt::wasm) const fn new(first: u32) -> Self {
        Self {
            limit: first,
            first_address: first + 1,
            second_address: first + 2,
            completed: first + 3,
            reported_step: first + 4,
            progress: first + 5,
        }
    }
}

/// capture debug restrictions before begin_instruction clears rf and ss shadow.
pub(in crate::cpu::dbt::wasm) fn capture_limit(code: &mut InstructionSink<'_>, imports: Imports) {
    let locals = imports.x87_pair_locals.expect("x87 pair locals");
    code.i32_const(1)
        .local_get(BUDGET)
        .i32_const(2)
        .i32_ge_u()
        .local_get(FLAGS)
        .i32_const(0x10100)
        .i32_and()
        .i32_eqz()
        .i32_and()
        .local_get(DEBUG_SHADOW)
        .i32_eqz()
        .i32_and()
        .i32_add()
        .local_set(locals.limit);
}

pub(in crate::cpu::dbt::wasm) fn operation(
    code: &mut InstructionSink<'_>,
    pair: [X87PairStep32; 2],
    imports: Imports,
    exit_depth: u32,
) {
    let locals = imports.x87_pair_locals.expect("x87 pair locals");
    for (step, local) in pair
        .into_iter()
        .zip([locals.first_address, locals.second_address])
    {
        if let Some(address) = step.address {
            crate::cpu::dbt::wasm::instructions::memory::address_value(code, address);
        } else {
            code.i32_const(0);
        }
        code.local_set(local);
    }
    let import = imports.x87_pair.expect("resident x87 pair helper");
    let index = import.emit_binding(code);
    code.i32_const(pair[0].pc as i32)
        .local_get(locals.first_address)
        .local_get(locals.second_address)
        .local_get(FLAGS)
        .local_get(locals.limit)
        .call(index)
        .local_tee(locals.progress)
        .i32_wrap_i64()
        .local_set(HELPER_STATUS);
    for (shift, mask, local) in [
        (X87_PAIR_COMPLETED_SHIFT, 3, locals.completed),
        (X87_PAIR_REPORTED_SHIFT, 1, locals.reported_step),
    ] {
        code.local_get(locals.progress)
            .i64_const(i64::from(shift))
            .i64_shr_u()
            .i32_wrap_i64()
            .i32_const(mask)
            .i32_and()
            .local_set(local);
    }
    check_progress(code, locals, exit_depth);

    // catch the ordinary validator's terminal branch locally so a completed
    // first instruction is accounted before reporting the second's fault.
    code.block(BlockType::Empty)
        .local_get(locals.reported_step)
        .if_(BlockType::Empty);
    select_step(code, pair[1], locals.second_address);
    let first_width = pair[0].operation.read_size() as u32;
    let second_width = pair[1].operation.read_size() as u32;
    if first_width != second_width {
        validate_packet(code, second_width, imports, 1);
    }
    code.else_();
    select_step(code, pair[0], locals.first_address);
    if first_width != second_width {
        validate_packet(code, first_width, imports, 1);
    }
    code.end();
    if first_width == second_width {
        validate_packet(code, first_width, imports, 0);
    }
    code.end();

    // a malformed reply cannot retire the reported instruction itself. only
    // the preceding completed prefix survives transport or packet failure.
    code.local_get(REASON)
        .if_(BlockType::Empty)
        .local_get(locals.reported_step)
        .local_set(locals.completed)
        .else_()
        .local_get(locals.completed)
        .i32_const(2)
        .i32_eq()
        .if_(BlockType::Result(wasm_encoder::ValType::I32))
        .i32_const(pair[1].next_pc as i32)
        .else_()
        .i32_const(pair[1].pc as i32)
        .end()
        .local_set(EIP)
        .end();
    code.local_get(RETIRED)
        .local_get(locals.completed)
        .i32_add()
        .local_set(RETIRED)
        .local_get(BUDGET)
        .local_get(locals.completed)
        .i32_sub()
        .local_set(BUDGET);
    protocol::exit_if_failed(code, exit_depth);
}

fn check_progress(code: &mut InstructionSink<'_>, locals: PairLocals, exit_depth: u32) {
    protocol::reset_checks(code);
    code.local_get(locals.progress)
        .i64_const(35)
        .i64_shr_u()
        .i64_const(0)
        .i64_ne();
    protocol::add_check(code);
    code.local_get(locals.completed)
        .local_get(locals.limit)
        .i32_gt_u();
    protocol::add_check(code);
    code.local_get(locals.reported_step)
        .local_get(locals.limit)
        .i32_ge_u();
    protocol::add_check(code);
    code.local_get(locals.completed)
        .local_get(locals.reported_step)
        .i32_sub()
        .i32_const(1)
        .i32_gt_u();
    protocol::add_check(code);
    // status23 is an existing certified page-fault reply. every nonzero
    // transport status reports no retirement for its reported instruction.
    code.local_get(locals.completed)
        .local_get(locals.reported_step)
        .local_get(HELPER_STATUS)
        .i32_eqz()
        .if_(BlockType::Result(wasm_encoder::ValType::I32));
    protocol::helper_field(code, 16);
    code.i32_eqz().else_().i32_const(0).end().i32_add().i32_ne();
    protocol::add_check(code);
    protocol::protocol_if_bad(code);
    protocol::exit_if_failed(code, exit_depth);
}

fn select_step(code: &mut InstructionSink<'_>, step: X87PairStep32, address: u32) {
    code.i32_const(step.pc as i32)
        .local_set(EIP)
        .local_get(address)
        .local_set(ADDRESS);
}

fn validate_packet(code: &mut InstructionSink<'_>, length: u32, imports: Imports, exit_depth: u32) {
    result::validate_x87(
        code,
        length,
        u32::from(length != 0),
        false,
        exit_depth,
        result::ResultContext::from(imports),
    );
    result::value(code);
    result::flags(code, 0, exit_depth);
}

pub(in crate::cpu::dbt::wasm) fn skip_completed_second(
    code: &mut InstructionSink<'_>,
    imports: Imports,
    remaining: u32,
) {
    debug_assert!(remaining >= 1);
    let locals = imports.x87_pair_locals.expect("x87 pair locals");
    // depth one skips the second instruction and lands on the next ordinary
    // boundary. a terminal pair retains the loop's safepoint and context check.
    let depth = if remaining > 1 { 1 } else { remaining + 1 };
    code.local_get(locals.completed)
        .i32_const(2)
        .i32_eq()
        .br_if(depth);
}
