use wasm_encoder::{BlockType, InstructionSink, MemArg};

use crate::cpu::x86::ir::{EffectiveAddress, Location32, Operation, Value32};

use super::locals::*;

#[derive(Clone, Copy, Default)]
pub(super) struct Imports {
    pub(super) read: Option<u32>,
    pub(super) store: Option<u32>,
}

impl Imports {
    pub(super) fn needed(blocks: &[crate::cpu::dbt::region::CompiledBlock]) -> (bool, bool) {
        let mut read = false;
        let mut store = false;
        for instruction in blocks.iter().flat_map(|block| &block.instructions) {
            match instruction.operation() {
                Operation::Move {
                    source: Value32::Memory(_),
                    ..
                } => read = true,
                Operation::Move {
                    destination: Location32::Memory(_),
                    ..
                } => store = true,
                _ => {}
            }
        }
        (read, store)
    }
}

pub(super) fn load(
    code: &mut InstructionSink<'_>,
    address: EffectiveAddress,
    destination: crate::cpu::x86::Register32,
    imports: Imports,
    exit_depth: u32,
) {
    effective_address(code, address);
    code.local_get(ADDRESS)
        .call(imports.read.expect("prepared memory load has an import"))
        .local_set(HELPER_STATUS);
    validate_result(code, false);
    exit_if_failed(code, exit_depth);
    helper_field(code, 20);
    code.local_set(register(destination));
}

pub(super) fn store(
    code: &mut InstructionSink<'_>,
    address: EffectiveAddress,
    source: Value32,
    imports: Imports,
    exit_depth: u32,
) {
    effective_address(code, address);
    code.local_get(ADDRESS);
    match source {
        Value32::Register(source) => {
            code.local_get(register(source));
        }
        Value32::Immediate(value) => {
            code.i32_const(value as i32);
        }
        Value32::Memory(_) => unreachable!("prepared move cannot have two memory operands"),
    }
    code.call(imports.store.expect("prepared memory store has an import"))
        .local_set(HELPER_STATUS);
    validate_result(code, true);
    exit_if_failed(code, exit_depth);
}

pub(super) fn exit_if_invalidated(code: &mut InstructionSink<'_>, exit_depth: u32) {
    code.local_get(HELPER_STATUS)
        .i32_const(11)
        .i32_eq()
        .if_(BlockType::Empty)
        .i32_const(6)
        .local_set(REASON)
        .br(exit_depth + 1)
        .end();
}

fn effective_address(code: &mut InstructionSink<'_>, address: EffectiveAddress) {
    code.i32_const(address.displacement as i32);
    if let Some(base) = address.base {
        code.local_get(register(base)).i32_add();
    }
    if let Some(index) = address.index {
        code.local_get(register(index))
            .i32_const(i32::from(address.scale))
            .i32_mul()
            .i32_add();
    }
    code.local_set(ADDRESS);
}

fn validate_result(code: &mut InstructionSink<'_>, store: bool) {
    code.local_get(HELPER_STATUS).i32_eqz();
    if store {
        code.local_get(HELPER_STATUS)
            .i32_const(11)
            .i32_eq()
            .i32_or();
    }
    code.i32_eqz().if_(BlockType::Empty);
    infrastructure(code, 3);
    code.else_();
    reset_checks(code);
    for (offset, expected) in [
        (0, u32::from_le_bytes(*b"R3MH")),
        (4, 0x0001_0001),
        (8, 40),
        (12, 0),
    ] {
        expect_field(code, offset, expected);
    }
    code.local_get(LHS).if_(BlockType::Empty);
    infrastructure(code, 2);
    code.else_();
    helper_field(code, 16);
    code.local_set(RHS)
        .local_get(RHS)
        .i32_eqz()
        .if_(BlockType::Empty);
    reset_checks(code);
    if store {
        expect_field(code, 20, 0);
    }
    zero_fault_fields(code);
    protocol_if_bad(code);
    code.else_()
        .local_get(RHS)
        .i32_const(1)
        .i32_eq()
        .if_(BlockType::Empty);
    validate_fault(code, store);
    code.else_()
        .local_get(RHS)
        .i32_const(2)
        .i32_eq()
        .if_(BlockType::Empty);
    validate_infrastructure(code, store);
    code.else_();
    infrastructure(code, 2);
    code.end().end().end().end().end();
}

fn validate_fault(code: &mut InstructionSink<'_>, store: bool) {
    reset_checks(code);
    expect_field(code, 20, 0);
    expect_field(code, 32, if store { 2 } else { 1 });
    expect_field(code, 36, 4);
    if store {
        code.local_get(HELPER_STATUS).i32_const(11).i32_eq();
        add_check(code);
    }
    helper_field(code, 24);
    code.i32_const(1).i32_sub().i32_const(2).i32_gt_u();
    add_check(code);
    helper_field(code, 24);
    code.i32_const(3)
        .i32_eq()
        .if_(BlockType::Result(wasm_encoder::ValType::I32));
    code.local_get(ADDRESS).i32_const(-4).i32_le_u();
    helper_field(code, 28);
    code.local_get(ADDRESS).i32_ne().i32_or().else_();
    code.local_get(ADDRESS).i32_const(-4).i32_gt_u();
    helper_field(code, 28);
    code.i32_const(-4).i32_gt_u().i32_or();
    helper_field(code, 28);
    code.local_get(ADDRESS)
        .i32_sub()
        .i32_const(3)
        .i32_gt_u()
        .i32_or()
        .end();
    add_check(code);
    code.local_get(LHS).if_(BlockType::Empty);
    infrastructure(code, 2);
    code.else_().i32_const(5).local_set(REASON);
    for (offset, local) in [
        (24, DETAIL),
        (28, FAULT_ADDRESS),
        (32, FAULT_ACCESS),
        (36, FAULT_LENGTH),
    ] {
        helper_field(code, offset);
        code.local_set(local);
    }
    code.end();
}

fn validate_infrastructure(code: &mut InstructionSink<'_>, store: bool) {
    reset_checks(code);
    for offset in [20, 28, 32, 36] {
        expect_field(code, offset, 0);
    }
    if store {
        code.local_get(HELPER_STATUS).i32_const(11).i32_eq();
        add_check(code);
    }
    helper_field(code, 24);
    code.i32_const(1).i32_sub().i32_const(1).i32_gt_u();
    add_check(code);
    code.local_get(LHS).if_(BlockType::Empty);
    infrastructure(code, 2);
    code.else_();
    helper_field(code, 24);
    code.i32_const(1).i32_eq().if_(BlockType::Empty);
    infrastructure(code, 1);
    code.else_();
    infrastructure(code, 3);
    code.end().end();
}

fn zero_fault_fields(code: &mut InstructionSink<'_>) {
    for offset in [24, 28, 32, 36] {
        expect_field(code, offset, 0);
    }
}

fn infrastructure(code: &mut InstructionSink<'_>, detail: i32) {
    code.i32_const(7)
        .local_set(REASON)
        .i32_const(detail)
        .local_set(DETAIL);
}

fn protocol_if_bad(code: &mut InstructionSink<'_>) {
    code.local_get(LHS).if_(BlockType::Empty);
    infrastructure(code, 2);
    code.end();
}

fn exit_if_failed(code: &mut InstructionSink<'_>, exit_depth: u32) {
    code.local_get(REASON)
        .if_(BlockType::Empty)
        .br(exit_depth + 1)
        .end();
}

fn reset_checks(code: &mut InstructionSink<'_>) {
    code.i32_const(0).local_set(LHS);
}

fn expect_field(code: &mut InstructionSink<'_>, offset: u64, expected: u32) {
    helper_field(code, offset);
    code.i32_const(expected as i32).i32_ne();
    add_check(code);
}

fn add_check(code: &mut InstructionSink<'_>) {
    code.local_get(LHS).i32_or().local_set(LHS);
}

fn helper_field(code: &mut InstructionSink<'_>, offset: u64) {
    code.local_get(STATE_PTR).i32_load(MemArg {
        offset: 100 + offset,
        align: 2,
        memory_index: 0,
    });
}
