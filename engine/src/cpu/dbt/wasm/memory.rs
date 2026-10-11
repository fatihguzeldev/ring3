use wasm_encoder::{BlockType, InstructionSink, MemArg};

use crate::cpu::x86::ir::{
    BinaryKind, BranchTarget, EffectiveAddress, Location32, Operation, SmallSource, SmallWidth,
    UnaryKind, Value32,
};

use super::locals::*;

mod byte_exchange;
mod byte_store;
mod exchange;
mod narrow;
mod word_exchange;
mod word_store;

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
    pub(super) store16: Option<StoreImport>,
    pub(super) exchange: Option<StoreImport>,
    pub(super) exchange8: Option<StoreImport>,
    pub(super) exchange16: Option<StoreImport>,
    pub(super) compare_exchange32: Option<StoreImport>,
    pub(super) exchange_add32: Option<StoreImport>,
    pub(super) compare_exchange8: Option<StoreImport>,
    pub(super) exchange_add8: Option<StoreImport>,
    pub(super) compare_exchange16: Option<StoreImport>,
    pub(super) exchange_add16: Option<StoreImport>,
    pub(super) bit_test_set32: Option<StoreImport>,
    pub(super) bit_test_reset32: Option<StoreImport>,
    pub(super) atomic_or_u32: Option<StoreImport>,
    pub(super) atomic_and_u32: Option<StoreImport>,
    pub(super) atomic_xor_u32: Option<StoreImport>,
}

pub(super) struct ImportNeeds {
    pub(super) read: bool,
    pub(super) store: bool,
    pub(super) read8: bool,
    pub(super) read16: bool,
    pub(super) store8: bool,
    pub(super) store16: bool,
    pub(super) exchange: bool,
    pub(super) exchange8: bool,
    pub(super) exchange16: bool,
    pub(super) compare_exchange32: bool,
    pub(super) exchange_add32: bool,
    pub(super) compare_exchange8: bool,
    pub(super) exchange_add8: bool,
    pub(super) compare_exchange16: bool,
    pub(super) exchange_add16: bool,
    pub(super) bit_test_set32: bool,
    pub(super) bit_test_reset32: bool,
    pub(super) atomic_or_u32: bool,
    pub(super) atomic_and_u32: bool,
    pub(super) atomic_xor_u32: bool,
}

impl Imports {
    pub(super) fn needed(blocks: &[crate::cpu::dbt::region::CompiledBlock]) -> ImportNeeds {
        let mut read = false;
        let mut store = false;
        let mut read8 = false;
        let mut read16 = false;
        let mut store8 = false;
        let mut store16 = false;
        let mut exchange = false;
        let mut exchange8 = false;
        let mut exchange16 = false;
        let mut compare_exchange32 = false;
        let mut exchange_add32 = false;
        let mut compare_exchange8 = false;
        let mut exchange_add8 = false;
        let mut compare_exchange16 = false;
        let mut exchange_add16 = false;
        let mut bit_test_set32 = false;
        let mut bit_test_reset32 = false;
        let mut atomic_or_u32 = false;
        let mut atomic_and_u32 = false;
        let mut atomic_xor_u32 = false;
        for instruction in blocks.iter().flat_map(|block| &block.instructions) {
            match instruction.operation() {
                Operation::MemoryExchange { .. } => exchange = true,
                Operation::MemoryExchangeByte { .. } => exchange8 = true,
                Operation::MemoryExchangeWord { .. } => exchange16 = true,
                Operation::LockedMemoryCompareExchange32 { .. } => compare_exchange32 = true,
                Operation::LockedMemoryExchangeAdd32 { .. } => exchange_add32 = true,
                Operation::LockedMemoryCompareExchangeByte { .. } => compare_exchange8 = true,
                Operation::LockedMemoryExchangeAddByte { .. } => exchange_add8 = true,
                Operation::LockedMemoryCompareExchangeWord { .. } => compare_exchange16 = true,
                Operation::LockedMemoryExchangeAddWord { .. } => exchange_add16 = true,
                Operation::LockedMemoryBitSet32 { .. } => bit_test_set32 = true,
                Operation::LockedMemoryBitReset32 { .. } => bit_test_reset32 = true,
                Operation::LockedMemoryOr32 { .. } | Operation::LockedMemoryOrRegister32 { .. } => {
                    atomic_or_u32 = true
                }
                Operation::LockedMemoryAnd32 { .. }
                | Operation::LockedMemoryAndRegister32 { .. } => atomic_and_u32 = true,
                Operation::LockedMemoryXor32 { .. }
                | Operation::LockedMemoryXorRegister32 { .. } => atomic_xor_u32 = true,
                Operation::X87StatusToMemory { .. }
                | Operation::X87ControlToMemory { .. }
                | Operation::StoreWord { .. }
                | Operation::PushWord { .. }
                | Operation::CallWord { .. }
                | Operation::CallRegisterWord { .. }
                | Operation::PushFlagsWord
                | Operation::EnterZeroWord
                | Operation::StoreStringWord => {
                    store16 = true;
                }
                Operation::LoadByte { .. }
                | Operation::ReadExtendByteToWord { .. }
                | Operation::LoadStringByte
                | Operation::TranslateByte
                | Operation::CompareStringByte
                | Operation::ScanStringByte
                | Operation::MemoryPredicateByte { .. }
                | Operation::ReadCompareByte { .. }
                | Operation::ReadLogicalByte { .. }
                | Operation::ReadByteMultiplyAccumulator { .. }
                | Operation::ReadByteDivideAccumulator { .. }
                | Operation::ReadArithmeticByte { .. } => read8 = true,
                Operation::StoreByte { .. }
                | Operation::MemorySetByte { .. }
                | Operation::StoreStringByte => {
                    store8 = true;
                }
                Operation::MoveStringByte
                | Operation::MemoryUnaryByte { .. }
                | Operation::MemoryLogicalByte { .. }
                | Operation::MemoryArithmeticByte { .. }
                | Operation::MemoryExchangeAddByte { .. }
                | Operation::MemoryCompareExchangeByte { .. }
                | Operation::MemoryShiftByte { .. }
                | Operation::MemoryShiftByteImmediate { .. }
                | Operation::MemoryShiftByteCl { .. }
                | Operation::MemoryByteRotateOne { .. }
                | Operation::MemoryByteRotate { .. }
                | Operation::MemoryByteRotateThroughCarryOne { .. }
                | Operation::MemoryByteRotateThroughCarryImmediate { .. }
                | Operation::MemoryByteRotateThroughCarryCl { .. } => {
                    read8 = true;
                    store8 = true;
                }
                Operation::MemoryCompareWord { .. }
                | Operation::ReadCompareWord { .. }
                | Operation::ReadArithmeticWord { .. }
                | Operation::ReadLogicalWord { .. }
                | Operation::MemoryTestWord { .. }
                | Operation::ReadConditionalMoveWord { .. }
                | Operation::ReadBitScanWord { .. }
                | Operation::ReadSignedMultiplyWord { .. }
                | Operation::ReadSignedMultiplyImmediateWord { .. }
                | Operation::ReadWordMultiplyAccumulator { .. }
                | Operation::ReadWordDivideAccumulator { .. }
                | Operation::ReadJumpWord { .. }
                | Operation::ReadBitTestWord { .. }
                | Operation::LoadWord { .. }
                | Operation::PopWord { .. }
                | Operation::ReturnWord { .. }
                | Operation::PopAllWord
                | Operation::LeaveWord
                | Operation::LoadStringWord
                | Operation::CompareStringWord
                | Operation::ScanStringWord => read16 = true,
                Operation::MoveStringWord
                | Operation::PushMemoryWord { .. }
                | Operation::ReadCallWord { .. }
                | Operation::PopMemoryWord { .. }
                | Operation::MemoryUnaryWord { .. }
                | Operation::MemoryShiftWord { .. }
                | Operation::MemoryDoubleShiftWord { .. }
                | Operation::MemoryRotateWord { .. }
                | Operation::MemoryRotateThroughCarryWord { .. }
                | Operation::MemoryExchangeAddWord { .. }
                | Operation::MemoryCompareExchangeWord { .. }
                | Operation::MemoryBitMutationWord { .. }
                | Operation::MemoryArithmeticWord { .. }
                | Operation::MemoryLogicalWord { .. } => {
                    read16 = true;
                    store16 = true;
                }
                Operation::Extend {
                    source: SmallSource::Memory { width, .. },
                    ..
                } => match width {
                    SmallWidth::Byte => read8 = true,
                    SmallWidth::Word => read16 = true,
                },
                Operation::MoveStringDword
                | Operation::MemoryBitMutation { .. }
                | Operation::Push {
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
                | Operation::MemoryRotateOne { .. }
                | Operation::MemoryRotate { .. }
                | Operation::MemoryDoubleShift { .. }
                | Operation::MemoryRotateThroughCarryOne { .. }
                | Operation::MemoryRotateThroughCarryImmediate { .. }
                | Operation::MemoryRotateThroughCarryCl { .. }
                | Operation::MemoryExchangeAdd { .. }
                | Operation::MemoryCompareExchange { .. }
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
                Operation::CompareStringDword
                | Operation::LoadStringDword
                | Operation::ScanStringDword
                | Operation::ReadConditionalMove { .. }
                | Operation::ReadMultiplyAccumulator { .. }
                | Operation::ReadDivideAccumulator { .. }
                | Operation::ReadBitScan { .. }
                | Operation::ReadBitTest { .. }
                | Operation::Move {
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
                | Operation::PopAll
                | Operation::Leave
                | Operation::Jump {
                    target: BranchTarget::Indirect(Location32::Memory(_)),
                }
                | Operation::Pop { .. } => read = true,
                Operation::StoreStringDword
                | Operation::Move {
                    destination: Location32::Memory(_),
                    ..
                }
                | Operation::Call { .. }
                | Operation::Push { .. }
                | Operation::EnterZero
                | Operation::PushFlags => store = true,
                _ => {}
            }
        }
        ImportNeeds {
            read,
            store,
            read8,
            read16,
            store8,
            store16,
            exchange,
            exchange8,
            exchange16,
            compare_exchange32,
            exchange_add32,
            compare_exchange8,
            exchange_add8,
            compare_exchange16,
            exchange_add16,
            bit_test_set32,
            bit_test_reset32,
            atomic_or_u32,
            atomic_and_u32,
            atomic_xor_u32,
        }
    }
}

pub(super) fn bit_test_set_result(
    code: &mut InstructionSink<'_>,
    address: EffectiveAddress,
    index: crate::cpu::x86::Register32,
    imports: Imports,
    exit_depth: u32,
) {
    exchange::bit_test_set_result(code, address, index, imports, exit_depth);
}

pub(super) fn bit_test_reset_result(
    code: &mut InstructionSink<'_>,
    address: EffectiveAddress,
    index: crate::cpu::x86::Register32,
    imports: Imports,
    exit_depth: u32,
) {
    exchange::bit_test_reset_result(code, address, index, imports, exit_depth);
}

pub(super) fn atomic_or_result(
    code: &mut InstructionSink<'_>,
    address: EffectiveAddress,
    mask: u32,
    imports: Imports,
    exit_depth: u32,
) {
    exchange::atomic_or_result(code, address, mask, imports, exit_depth);
}

pub(super) fn atomic_and_result(
    code: &mut InstructionSink<'_>,
    address: EffectiveAddress,
    mask: u32,
    imports: Imports,
    exit_depth: u32,
) {
    exchange::atomic_and_result(code, address, mask, imports, exit_depth);
}

pub(super) fn atomic_xor_result(
    code: &mut InstructionSink<'_>,
    address: EffectiveAddress,
    mask: u32,
    imports: Imports,
    exit_depth: u32,
) {
    exchange::atomic_xor_result(code, address, mask, imports, exit_depth);
}

pub(super) fn atomic_or_register_result(
    code: &mut InstructionSink<'_>,
    address: EffectiveAddress,
    source: crate::cpu::x86::Register32,
    imports: Imports,
    exit_depth: u32,
) {
    exchange::atomic_or_register_result(code, address, source, imports, exit_depth);
}

pub(super) fn atomic_and_register_result(
    code: &mut InstructionSink<'_>,
    address: EffectiveAddress,
    source: crate::cpu::x86::Register32,
    imports: Imports,
    exit_depth: u32,
) {
    exchange::atomic_and_register_result(code, address, source, imports, exit_depth);
}

pub(super) fn atomic_xor_register_result(
    code: &mut InstructionSink<'_>,
    address: EffectiveAddress,
    source: crate::cpu::x86::Register32,
    imports: Imports,
    exit_depth: u32,
) {
    exchange::atomic_xor_register_result(code, address, source, imports, exit_depth);
}

pub(super) fn exchange_add_result(
    code: &mut InstructionSink<'_>,
    address: EffectiveAddress,
    source: crate::cpu::x86::Register32,
    imports: Imports,
    exit_depth: u32,
) {
    exchange::exchange_add_result(code, address, source, imports, exit_depth);
}

pub(super) fn exchange_result(
    code: &mut InstructionSink<'_>,
    address: EffectiveAddress,
    source: crate::cpu::x86::Register32,
    imports: Imports,
    exit_depth: u32,
) {
    exchange::exchange_result(code, address, source, imports, exit_depth);
}

pub(super) fn compare_exchange_result(
    code: &mut InstructionSink<'_>,
    address: EffectiveAddress,
    source: crate::cpu::x86::Register32,
    imports: Imports,
    exit_depth: u32,
) {
    exchange::compare_exchange_result(code, address, source, imports, exit_depth);
}

pub(super) fn exchange_byte_result(
    code: &mut InstructionSink<'_>,
    address: EffectiveAddress,
    imports: Imports,
    exit_depth: u32,
) {
    byte_exchange::exchange_result(code, address, imports, exit_depth);
}

pub(super) fn exchange_add_byte_result(
    code: &mut InstructionSink<'_>,
    address: EffectiveAddress,
    imports: Imports,
    exit_depth: u32,
) {
    byte_exchange::exchange_add_result(code, address, imports, exit_depth);
}

pub(super) fn compare_exchange_byte_result(
    code: &mut InstructionSink<'_>,
    address: EffectiveAddress,
    imports: Imports,
    exit_depth: u32,
) {
    byte_exchange::compare_exchange_result(code, address, imports, exit_depth);
}

pub(super) fn exchange_word_result(
    code: &mut InstructionSink<'_>,
    address: EffectiveAddress,
    imports: Imports,
    exit_depth: u32,
) {
    word_exchange::exchange_result(code, address, imports, exit_depth);
}

pub(super) fn exchange_add_word_result(
    code: &mut InstructionSink<'_>,
    address: EffectiveAddress,
    imports: Imports,
    exit_depth: u32,
) {
    word_exchange::exchange_add_result(code, address, imports, exit_depth);
}

pub(super) fn compare_exchange_word_result(
    code: &mut InstructionSink<'_>,
    address: EffectiveAddress,
    imports: Imports,
    exit_depth: u32,
) {
    word_exchange::compare_exchange_result(code, address, imports, exit_depth);
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

pub(super) fn store_word_result(
    code: &mut InstructionSink<'_>,
    address: EffectiveAddress,
    imports: Imports,
    exit_depth: u32,
) {
    word_store::store_result(code, address, imports, exit_depth);
}

pub(super) fn store_word_result_at_address(
    code: &mut InstructionSink<'_>,
    imports: Imports,
    exit_depth: u32,
) {
    word_store::store_result_at_address(code, imports, exit_depth);
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

pub(super) fn load_narrow_at_address(
    code: &mut InstructionSink<'_>,
    width: SmallWidth,
    imports: Imports,
    exit_depth: u32,
) {
    narrow::load_at_address(code, width, imports, exit_depth);
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

pub(super) fn push_word_result(code: &mut InstructionSink<'_>, imports: Imports, exit_depth: u32) {
    let esp = crate::cpu::x86::Register32::Esp;
    store_word_result(
        code,
        EffectiveAddress {
            base: Some(esp),
            index: None,
            scale: 1,
            displacement: (-2_i32) as u32,
        },
        imports,
        exit_depth,
    );
    code.local_get(ADDRESS).local_set(register(esp));
}

pub(super) fn pop_word_register(
    code: &mut InstructionSink<'_>,
    destination: crate::cpu::x86::Register32,
    imports: Imports,
    exit_depth: u32,
) {
    let esp = crate::cpu::x86::Register32::Esp;
    load_narrow_value(
        code,
        EffectiveAddress {
            base: Some(esp),
            index: None,
            scale: 1,
            displacement: 0,
        },
        SmallWidth::Word,
        imports,
        exit_depth,
    );
    code.local_get(register(destination));
    if destination == esp {
        code.i32_const(2).i32_add();
    }
    code.i32_const(!0xffff)
        .i32_and()
        .i32_or()
        .local_set(register(destination));
    if destination != esp {
        code.local_get(register(esp))
            .i32_const(2)
            .i32_add()
            .local_set(register(esp));
    }
}

pub(super) fn pop_word_memory(
    code: &mut InstructionSink<'_>,
    mut destination: EffectiveAddress,
    imports: Imports,
    exit_depth: u32,
) {
    let esp = crate::cpu::x86::Register32::Esp;
    load_narrow_value(
        code,
        EffectiveAddress {
            base: Some(esp),
            index: None,
            scale: 1,
            displacement: 0,
        },
        SmallWidth::Word,
        imports,
        exit_depth,
    );
    code.local_set(RESULT);
    if destination.base == Some(esp) {
        destination.displacement = destination.displacement.wrapping_add(2);
    }
    store_word_result(code, destination, imports, exit_depth);
    code.local_get(register(esp))
        .i32_const(2)
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
    load_result_at_address(code, imports, exit_depth);
}

pub(super) fn load_result_at_address(
    code: &mut InstructionSink<'_>,
    imports: Imports,
    exit_depth: u32,
) {
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
    store_result_at_address(code, imports, exit_depth);
}

pub(super) fn store_result_at_address(
    code: &mut InstructionSink<'_>,
    imports: Imports,
    exit_depth: u32,
) {
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

pub(super) fn pop_all(
    code: &mut InstructionSink<'_>,
    word: bool,
    imports: Imports,
    exit_depth: u32,
) {
    use crate::cpu::x86::Register32::{Eax, Ebp, Ebx, Ecx, Edi, Edx, Esi, Esp};

    let width = if word { 2 } else { 4 };
    let registers = [
        (0, Edi),
        (1, Esi),
        (2, Ebp),
        (4, Ebx),
        (5, Edx),
        (6, Ecx),
        (7, Eax),
    ];
    // retain all original parents until every checked read succeeds.
    for &(slot, destination) in &registers {
        let source = EffectiveAddress {
            base: Some(Esp),
            index: None,
            scale: 1,
            displacement: slot * width,
        };
        if word {
            load_narrow_value(code, source, SmallWidth::Word, imports, exit_depth);
            code.local_get(register(destination))
                .i32_const(!0xffff)
                .i32_and()
                .i32_or();
        } else {
            load_result(code, source, imports, exit_depth);
            code.local_get(RESULT);
        }
    }
    for &(_, destination) in registers.iter().rev() {
        code.local_set(register(destination));
    }
    code.local_get(register(Esp))
        .i32_const((width * 8) as i32)
        .i32_add()
        .local_set(register(Esp));
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
