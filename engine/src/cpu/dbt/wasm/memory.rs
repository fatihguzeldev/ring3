use wasm_encoder::{BlockType, InstructionSink, MemArg};

use crate::cpu::x86::ir::{
    BinaryKind, BranchTarget, EffectiveAddress, Location32, Operation, SmallSource, SmallWidth,
    UnaryKind, Value32,
};

use super::locals::*;

mod byte_store;
mod narrow;

#[derive(Clone, Copy)]
pub(super) enum StoreImport {
    Replacement(u32),
    Resident { index: u32, key: u64, id: u64 },
}

impl StoreImport {
    fn emit_binding(self, code: &mut InstructionSink<'_>) -> u32 {
        match self {
            Self::Replacement(index) => index,
            Self::Resident { index, key, id } => {
                code.i32_const(key as u32 as i32)
                    .i32_const((key >> 32) as u32 as i32)
                    .i32_const(id as u32 as i32)
                    .i32_const((id >> 32) as u32 as i32);
                index
            }
        }
    }
}

#[derive(Clone, Copy, Default)]
pub(super) struct Imports {
    pub(super) read: Option<u32>,
    pub(super) store: Option<StoreImport>,
    pub(super) read8: Option<u32>,
    pub(super) read16: Option<u32>,
    pub(super) store8: Option<StoreImport>,
}

impl Imports {
    pub(super) fn needed(
        blocks: &[crate::cpu::dbt::region::CompiledBlock],
    ) -> (bool, bool, bool, bool, bool) {
        let mut read = false;
        let mut store = false;
        let mut read8 = false;
        let mut read16 = false;
        let mut store8 = false;
        for instruction in blocks.iter().flat_map(|block| &block.instructions) {
            match instruction.operation() {
                Operation::LoadByte { .. } | Operation::MemoryPredicateByte { .. } => read8 = true,
                Operation::StoreByte { .. } => store8 = true,
                Operation::MemoryUnaryByte { .. } => {
                    read8 = true;
                    store8 = true;
                }
                Operation::Extend {
                    source: SmallSource::Memory { width, .. },
                    ..
                } => match width {
                    SmallWidth::Byte => read8 = true,
                    SmallWidth::Word => read16 = true,
                },
                Operation::Push {
                    source: Value32::Memory(_),
                }
                | Operation::Pop {
                    destination: Location32::Memory(_),
                }
                | Operation::Call {
                    target: BranchTarget::Indirect(Location32::Memory(_)),
                }
                | Operation::Unary {
                    kind: UnaryKind::Inc | UnaryKind::Dec | UnaryKind::Not | UnaryKind::Neg,
                    destination: Location32::Memory(_),
                }
                | Operation::Shift {
                    destination: Location32::Memory(_),
                    ..
                }
                | Operation::Binary {
                    kind:
                        BinaryKind::Add
                        | BinaryKind::Adc
                        | BinaryKind::Sub
                        | BinaryKind::Sbb
                        | BinaryKind::And
                        | BinaryKind::Or
                        | BinaryKind::Xor,
                    destination: Location32::Memory(_),
                    source: Value32::Register(_) | Value32::Immediate(_),
                } => {
                    read = true;
                    store = true;
                }
                Operation::Move {
                    source: Value32::Memory(_),
                    ..
                }
                | Operation::Binary {
                    source: Value32::Memory(_),
                    ..
                }
                | Operation::Binary {
                    kind: BinaryKind::Cmp | BinaryKind::Test,
                    destination: Location32::Memory(_),
                    ..
                }
                | Operation::SignedMultiply {
                    source: Location32::Memory(_),
                    ..
                }
                | Operation::Return { .. }
                | Operation::Jump {
                    target: BranchTarget::Indirect(Location32::Memory(_)),
                }
                | Operation::Pop { .. } => read = true,
                Operation::Move {
                    destination: Location32::Memory(_),
                    ..
                }
                | Operation::Call { .. }
                | Operation::Push { .. } => store = true,
                _ => {}
            }
        }
        (read, store, read8, read16, store8)
    }
}

pub(super) fn store_byte(
    code: &mut InstructionSink<'_>,
    address: EffectiveAddress,
    source: crate::cpu::x86::ir::ByteValue,
    imports: Imports,
    exit_depth: u32,
) {
    byte_store::store(code, address, source, imports, exit_depth);
}

pub(super) fn store_byte_result(
    code: &mut InstructionSink<'_>,
    address: EffectiveAddress,
    imports: Imports,
    exit_depth: u32,
) {
    byte_store::store_result(code, address, imports, exit_depth);
}

pub(super) fn load_narrow_value(
    code: &mut InstructionSink<'_>,
    address: EffectiveAddress,
    width: SmallWidth,
    imports: Imports,
    exit_depth: u32,
) {
    narrow::load_value(code, address, width, imports, exit_depth);
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
    let index = imports
        .store
        .expect("prepared memory store has an import")
        .emit_binding(code);
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
    code.call(index).local_set(HELPER_STATUS);
    validate_result(code, true);
    exit_if_failed(code, exit_depth);
}

pub(super) fn push_memory(
    code: &mut InstructionSink<'_>,
    source: EffectiveAddress,
    imports: Imports,
    exit_depth: u32,
) {
    load_result(code, source, imports, exit_depth);
    store_result(
        code,
        EffectiveAddress {
            base: Some(crate::cpu::x86::Register32::Esp),
            index: None,
            scale: 1,
            displacement: (-4_i32) as u32,
        },
        imports,
        exit_depth,
    );
    code.local_get(ADDRESS)
        .local_set(register(crate::cpu::x86::Register32::Esp));
}

pub(super) fn pop_memory(
    code: &mut InstructionSink<'_>,
    mut destination: EffectiveAddress,
    imports: Imports,
    exit_depth: u32,
) {
    let esp = crate::cpu::x86::Register32::Esp;
    load_result(
        code,
        EffectiveAddress {
            base: Some(esp),
            index: None,
            scale: 1,
            displacement: 0,
        },
        imports,
        exit_depth,
    );
    if destination.base == Some(esp) {
        destination.displacement = destination.displacement.wrapping_add(4);
    }
    store_result(code, destination, imports, exit_depth);
    code.local_get(register(esp))
        .i32_const(4)
        .i32_add()
        .local_set(register(esp));
}

pub(super) fn load_result(
    code: &mut InstructionSink<'_>,
    source: EffectiveAddress,
    imports: Imports,
    exit_depth: u32,
) {
    effective_address(code, source);
    code.local_get(ADDRESS)
        .call(
            imports
                .read
                .expect("prepared memory operand has a read import"),
        )
        .local_set(HELPER_STATUS);
    validate_result(code, false);
    exit_if_failed(code, exit_depth);
    helper_field(code, 20);
    code.local_set(RESULT);
}

pub(super) fn store_result(
    code: &mut InstructionSink<'_>,
    destination: EffectiveAddress,
    imports: Imports,
    exit_depth: u32,
) {
    effective_address(code, destination);
    let index = imports
        .store
        .expect("prepared memory operand has a store import")
        .emit_binding(code);
    code.local_get(ADDRESS)
        .local_get(RESULT)
        .call(index)
        .local_set(HELPER_STATUS);
    validate_result(code, true);
    exit_if_failed(code, exit_depth);
}

pub(super) fn pop_register(
    code: &mut InstructionSink<'_>,
    destination: crate::cpu::x86::Register32,
    imports: Imports,
    exit_depth: u32,
) {
    let esp = crate::cpu::x86::Register32::Esp;
    load(
        code,
        EffectiveAddress {
            base: Some(esp),
            index: None,
            scale: 1,
            displacement: 0,
        },
        destination,
        imports,
        exit_depth,
    );
    if destination != esp {
        code.local_get(register(esp))
            .i32_const(4)
            .i32_add()
            .local_set(register(esp));
    }
}

pub(super) fn pop_return(
    code: &mut InstructionSink<'_>,
    stack_adjust: u16,
    imports: Imports,
    exit_depth: u32,
) {
    let esp = register(crate::cpu::x86::Register32::Esp);
    code.local_get(esp)
        .local_set(ADDRESS)
        .local_get(ADDRESS)
        .call(imports.read.expect("prepared return has a read import"))
        .local_set(HELPER_STATUS);
    validate_result(code, false);
    exit_if_failed(code, exit_depth);
    helper_field(code, 20);
    code.local_set(EIP)
        .local_get(esp)
        .i32_const(4 + i32::from(stack_adjust))
        .i32_add()
        .local_set(esp);
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
    address_value(code, address);
    code.local_set(ADDRESS);
}

pub(super) fn address_value(code: &mut InstructionSink<'_>, address: EffectiveAddress) {
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
