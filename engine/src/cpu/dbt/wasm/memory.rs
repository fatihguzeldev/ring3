use wasm_encoder::{BlockType, InstructionSink, MemArg};

use crate::cpu::x86::ir::{
    BinaryKind, BranchTarget, EffectiveAddress, Location32, Operation, SmallSource, SmallWidth,
    UnaryKind, Value32,
};
use crate::cpu::x86::{SegmentAccess, SegmentRegister};

use super::locals::*;

mod byte_exchange;
mod byte_store;
mod cpu_scalar;
mod intent;
pub(super) use intent::read_modify_needs;
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
    pub(super) fn emit_binding(self, code: &mut InstructionSink<'_>) -> u32 {
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
    pub(super) paging_configured: bool,
    pub(super) fetch_fault: Option<StoreImport>,
    pub(super) detection: Option<super::detection::DetectionLocals>,
    pub(super) finish_detection: Option<u32>,
    pub(super) segment_check: Option<u32>,
    pub(super) code_fetch_limit: Option<u32>,
    pub(super) stack_execute: Option<StoreImport>,
    pub(super) cpu_integer: Option<StoreImport>,
    pub(super) cpu_read: Option<u32>,
    pub(super) cpu_write: Option<StoreImport>,
    pub(super) x87_op: Option<StoreImport>,
    pub(super) simd_execute: Option<StoreImport>,
    pub(super) avx_execute: Option<StoreImport>,
    pub(super) avx_locals: Option<super::avx::AvxLocals>,
    pub(super) cpu_fxsave: Option<StoreImport>,
    pub(super) cpu_xsave: Option<StoreImport>,
    pub(super) cpu_xrstor: Option<StoreImport>,
    pub(super) cpu_mask_write: Option<StoreImport>,
    pub(super) segment_load: Option<StoreImport>,
    pub(super) segment_load_far: Option<StoreImport>,
    pub(super) far_jump: Option<StoreImport>,
    pub(super) far_call: Option<StoreImport>,
    pub(super) far_return: Option<StoreImport>,
    pub(super) interrupt_return_locals: Option<super::far::InterruptReturnLocals>,
    pub(super) read: Option<u32>,
    pub(super) store: Option<StoreImport>,
    pub(super) read8: Option<u32>,
    pub(super) read16: Option<u32>,
    pub(super) read_modify: Option<u32>,
    pub(super) read_modify8: Option<u32>,
    pub(super) read_modify16: Option<u32>,
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
    pub(super) stack_execute: bool,
    pub(super) cpu_integer: bool,
    pub(super) cpu_read: bool,
    pub(super) cpu_write: bool,
    pub(super) x87_op: bool,
    pub(super) simd_execute: bool,
    pub(super) avx_execute: bool,
    pub(super) cpu_fxsave: bool,
    pub(super) cpu_xsave: bool,
    pub(super) cpu_xrstor: bool,
    pub(super) cpu_mask_write: bool,
    pub(super) segment_load: bool,
    pub(super) segment_load_far: bool,
    pub(super) far_jump: bool,
    pub(super) far_call: bool,
    pub(super) far_return: bool,
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
        let mut stack_execute = false;
        let mut cpu_integer = false;
        let mut cpu_read = false;
        let mut cpu_write = false;
        let mut x87_op = false;
        let mut simd_execute = false;
        let mut avx_execute = false;
        let mut cpu_fxsave = false;
        let mut cpu_xsave = false;
        let mut cpu_xrstor = false;
        let mut cpu_mask_write = false;
        let mut segment_load = false;
        let mut segment_load_far = false;
        let mut far_jump = false;
        let mut far_call = false;
        let mut far_return = false;
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
                Operation::Avx(operation) => {
                    if super::avx::direct_hotpath(operation.instruction()) {
                        if let crate::cpu::x86::avx::MemoryEffect::Full(requirement) =
                            operation.instruction().memory()
                        {
                            cpu_read |= requirement.access == crate::memory::Access::Read;
                            cpu_write |= requirement.access == crate::memory::Access::Write;
                        }
                    } else {
                        avx_execute = true;
                    }
                }
                Operation::FarJump { target, .. } => {
                    far_jump = true;
                    cpu_read |= matches!(target, crate::cpu::x86::ir::FarPointer::Memory(_));
                }
                Operation::FarCall { target, .. } => {
                    far_call = true;
                    cpu_read |= matches!(target, crate::cpu::x86::ir::FarPointer::Memory(_));
                }
                Operation::FarReturn { .. } | Operation::InterruptReturn { .. } => {
                    far_return = true;
                    cpu_read = true;
                }
                Operation::XState(operation) => match operation {
                    crate::cpu::x86::xstate::XStateOperation::Save { .. } => cpu_xsave = true,
                    crate::cpu::x86::xstate::XStateOperation::Restore { .. } => cpu_xrstor = true,
                    crate::cpu::x86::xstate::XStateOperation::ReadControl => {}
                },
                Operation::AdjustPrivilege {
                    destination: Location32::Memory(_),
                    ..
                } => cpu_integer = true,
                Operation::VerifySegment { source, .. }
                | Operation::QuerySegment { source, .. } => {
                    cpu_integer = true;
                    read16 |= matches!(source, Value32::Memory(_));
                }
                Operation::StoreSystemDescriptor { .. } => cpu_write = true,
                Operation::CacheFlush { .. } => read8 = true,
                Operation::VirtualTimestampAux => cpu_integer = true,
                Operation::PackedStringCompare(operation) => {
                    cpu_read |= matches!(
                        operation.right,
                        crate::cpu::x86::packed_strings::PackedStringSource::Memory(_)
                    );
                }
                Operation::Crc32 {
                    source: crate::cpu::x86::bits::Crc32Source::Memory { width, .. },
                    ..
                } => match width {
                    crate::cpu::x86::bits::Crc32Width::Byte => read8 = true,
                    crate::cpu::x86::bits::Crc32Width::Word => read16 = true,
                    crate::cpu::x86::bits::Crc32Width::Dword => read = true,
                },
                Operation::BitManipulation { source, .. }
                | Operation::MultiplyExtended { source, .. } => {
                    read |= matches!(source, Value32::Memory(_));
                }
                Operation::Control(operation) => match operation {
                    crate::cpu::x86::ControlOperation::PopFlags { word: true } => read16 = true,
                    crate::cpu::x86::ControlOperation::PopFlags { word: false } => read = true,
                    crate::cpu::x86::ControlOperation::Bound { .. } => cpu_read = true,
                    _ => {}
                },
                Operation::BitCount { width, source, .. } => {
                    if matches!(source, Value32::Memory(_)) {
                        match width {
                            crate::cpu::x86::bits::ScalarWidth::Word => read16 = true,
                            crate::cpu::x86::bits::ScalarWidth::Dword => read = true,
                        }
                    }
                }
                Operation::MoveByteSwap {
                    width,
                    destination,
                    source,
                } => {
                    let reads = matches!(source, Value32::Memory(_));
                    let writes = matches!(destination, Location32::Memory(_));
                    match width {
                        crate::cpu::x86::bits::ScalarWidth::Word => {
                            read16 |= reads;
                            store16 |= writes;
                        }
                        crate::cpu::x86::bits::ScalarWidth::Dword => {
                            read |= reads;
                            store |= writes;
                        }
                    }
                }
                Operation::StackFrame { .. }
                | Operation::PopAll
                | Operation::PopAllWord
                | Operation::EnterZero
                | Operation::EnterZeroWord => stack_execute = true,
                Operation::AtomicMemory { .. }
                | Operation::CompareExchange8B { .. }
                | Operation::VirtualTimestamp => cpu_integer = true,
                Operation::X87 { .. } => x87_op = true,
                Operation::Simd(operation) => {
                    use crate::cpu::x86::simd::SimdKind;
                    if !matches!(operation.kind, SimdKind::Fence | SimdKind::Prefetch)
                        && !super::simd::direct_hotpath(*operation)
                    {
                        simd_execute = true;
                    }
                    cpu_read |= operation.reads_memory();
                    if operation.writes_memory() {
                        match operation.kind {
                            SimdKind::SaveExtended => cpu_fxsave = true,
                            SimdKind::MaskedStore => cpu_mask_write = true,
                            _ => cpu_write = true,
                        }
                    }
                }
                Operation::LoadSegment { source, .. } => {
                    segment_load = true;
                    read16 |= matches!(source, Value32::Memory(_));
                }
                Operation::PopSegment { word, .. } => {
                    segment_load = true;
                    if *word {
                        read16 = true;
                    } else {
                        read = true;
                    }
                }
                Operation::LoadFarPointer { .. } => segment_load_far = true,
                Operation::String { kind, width, .. }
                | Operation::RepeatString { kind, width, .. } => {
                    use crate::cpu::x86::ir::{StringKind, StringWidth};
                    let reads = !matches!(kind, StringKind::Store);
                    let writes = matches!(kind, StringKind::Move | StringKind::Store);
                    match width {
                        StringWidth::Byte => {
                            read8 |= reads;
                            store8 |= writes;
                        }
                        StringWidth::Word => {
                            read16 |= reads;
                            store16 |= writes;
                        }
                        StringWidth::Dword => {
                            read |= reads;
                            store |= writes;
                        }
                    }
                }
                Operation::ReadSegment {
                    destination: Location32::Memory(_),
                    ..
                }
                | Operation::PushSegment { word: true, .. } => store16 = true,
                Operation::PushSegment { word: false, .. } => store = true,
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
                | Operation::PushFlagsWord => {
                    store16 = true;
                }
                Operation::LoadByte { .. }
                | Operation::ReadExtendByteToWord { .. }
                | Operation::TranslateByteAddress { .. }
                | Operation::TranslateByte
                | Operation::MemoryPredicateByte { .. }
                | Operation::ReadCompareByte { .. }
                | Operation::ReadLogicalByte { .. }
                | Operation::ReadByteMultiplyAccumulator { .. }
                | Operation::ReadByteDivideAccumulator { .. }
                | Operation::ReadArithmeticByte { .. } => read8 = true,
                Operation::StoreByte { .. } | Operation::MemorySetByte { .. } => {
                    store8 = true;
                }
                Operation::MemoryUnaryByte { .. }
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
                | Operation::LeaveWord => read16 = true,
                Operation::PushMemoryWord { .. }
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
                Operation::MemoryBitMutation { .. }
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
                Operation::ReadConditionalMove { .. }
                | Operation::ReadMultiplyAccumulator { .. }
                | Operation::ReadDivideAccumulator { .. }
                | Operation::ReadBitScan { .. }
                | Operation::ReadTrailingZeroCount32 { .. }
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
                | Operation::Leave
                | Operation::Jump {
                    target: BranchTarget::Indirect(Location32::Memory(_)),
                }
                | Operation::Pop { .. } => read = true,
                Operation::Move {
                    destination: Location32::Memory(_),
                    ..
                }
                | Operation::Call { .. }
                | Operation::Push { .. }
                | Operation::PushFlags => store = true,
                _ => {}
            }
        }
        ImportNeeds {
            stack_execute,
            cpu_integer,
            cpu_read,
            cpu_write,
            x87_op,
            simd_execute,
            avx_execute,
            cpu_fxsave,
            cpu_xsave,
            cpu_xrstor,
            cpu_mask_write,
            segment_load,
            segment_load_far,
            far_jump,
            far_call,
            far_return,
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

pub(super) fn store_byte_result_at_address(
    code: &mut InstructionSink<'_>,
    imports: Imports,
    exit_depth: u32,
) {
    byte_store::store_result_at_address(code, imports, exit_depth);
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
    checked_address(code, address, 4, SegmentAccess::Read, imports, exit_depth);
    code.local_get(ADDRESS)
        .call(imports.read.expect("prepared memory load has an import"))
        .local_set(HELPER_STATUS);
    super::code_fetch::check_context(code, exit_depth);
    cpu_scalar::validate(
        code,
        4,
        crate::abi::memory_helper::CpuScalarOperation::Read,
        imports.paging_configured,
    );
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
    checked_address(code, address, 4, SegmentAccess::Write, imports, exit_depth);
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
    cpu_scalar::validate(
        code,
        4,
        crate::abi::memory_helper::CpuScalarOperation::Store,
        imports.paging_configured,
    );
    exit_if_failed(code, exit_depth);
}

pub(super) fn push_memory(
    code: &mut InstructionSink<'_>,
    source: EffectiveAddress,
    imports: Imports,
    exit_depth: u32,
) {
    checked_address(code, source, 4, SegmentAccess::Read, imports, exit_depth);
    super::stack::pointer::check(
        code,
        crate::cpu::x86::Register32::Esp,
        -4,
        4,
        SegmentAccess::Write,
        imports,
        exit_depth,
    );
    load_result(code, source, imports, exit_depth);
    super::stack::pointer::push_result(code, 4, imports, exit_depth);
}

pub(super) fn pop_memory(
    code: &mut InstructionSink<'_>,
    destination: EffectiveAddress,
    imports: Imports,
    exit_depth: u32,
) {
    pop_memory_width(code, destination, 4, imports, exit_depth);
}

fn checked_pop_destination(
    code: &mut InstructionSink<'_>,
    destination: EffectiveAddress,
    width: u32,
    imports: Imports,
    exit_depth: u32,
) {
    if destination.base == Some(crate::cpu::x86::Register32::Esp) {
        // wrapped B16 POP follows the declared Ring3 recomposed-ESP destination policy.
        code.i32_const(destination.displacement as i32);
        super::stack::pointer::assigned(
            code,
            register(crate::cpu::x86::Register32::Esp),
            width as i32,
        );
        code.i32_add();
        if let Some(index) = destination.index {
            code.local_get(register(index))
                .i32_const(i32::from(destination.scale))
                .i32_mul()
                .i32_add();
        }
        if destination.address_size == crate::cpu::x86::ir::AddressSize::Bits16 {
            code.i32_const(0xffff).i32_and();
        }
        checked_offset(
            code,
            destination.resolved_segment(),
            width,
            SegmentAccess::Write,
            imports,
            exit_depth,
        );
    } else {
        checked_address(
            code,
            destination,
            width,
            SegmentAccess::Write,
            imports,
            exit_depth,
        );
    }
}

fn pop_memory_width(
    code: &mut InstructionSink<'_>,
    destination: EffectiveAddress,
    width: u32,
    imports: Imports,
    exit_depth: u32,
) {
    super::stack::pointer::check(
        code,
        crate::cpu::x86::Register32::Esp,
        0,
        width,
        SegmentAccess::Read,
        imports,
        exit_depth,
    );
    checked_pop_destination(code, destination, width, imports, exit_depth);
    super::stack::pointer::load_result(code, width, imports, exit_depth);
    checked_pop_destination(code, destination, width, imports, exit_depth);
    if width == 2 {
        store_word_result_at_address(code, imports, exit_depth);
    } else {
        store_result_at_address(code, imports, exit_depth);
    }
    super::stack::pointer::advance(code, width as i32);
}

pub(super) fn push_word_result(code: &mut InstructionSink<'_>, imports: Imports, exit_depth: u32) {
    super::stack::pointer::push_result(code, 2, imports, exit_depth);
}

pub(super) fn pop_word_register(
    code: &mut InstructionSink<'_>,
    destination: crate::cpu::x86::Register32,
    imports: Imports,
    exit_depth: u32,
) {
    let esp = crate::cpu::x86::Register32::Esp;
    super::stack::pointer::load(code, esp, 0, 2, imports, exit_depth);
    if destination == esp {
        super::stack::pointer::assigned(code, register(esp), 2);
    } else {
        code.local_get(register(destination));
    }
    code.i32_const(!0xffff)
        .i32_and()
        .i32_or()
        .local_set(register(destination));
    if destination != esp {
        super::stack::pointer::advance(code, 2);
    }
}

pub(super) fn pop_word_memory(
    code: &mut InstructionSink<'_>,
    destination: EffectiveAddress,
    imports: Imports,
    exit_depth: u32,
) {
    pop_memory_width(code, destination, 2, imports, exit_depth);
}

pub(super) fn load_result(
    code: &mut InstructionSink<'_>,
    source: EffectiveAddress,
    imports: Imports,
    exit_depth: u32,
) {
    checked_address(code, source, 4, SegmentAccess::Read, imports, exit_depth);
    load_result_at_address(code, imports, exit_depth);
}

pub(super) fn load_modify_result(
    code: &mut InstructionSink<'_>,
    source: EffectiveAddress,
    imports: Imports,
    exit_depth: u32,
) {
    checked_address(code, source, 4, SegmentAccess::Write, imports, exit_depth);
    load_modify_at_address(code, imports, exit_depth);
}

pub(super) fn load_narrow_for_modify(
    code: &mut InstructionSink<'_>,
    source: EffectiveAddress,
    width: SmallWidth,
    imports: Imports,
    exit_depth: u32,
) {
    let length = match width {
        SmallWidth::Byte => 1,
        SmallWidth::Word => 2,
    };
    checked_address(
        code,
        source,
        length,
        SegmentAccess::Write,
        imports,
        exit_depth,
    );
    load_narrow_modify_at_address(code, width, imports, exit_depth);
}

pub(super) fn load_narrow_modify_at_address(
    code: &mut InstructionSink<'_>,
    width: SmallWidth,
    imports: Imports,
    exit_depth: u32,
) {
    narrow::load_at_address_with_intent(code, width, imports, exit_depth, true);
}

pub(super) fn load_modify_at_address(
    code: &mut InstructionSink<'_>,
    imports: Imports,
    exit_depth: u32,
) {
    load_at_address_with_intent(code, imports, exit_depth, true);
}

pub(super) fn load_result_at_address(
    code: &mut InstructionSink<'_>,
    imports: Imports,
    exit_depth: u32,
) {
    load_at_address_with_intent(code, imports, exit_depth, false);
}

fn load_at_address_with_intent(
    code: &mut InstructionSink<'_>,
    imports: Imports,
    exit_depth: u32,
    modify: bool,
) {
    let paging_modify = modify && imports.paging_configured;
    code.local_get(ADDRESS)
        .call(
            (if paging_modify {
                imports.read_modify
            } else {
                imports.read
            })
            .expect("prepared memory operand has a read import"),
        )
        .local_set(HELPER_STATUS);
    super::code_fetch::check_context(code, exit_depth);
    cpu_scalar::validate(
        code,
        4,
        if paging_modify {
            crate::abi::memory_helper::CpuScalarOperation::Modify
        } else {
            crate::abi::memory_helper::CpuScalarOperation::Read
        },
        imports.paging_configured,
    );
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
    checked_address(
        code,
        destination,
        4,
        SegmentAccess::Write,
        imports,
        exit_depth,
    );
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
    cpu_scalar::validate(
        code,
        4,
        crate::abi::memory_helper::CpuScalarOperation::Store,
        imports.paging_configured,
    );
    exit_if_failed(code, exit_depth);
}

pub(super) fn pop_register(
    code: &mut InstructionSink<'_>,
    destination: crate::cpu::x86::Register32,
    imports: Imports,
    exit_depth: u32,
) {
    super::stack::pointer::load_result(code, 4, imports, exit_depth);
    code.local_get(RESULT).local_set(register(destination));
    if destination != crate::cpu::x86::Register32::Esp {
        super::stack::pointer::advance(code, 4);
    }
}

pub(super) fn pop_return(
    code: &mut InstructionSink<'_>,
    stack_adjust: u16,
    imports: Imports,
    exit_depth: u32,
) {
    super::stack::pointer::load_result(code, 4, imports, exit_depth);
    if let Some(limit) = imports.code_fetch_limit {
        super::code_fetch::check_value(code, RESULT, limit, exit_depth);
    }
    code.local_get(RESULT).local_set(EIP);
    super::stack::pointer::advance(code, 4 + i32::from(stack_adjust));
}

pub(super) fn exit_if_invalidated(
    code: &mut InstructionSink<'_>,
    exit_depth: u32,
    imports: Imports,
) {
    super::abi::debug_after_retirement(code, exit_depth, imports.detection, false);
    code.local_get(HELPER_STATUS)
        .i32_const(11)
        .i32_eq()
        .if_(BlockType::Empty)
        .i32_const(6)
        .local_set(REASON)
        .br(exit_depth + 1)
        .end();
}

pub(super) fn effective_address(code: &mut InstructionSink<'_>, address: EffectiveAddress) {
    address_value(code, address);
    code.local_set(ADDRESS);
}

pub(super) fn checked_address(
    code: &mut InstructionSink<'_>,
    address: EffectiveAddress,
    width: u32,
    access: SegmentAccess,
    imports: Imports,
    exit_depth: u32,
) {
    offset_value(code, address);
    checked_offset(
        code,
        address.resolved_segment(),
        width,
        access,
        imports,
        exit_depth,
    );
}

/// consumes a logical offset, including any instruction-specific bit-index displacement.
pub(super) fn checked_offset(
    code: &mut InstructionSink<'_>,
    segment: SegmentRegister,
    width: u32,
    access: SegmentAccess,
    imports: Imports,
    exit_depth: u32,
) {
    code.local_set(ADDRESS);
    super::segments::check_span(code, segment, width, access, imports, exit_depth);
    code.local_get(ADDRESS);
    super::segments::base(code, segment);
    code.i32_add().local_set(ADDRESS);
}

pub(super) fn address_value(code: &mut InstructionSink<'_>, address: EffectiveAddress) {
    offset_value(code, address);
    super::segments::base(code, address.resolved_segment());
    code.i32_add();
}

pub(super) fn offset_value(code: &mut InstructionSink<'_>, address: EffectiveAddress) {
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
    if address.address_size == crate::cpu::x86::ir::AddressSize::Bits16 {
        code.i32_const(0xffff).i32_and();
    }
}

pub(super) fn infrastructure(code: &mut InstructionSink<'_>, detail: i32) {
    code.i32_const(7)
        .local_set(REASON)
        .i32_const(detail)
        .local_set(DETAIL);
}

pub(super) fn protocol_if_bad(code: &mut InstructionSink<'_>) {
    code.local_get(LHS).if_(BlockType::Empty);
    infrastructure(code, 2);
    code.end();
}

pub(super) fn exit_if_failed(code: &mut InstructionSink<'_>, exit_depth: u32) {
    super::code_fetch::check_rejected(code, exit_depth);
    code.local_get(REASON)
        .if_(BlockType::Empty)
        .br(exit_depth + 1)
        .end();
}

pub(super) fn reset_checks(code: &mut InstructionSink<'_>) {
    code.i32_const(0).local_set(LHS);
}

pub(super) fn expect_field(code: &mut InstructionSink<'_>, offset: u64, expected: u32) {
    helper_field(code, offset);
    code.i32_const(expected as i32).i32_ne();
    add_check(code);
}

pub(super) fn add_check(code: &mut InstructionSink<'_>) {
    code.local_get(LHS).i32_or().local_set(LHS);
}

pub(super) fn helper_field(code: &mut InstructionSink<'_>, offset: u64) {
    code.local_get(STATE_PTR).i32_load(MemArg {
        offset: 100 + offset,
        align: 2,
        memory_index: 0,
    });
}
