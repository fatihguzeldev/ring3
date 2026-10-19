use super::pair::{PacketContract, select_step, validate_packet};
use crate::cpu::dbt::{
    X87_GROUP_COMPLETED_SHIFT, X87_GROUP_MAX_STEPS, X87_GROUP_REPORTED_SHIFT, X87Group32,
    wasm::{helpers::protocol, imports::Imports, locals::*},
};
use wasm_encoder::{BlockType, InstructionSink, ValType};

pub(in crate::cpu::dbt::wasm) const INTEGER_LOCALS: u32 = 11;

#[derive(Clone, Copy)]
pub(in crate::cpu::dbt::wasm) struct GroupLocals {
    limit: u32,
    first_address: u32,
    completed: u32,
    reported_step: u32,
    progress: u32,
}

impl GroupLocals {
    pub(in crate::cpu::dbt::wasm) const fn new(first: u32) -> Self {
        Self {
            limit: first,
            first_address: first + 1,
            completed: first + 9,
            reported_step: first + 10,
            progress: first + INTEGER_LOCALS,
        }
    }

    fn address(self, index: usize) -> u32 {
        debug_assert!(index < X87_GROUP_MAX_STEPS);
        self.first_address + index as u32
    }
}

/// original debug metadata controls batching before begin_instruction clears it.
pub(in crate::cpu::dbt::wasm) fn capture_limit(
    code: &mut InstructionSink<'_>,
    group: X87Group32,
    imports: Imports,
) {
    let locals = imports.x87_group_locals.expect("x87 group locals");
    code.local_get(BUDGET)
        .i32_const(group.len() as i32)
        .i32_lt_u()
        .if_(BlockType::Result(ValType::I32))
        .local_get(BUDGET)
        .else_()
        .i32_const(group.len() as i32)
        .end()
        .local_set(locals.limit)
        .local_get(FLAGS)
        .i32_const(0x10100)
        .i32_and()
        .local_get(DEBUG_SHADOW)
        .i32_or()
        .if_(BlockType::Empty)
        .i32_const(1)
        .local_set(locals.limit)
        .end();
}

pub(in crate::cpu::dbt::wasm) fn operation(
    code: &mut InstructionSink<'_>,
    ordinal: u32,
    group: X87Group32,
    imports: Imports,
    exit_depth: u32,
) {
    let locals = imports.x87_group_locals.expect("x87 group locals");
    for (index, step) in group.steps().iter().enumerate() {
        if let Some(address) = step.address {
            crate::cpu::dbt::wasm::instructions::memory::address_value(code, address);
        } else {
            code.i32_const(0);
        }
        code.local_set(locals.address(index));
    }
    let import = imports.x87_group.expect("resident x87 group helper");
    let index = import.emit_binding(code);
    code.i32_const(ordinal as i32);
    for index in 0..X87_GROUP_MAX_STEPS {
        if index < group.len() {
            code.local_get(locals.address(index));
        } else {
            code.i32_const(0);
        }
    }
    code.local_get(FLAGS)
        .local_get(locals.limit)
        .call(index)
        .local_tee(locals.progress)
        .i32_wrap_i64()
        .local_set(HELPER_STATUS);
    for (shift, mask, local) in [
        (X87_GROUP_COMPLETED_SHIFT, 15, locals.completed),
        (X87_GROUP_REPORTED_SHIFT, 7, locals.reported_step),
    ] {
        code.local_get(locals.progress)
            .i64_const(i64::from(shift))
            .i64_shr_u()
            .i32_wrap_i64()
            .i32_const(mask)
            .i32_and()
            .local_set(local);
    }
    let final_store = group.steps().last().unwrap().operation.write_size() != 0;
    check_progress(code, locals, group.len() as u32, final_store, exit_depth);

    // select the reported instruction before invoking one validator for its
    // packet contract; intermediate instructions do not duplicate validator bodies.
    code.block(BlockType::Empty);
    for (index, step) in group.steps().iter().enumerate() {
        code.local_get(locals.reported_step)
            .i32_const(index as i32)
            .i32_eq()
            .if_(BlockType::Empty);
        select_step(code, *step, locals.address(index));
        code.br(1).end();
    }
    code.end();
    validate_reported_packet(code, group, locals, imports);

    code.local_get(REASON)
        .if_(BlockType::Empty)
        .local_get(locals.reported_step)
        .local_set(locals.completed)
        .else_();
    for (index, step) in group.steps().iter().enumerate() {
        code.local_get(locals.completed)
            .i32_const(index as i32 + 1)
            .i32_eq()
            .if_(BlockType::Empty)
            .i32_const(step.next_pc as i32)
            .local_set(EIP)
            .end();
    }
    code.end()
        .local_get(RETIRED)
        .local_get(locals.completed)
        .i32_add()
        .local_set(RETIRED)
        .local_get(BUDGET)
        .local_get(locals.completed)
        .i32_sub()
        .local_set(BUDGET);
    protocol::exit_if_failed(code, exit_depth);
    if final_store {
        protocol::exit_if_invalidated(code, exit_depth, imports);
    }
}

fn validate_reported_packet(
    code: &mut InstructionSink<'_>,
    group: X87Group32,
    locals: GroupLocals,
    imports: Imports,
) {
    let mut contracts = [PacketContract::from(group.steps()[0]); X87_GROUP_MAX_STEPS];
    let mut count = 0;
    for step in group.steps() {
        let contract = PacketContract::from(*step);
        if !contracts[..count].contains(&contract) {
            contracts[count] = contract;
            count += 1;
        }
    }
    // catch failure locally so the completed architectural prefix is retired
    // before the reported instruction's fault exits the generated region.
    code.block(BlockType::Empty);
    for (index, contract) in contracts[..count].iter().enumerate() {
        let last = index + 1 == count;
        if !last {
            let mut first = true;
            for (step_index, step) in group.steps().iter().enumerate() {
                if PacketContract::from(*step) == *contract {
                    code.local_get(locals.reported_step)
                        .i32_const(step_index as i32)
                        .i32_eq();
                    if !first {
                        code.i32_or();
                    }
                    first = false;
                }
            }
            code.if_(BlockType::Empty);
        }
        validate_packet(code, *contract, imports, u32::from(!last));
        if !last {
            code.br(1).end();
        }
    }
    code.end();
}

fn check_progress(
    code: &mut InstructionSink<'_>,
    locals: GroupLocals,
    length: u32,
    final_store: bool,
    exit_depth: u32,
) {
    protocol::reset_checks(code);
    code.local_get(locals.progress)
        .i64_const(39)
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
    code.local_get(HELPER_STATUS).i32_const(11).i32_eq();
    if final_store {
        code.if_(BlockType::Empty)
            .local_get(locals.completed)
            .i32_const(length as i32)
            .i32_ne()
            .local_get(locals.reported_step)
            .i32_const(length as i32 - 1)
            .i32_ne()
            .i32_or()
            .local_get(locals.limit)
            .i32_const(length as i32)
            .i32_ne()
            .i32_or();
        protocol::add_check(code);
        code.end();
    } else {
        protocol::add_check(code);
    }
    code.local_get(locals.completed)
        .local_get(locals.reported_step)
        .local_get(HELPER_STATUS)
        .i32_eqz();
    if final_store {
        code.local_get(HELPER_STATUS)
            .i32_const(11)
            .i32_eq()
            .i32_or();
    }
    code.if_(BlockType::Result(ValType::I32));
    protocol::helper_field(code, 16);
    code.i32_eqz().else_().i32_const(0).end().i32_add().i32_ne();
    protocol::add_check(code);
    protocol::protocol_if_bad(code);
    protocol::exit_if_failed(code, exit_depth);
}

pub(in crate::cpu::dbt::wasm) fn skip_completed(
    code: &mut InstructionSink<'_>,
    imports: Imports,
    length: usize,
    remaining: u32,
) {
    debug_assert!(length as u32 <= remaining + 1);
    let locals = imports.x87_group_locals.expect("x87 group locals");
    let depths = (2..length + 1).map(|completed| {
        let completed = u32::try_from(completed).expect("bounded x87 group length");
        if completed == remaining + 1 {
            remaining + 2
        } else {
            completed
        }
    });
    // the if contributes one label. nonterminal completions land on the next
    // original boundary; terminal completions retain the outer-loop safepoint.
    code.local_get(locals.completed)
        .i32_const(2)
        .i32_ge_u()
        .if_(BlockType::Empty)
        .local_get(locals.completed)
        .i32_const(2)
        .i32_sub()
        .br_table(depths, remaining + 2)
        .end();
}
