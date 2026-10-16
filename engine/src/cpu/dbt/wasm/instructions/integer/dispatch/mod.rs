mod arithmetic;
mod control;
mod exchange;
mod extended;
mod moves;
mod predicates;
mod shifts;

use crate::cpu::dbt::wasm::{
    helpers::protocol,
    imports::Imports,
    instructions::{
        atomic,
        integer::{
            BUDGET, BinaryKind, BitIndex, BitTestKind, BlockType, BranchTarget, ByteArithmeticKind,
            ByteLogicalKind, BytePredicateKind, ByteReadArithmeticKind, ByteRegister, ByteValue,
            CarryFlag, CarryKind, Condition, CountBranchKind, DecodedOperation, EIP, ExtensionKind,
            FLAGS, InstructionSink, LHS, Location32, Operation, RESULT, RETIRED, RHS, Register32,
            ShiftCount, SmallSource, SmallWidth, ValType, Value32, WordArithmeticKind,
            WordLogicalKind, WordMemoryArithmeticKind, WordReadArithmeticKind, WordValue,
            accumulator_divide, accumulator_multiply, arithmetic_flags, arithmetic_memory_byte,
            ascii_adjust, ascii_adjust_after_multiplication, ascii_adjust_before_division,
            binary_memory_store, bit_scan, bit_scan_word, bit_test, bit_test_word,
            byte_accumulator_divide, byte_accumulator_divide_value, byte_accumulator_multiply,
            byte_accumulator_multiply_value, byte_rotate_cl, byte_rotate_immediate,
            byte_rotate_memory, byte_rotate_one, byte_rotate_one_flags, byte_rotate_one_value,
            byte_rotate_through_carry, byte_rotate_through_carry_cl,
            byte_rotate_through_carry_memory_cl, byte_rotate_through_carry_memory_immediate,
            byte_rotate_through_carry_one, byte_rotate_through_carry_one_flags,
            byte_rotate_through_carry_one_value, byte_value, control as conditions, decimal_adjust,
            double_shift, double_shift_memory, double_shift_memory_word, double_shift_word,
            extend_byte_to_word, extend_value, indirect_target, insert_byte, logical_flags,
            logical_memory_byte, memory, memory_bit_mutation, memory_bit_test_word,
            memory_rotate_through_carry_cl, memory_rotate_through_carry_immediate, register,
            rotate, rotate_memory, rotate_memory_word, rotate_one, rotate_one_flags,
            rotate_one_value, rotate_through_carry_cl, rotate_through_carry_immediate,
            rotate_through_carry_memory_word, rotate_through_carry_one,
            rotate_through_carry_one_flags, rotate_through_carry_one_value,
            rotate_through_carry_word, rotate_word, shift, shift_byte, shift_memory,
            shift_memory_byte, shift_memory_byte_cl, shift_memory_byte_immediate,
            shift_memory_word, shift_word, signed_multiply, signed_multiply_word,
            trailing_zero_count32, unary, unary_byte, unary_memory, unary_memory_byte,
            unary_memory_word, unary_word, value, word_accumulator_divide,
            word_accumulator_multiply, word_value, x87,
        },
        integer_system, segments, simd, stack, strings,
    },
};
pub(in crate::cpu::dbt::wasm) fn instruction(
    code: &mut InstructionSink<'_>,
    instruction: &DecodedOperation,
    imports: Imports,
    exit_depth: u32,
) {
    if !matches!(
        instruction.operation(),
        Operation::String { .. }
            | Operation::RepeatString { .. }
            | Operation::X87 { .. }
            | Operation::FarCall { .. }
            | Operation::InterruptReturn { .. }
            | Operation::SoftwareInterrupt { .. }
            | Operation::Call { .. }
            | Operation::CallWord { .. }
            | Operation::CallRegisterWord { .. }
            | Operation::ReadCallWord { .. }
    ) && !matches!(instruction.operation(), Operation::Simd(operation) if operation.kind == crate::cpu::x86::simd::SimdKind::MaskedStore)
        && !matches!(instruction.operation(), Operation::Avx(_))
        && !(crate::cpu::x86::capabilities::AVX_ENABLED
            && matches!(instruction.operation(), Operation::XState(_)))
    {
        use crate::cpu::x86::SegmentRegister::*;
        for segment in [Es, Cs, Ss, Ds, Fs, Gs] {
            if instruction.segment_accesses() & (1 << segment.index()) != 0 {
                segments::access(code, segment, exit_depth);
            }
        }
    }
    let store;
    match *instruction.operation() {
        Operation::SoftwareInterrupt { vector } => {
            crate::cpu::dbt::wasm::instructions::software_interrupts::instruction(
                code,
                instruction,
                vector,
                exit_depth,
            );
            return;
        }
        Operation::Control(_) => {
            crate::cpu::dbt::wasm::instructions::control_instructions::instruction(
                code,
                instruction,
                imports,
                exit_depth,
            );
            return;
        }
        Operation::StackFrame { .. }
        | Operation::PopAll
        | Operation::PopAllWord
        | Operation::EnterZero
        | Operation::EnterZeroWord => {
            stack::instruction(code, instruction, imports, exit_depth);
            return;
        }
        Operation::RepeatString { .. } => {
            strings::instruction(code, instruction, imports, exit_depth);
            return;
        }
        Operation::ReadSegment { .. }
        | Operation::PushSegment { .. }
        | Operation::LoadSegment { .. }
        | Operation::PopSegment { .. }
        | Operation::LoadFarPointer { .. } => {
            segments::instruction(code, instruction, imports, exit_depth);
            return;
        }
        Operation::String { .. } => {
            strings::instruction(code, instruction, imports, exit_depth);
            return;
        }
        Operation::Nop => store = false,
        Operation::FarCall { .. }
        | Operation::FarReturn { .. }
        | Operation::InterruptReturn { .. }
        | Operation::FarJump { .. }
        | Operation::Jump { .. }
        | Operation::JumpWord { .. }
        | Operation::ReadJumpWord { .. }
        | Operation::ConditionalJump { .. }
        | Operation::CountBranchWord { .. }
        | Operation::CountBranch { .. }
        | Operation::CallWord { .. }
        | Operation::CallRegisterWord { .. }
        | Operation::ReadCallWord { .. }
        | Operation::Call { .. }
        | Operation::Push { .. }
        | Operation::PushWord { .. }
        | Operation::PushMemoryWord { .. }
        | Operation::PushFlags
        | Operation::PushFlagsWord
        | Operation::Pop { .. }
        | Operation::PopWord { .. }
        | Operation::PopMemoryWord { .. }
        | Operation::Leave
        | Operation::LeaveWord
        | Operation::Return { .. }
        | Operation::ReturnWord { .. } => {
            store = control::emit(code, instruction, imports, exit_depth)
        }
        Operation::SignExtendAccumulatorWord
        | Operation::SignExtendHighWord
        | Operation::SignExtendHigh
        | Operation::DecimalAdjust { .. }
        | Operation::AsciiAdjust { .. }
        | Operation::AsciiAdjustBeforeDivision { .. }
        | Operation::AsciiAdjustAfterMultiplication { .. }
        | Operation::ArithmeticWord { .. }
        | Operation::ReadArithmeticWord { .. }
        | Operation::MemoryArithmeticWord { .. }
        | Operation::MemoryLogicalWord { .. }
        | Operation::ReadLogicalWord { .. }
        | Operation::LogicalWord { .. }
        | Operation::MemoryArithmeticByte { .. }
        | Operation::MemoryLogicalByte { .. }
        | Operation::ReadLogicalByte { .. }
        | Operation::LogicalByte { .. }
        | Operation::ArithmeticByte { .. }
        | Operation::ReadArithmeticByte { .. }
        | Operation::Binary { .. }
        | Operation::UnaryWord { .. }
        | Operation::MemoryUnaryWord { .. }
        | Operation::UnaryByte { .. }
        | Operation::MemoryUnaryByte { .. }
        | Operation::Unary { .. }
        | Operation::SignedMultiply { .. }
        | Operation::SignedMultiplyWord { .. }
        | Operation::ReadSignedMultiplyWord { .. }
        | Operation::SignedMultiplyImmediateWord { .. }
        | Operation::ReadSignedMultiplyImmediateWord { .. }
        | Operation::MultiplyAccumulator { .. }
        | Operation::WordMultiplyAccumulator { .. }
        | Operation::ReadWordMultiplyAccumulator { .. }
        | Operation::ByteMultiplyAccumulator { .. }
        | Operation::ReadByteMultiplyAccumulator { .. }
        | Operation::ReadMultiplyAccumulator { .. }
        | Operation::DivideAccumulator { .. }
        | Operation::WordDivideAccumulator { .. }
        | Operation::ReadWordDivideAccumulator { .. }
        | Operation::ByteDivideAccumulator { .. }
        | Operation::ReadByteDivideAccumulator { .. }
        | Operation::ReadDivideAccumulator { .. } => {
            store = arithmetic::emit(code, instruction, imports, exit_depth)
        }
        Operation::MoveByteSwap { .. }
        | Operation::TranslateByteAddress { .. }
        | Operation::TranslateByte
        | Operation::Move { .. }
        | Operation::MoveByte { .. }
        | Operation::MoveWord { .. }
        | Operation::LoadByte { .. }
        | Operation::StoreByte { .. }
        | Operation::LoadWord { .. }
        | Operation::StoreSystemDescriptor { .. }
        | Operation::StoreWord { .. }
        | Operation::ExtendByteToWord { .. }
        | Operation::ReadExtendByteToWord { .. }
        | Operation::Extend { .. }
        | Operation::ByteSwap { .. }
        | Operation::Lea { .. }
        | Operation::LeaWord { .. } => store = moves::emit(code, instruction, imports, exit_depth),
        Operation::AtomicMemory { .. }
        | Operation::CompareExchange8B { .. }
        | Operation::ExchangeWord { .. }
        | Operation::Exchange { .. }
        | Operation::MemoryExchange { .. }
        | Operation::MemoryExchangeByte { .. }
        | Operation::MemoryExchangeWord { .. }
        | Operation::ExchangeAdd { .. }
        | Operation::ExchangeAddWord { .. }
        | Operation::MemoryExchangeAddWord { .. }
        | Operation::MemoryExchangeAdd { .. }
        | Operation::LockedMemoryExchangeAdd32 { .. }
        | Operation::ExchangeAddByte { .. }
        | Operation::LockedMemoryExchangeAddByte { .. }
        | Operation::LockedMemoryExchangeAddWord { .. }
        | Operation::MemoryExchangeAddByte { .. }
        | Operation::CompareExchange { .. }
        | Operation::CompareExchangeWord { .. }
        | Operation::LockedMemoryCompareExchangeWord { .. }
        | Operation::MemoryCompareExchangeWord { .. }
        | Operation::LockedMemoryCompareExchange32 { .. }
        | Operation::MemoryCompareExchange { .. }
        | Operation::CompareExchangeByte { .. }
        | Operation::LockedMemoryCompareExchangeByte { .. }
        | Operation::MemoryCompareExchangeByte { .. }
        | Operation::ExchangeByte { .. } => {
            store = exchange::emit(code, instruction, imports, exit_depth)
        }
        Operation::CompareWord { .. }
        | Operation::TestWord { .. }
        | Operation::MemoryCompareWord { .. }
        | Operation::ReadCompareWord { .. }
        | Operation::MemoryTestWord { .. }
        | Operation::SetByte { .. }
        | Operation::ConditionalMove { .. }
        | Operation::ConditionalMoveWord { .. }
        | Operation::ReadConditionalMove { .. }
        | Operation::ReadConditionalMoveWord { .. }
        | Operation::MemorySetByte { .. }
        | Operation::CompareByte { .. }
        | Operation::ReadCompareByte { .. }
        | Operation::TestByte { .. }
        | Operation::MemoryPredicateByte { .. }
        | Operation::BitTest { .. }
        | Operation::BitTestWord { .. }
        | Operation::ReadBitTestWord { .. }
        | Operation::MemoryBitMutationWord { .. }
        | Operation::LockedMemoryBitSet32 { .. }
        | Operation::LockedMemoryBitReset32 { .. }
        | Operation::LockedMemoryOr32 { .. }
        | Operation::LockedMemoryAnd32 { .. }
        | Operation::LockedMemoryXor32 { .. }
        | Operation::LockedMemoryOrRegister32 { .. }
        | Operation::LockedMemoryAndRegister32 { .. }
        | Operation::LockedMemoryXorRegister32 { .. }
        | Operation::MemoryBitMutation { .. }
        | Operation::ReadBitTest { .. }
        | Operation::TrailingZeroCount32 { .. }
        | Operation::ReadTrailingZeroCount32 { .. }
        | Operation::BitScan { .. }
        | Operation::BitScanWord { .. }
        | Operation::ReadBitScan { .. }
        | Operation::ReadBitScanWord { .. } => {
            store = predicates::emit(code, instruction, imports, exit_depth)
        }
        Operation::RotateWord { .. }
        | Operation::MemoryRotateWord { .. }
        | Operation::RotateThroughCarryWord { .. }
        | Operation::MemoryRotateThroughCarryWord { .. }
        | Operation::ShiftWord { .. }
        | Operation::MemoryShiftWord { .. }
        | Operation::ShiftByte { .. }
        | Operation::MemoryShiftByte { .. }
        | Operation::MemoryShiftByteImmediate { .. }
        | Operation::MemoryShiftByteCl { .. }
        | Operation::Shift { .. }
        | Operation::DoubleShift { .. }
        | Operation::DoubleShiftWord { .. }
        | Operation::MemoryDoubleShiftWord { .. }
        | Operation::MemoryDoubleShift { .. }
        | Operation::ByteRotateImmediate { .. }
        | Operation::ByteRotateCl { .. }
        | Operation::ByteRotateOne { .. }
        | Operation::ByteRotateThroughCarryOne { .. }
        | Operation::ByteRotateThroughCarryImmediate { .. }
        | Operation::ByteRotateThroughCarryCl { .. }
        | Operation::MemoryByteRotateThroughCarryOne { .. }
        | Operation::MemoryByteRotateThroughCarryImmediate { .. }
        | Operation::MemoryByteRotateThroughCarryCl { .. }
        | Operation::MemoryByteRotateOne { .. }
        | Operation::MemoryByteRotate { .. }
        | Operation::RotateOne { .. }
        | Operation::Rotate { .. }
        | Operation::MemoryRotate { .. }
        | Operation::RotateThroughCarryOne { .. }
        | Operation::RotateThroughCarryImmediate { .. }
        | Operation::RotateThroughCarryCl { .. }
        | Operation::MemoryRotateThroughCarryOne { .. }
        | Operation::MemoryRotateThroughCarryImmediate { .. }
        | Operation::MemoryRotateThroughCarryCl { .. }
        | Operation::MemoryRotateOne { .. } => {
            store = shifts::emit(code, instruction, imports, exit_depth)
        }
        Operation::QuerySegment { .. }
        | Operation::VerifySegment { .. }
        | Operation::XState(..)
        | Operation::AdjustPrivilege { .. }
        | Operation::CacheFlush { .. }
        | Operation::VirtualTimestampAux
        | Operation::PackedStringCompare(..)
        | Operation::Crc32 { .. }
        | Operation::MultiplyExtended { .. }
        | Operation::BitManipulation { .. }
        | Operation::BitCount { .. }
        | Operation::X87 { .. }
        | Operation::Avx(..)
        | Operation::Simd(..)
        | Operation::InitializeX87
        | Operation::ClearX87Exceptions
        | Operation::X87StatusToAx
        | Operation::X87StatusToMemory { .. }
        | Operation::X87ControlToMemory { .. }
        | Operation::FlagsToAh
        | Operation::AhToFlags
        | Operation::SetAlFromCarry
        | Operation::Carry { .. }
        | Operation::Direction { .. }
        | Operation::CpuId
        | Operation::ReadProcessorId { .. }
        | Operation::VirtualTimestamp => {
            store = extended::emit(code, instruction, imports, exit_depth)
        }
    }
    if !matches!(
        instruction.operation(),
        Operation::FarJump { .. }
            | Operation::FarCall { .. }
            | Operation::FarReturn { .. }
            | Operation::InterruptReturn { .. }
            | Operation::SoftwareInterrupt { .. }
            | Operation::Jump { .. }
            | Operation::JumpWord { .. }
            | Operation::ReadJumpWord { .. }
            | Operation::ConditionalJump { .. }
            | Operation::CountBranch { .. }
            | Operation::CountBranchWord { .. }
            | Operation::Call { .. }
            | Operation::CallWord { .. }
            | Operation::CallRegisterWord { .. }
            | Operation::ReadCallWord { .. }
            | Operation::Return { .. }
            | Operation::ReturnWord { .. }
    ) {
        code.i32_const(instruction.next_pc().0 as i32)
            .local_set(EIP);
    }
    code.local_get(BUDGET)
        .i32_const(1)
        .i32_sub()
        .local_set(BUDGET)
        .local_get(RETIRED)
        .i32_const(1)
        .i32_add()
        .local_set(RETIRED);
    if store {
        protocol::exit_if_invalidated(code, exit_depth, imports);
    }
}

fn check_near_target(
    code: &mut InstructionSink<'_>,
    target: u32,
    imports: Imports,
    exit_depth: u32,
) {
    if let Some(limit) = imports.code_fetch_limit {
        crate::cpu::dbt::wasm::code_fetch::check_span(code, target, 1, limit, exit_depth);
    }
}

fn check_near_value(
    code: &mut InstructionSink<'_>,
    target: u32,
    imports: Imports,
    exit_depth: u32,
) {
    if let Some(limit) = imports.code_fetch_limit {
        crate::cpu::dbt::wasm::code_fetch::check_value(code, target, limit, exit_depth);
    }
}
