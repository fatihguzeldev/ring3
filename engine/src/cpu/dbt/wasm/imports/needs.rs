use super::Imports;
use crate::cpu::{
    dbt::wasm::instructions::{avx, simd},
    x86::ir::{
        BinaryKind, BranchTarget, Location32, Operation, SmallSource, SmallWidth, UnaryKind,
        Value32,
    },
};
#[derive(Clone, Copy)]
pub(in crate::cpu::dbt::wasm) struct ImportNeeds {
    pub(in crate::cpu::dbt::wasm) stack_execute: bool,
    pub(in crate::cpu::dbt::wasm) cpu_integer: bool,
    pub(in crate::cpu::dbt::wasm) cpu_read: bool,
    pub(in crate::cpu::dbt::wasm) cpu_write: bool,
    pub(in crate::cpu::dbt::wasm) x87_op: bool,
    pub(in crate::cpu::dbt::wasm) simd_execute: bool,
    pub(in crate::cpu::dbt::wasm) avx_execute: bool,
    pub(in crate::cpu::dbt::wasm) cpu_fxsave: bool,
    pub(in crate::cpu::dbt::wasm) cpu_xsave: bool,
    pub(in crate::cpu::dbt::wasm) cpu_xrstor: bool,
    pub(in crate::cpu::dbt::wasm) cpu_mask_write: bool,
    pub(in crate::cpu::dbt::wasm) segment_load: bool,
    pub(in crate::cpu::dbt::wasm) segment_load_far: bool,
    pub(in crate::cpu::dbt::wasm) far_jump: bool,
    pub(in crate::cpu::dbt::wasm) far_call: bool,
    pub(in crate::cpu::dbt::wasm) far_return: bool,
    pub(in crate::cpu::dbt::wasm) read: bool,
    pub(in crate::cpu::dbt::wasm) read32_result_sites: usize,
    pub(in crate::cpu::dbt::wasm) store: bool,
    pub(in crate::cpu::dbt::wasm) read8: bool,
    pub(in crate::cpu::dbt::wasm) read16: bool,
    pub(in crate::cpu::dbt::wasm) store8: bool,
    pub(in crate::cpu::dbt::wasm) store16: bool,
    pub(in crate::cpu::dbt::wasm) exchange: bool,
    pub(in crate::cpu::dbt::wasm) exchange8: bool,
    pub(in crate::cpu::dbt::wasm) exchange16: bool,
    pub(in crate::cpu::dbt::wasm) compare_exchange32: bool,
    pub(in crate::cpu::dbt::wasm) exchange_add32: bool,
    pub(in crate::cpu::dbt::wasm) compare_exchange8: bool,
    pub(in crate::cpu::dbt::wasm) exchange_add8: bool,
    pub(in crate::cpu::dbt::wasm) compare_exchange16: bool,
    pub(in crate::cpu::dbt::wasm) exchange_add16: bool,
    pub(in crate::cpu::dbt::wasm) bit_test_set32: bool,
    pub(in crate::cpu::dbt::wasm) bit_test_reset32: bool,
    pub(in crate::cpu::dbt::wasm) atomic_or_u32: bool,
    pub(in crate::cpu::dbt::wasm) atomic_and_u32: bool,
    pub(in crate::cpu::dbt::wasm) atomic_xor_u32: bool,
}

impl Imports {
    pub(in crate::cpu::dbt::wasm) fn needed(
        blocks: &[crate::cpu::dbt::region::CompiledBlock],
    ) -> ImportNeeds {
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
        let mut read32_result_sites = 0;
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
                    if avx::direct_hotpath(operation.instruction()) {
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
                    crate::cpu::x86::bits::Crc32Width::Dword => {
                        read = true;
                        read32_result_sites += 1;
                    }
                },
                Operation::BitManipulation { source, .. }
                | Operation::MultiplyExtended { source, .. } => {
                    read |= matches!(source, Value32::Memory(_));
                    read32_result_sites += usize::from(matches!(source, Value32::Memory(_)));
                }
                Operation::Control(operation) => match operation {
                    crate::cpu::x86::ControlOperation::PopFlags { word: true } => read16 = true,
                    crate::cpu::x86::ControlOperation::PopFlags { word: false } => {
                        read = true;
                        read32_result_sites += 1;
                    }
                    crate::cpu::x86::ControlOperation::Bound { .. } => cpu_read = true,
                    _ => {}
                },
                Operation::BitCount { width, source, .. } => {
                    if matches!(source, Value32::Memory(_)) {
                        match width {
                            crate::cpu::x86::bits::ScalarWidth::Word => read16 = true,
                            crate::cpu::x86::bits::ScalarWidth::Dword => {
                                read = true;
                                read32_result_sites += 1;
                            }
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
                            read32_result_sites += usize::from(reads);
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
                        && !simd::direct_hotpath(*operation)
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
                        read32_result_sites += 1;
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
                            read32_result_sites += usize::from(reads)
                                + usize::from(matches!(kind, StringKind::Compare));
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
                Operation::Push {
                    source: Value32::Memory(_),
                }
                | Operation::Pop {
                    destination: Location32::Memory(_),
                }
                | Operation::Call {
                    target: BranchTarget::Indirect(Location32::Memory(_)),
                } => {
                    read = true;
                    read32_result_sites += 1;
                    store = true;
                }
                Operation::MemoryBitMutation { .. }
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
                | Operation::Pop { .. } => {
                    read = true;
                    read32_result_sites += 1;
                }
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
            read32_result_sites,
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

pub(in crate::cpu::dbt::wasm) fn read_modify_needs(
    blocks: &[crate::cpu::dbt::region::CompiledBlock],
) -> [bool; 3] {
    let mut widths = [false; 3];
    for instruction in blocks.iter().flat_map(|block| &block.instructions) {
        let index = match instruction.operation() {
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
            | Operation::MemoryByteRotateThroughCarryCl { .. } => 0,
            Operation::MemoryUnaryWord { .. }
            | Operation::MemoryShiftWord { .. }
            | Operation::MemoryDoubleShiftWord { .. }
            | Operation::MemoryRotateWord { .. }
            | Operation::MemoryRotateThroughCarryWord { .. }
            | Operation::MemoryExchangeAddWord { .. }
            | Operation::MemoryCompareExchangeWord { .. }
            | Operation::MemoryBitMutationWord { .. }
            | Operation::MemoryArithmeticWord { .. }
            | Operation::MemoryLogicalWord { .. } => 1,
            Operation::MemoryBitMutation { .. }
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
                ..
            } => 2,
            _ => continue,
        };
        widths[index] = true;
    }
    widths
}
