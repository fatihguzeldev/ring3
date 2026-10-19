use super::PreparedRegion;
use crate::cpu::x86::{X87Operation, ir::Operation};

// this certifies structural validity after admitted generated execution. it does
// not certify unchanged values, current source, or permission to enter a region.
// the caller must establish valid records first and exclude external writers.
pub(super) fn preserves_extended_record_validity(prepared: &PreparedRegion) -> bool {
    !prepared.blocks.is_empty()
        && prepared.blocks.iter().all(|block| {
            block.gate().is_none()
                && block.context_fault().is_none()
                && !block.instructions.is_empty()
                && block
                    .instructions
                    .iter()
                    .all(|instruction| preserves_operation(instruction.operation()))
        })
}

fn preserves_operation(operation: &Operation) -> bool {
    match operation {
        // scalar emitters change integer locals, state metadata, helper packets
        // and guest ram. scalar paging/store helpers do not write extended banks.
        Operation::Nop
        | Operation::Carry { .. }
        | Operation::SetAlFromCarry
        | Operation::Direction { .. }
        | Operation::FlagsToAh
        | Operation::AhToFlags
        | Operation::Move { .. }
        | Operation::MoveByte { .. }
        | Operation::MoveWord { .. }
        | Operation::LoadByte { .. }
        | Operation::StoreByte { .. }
        | Operation::LoadWord { .. }
        | Operation::StoreWord { .. }
        | Operation::ExtendByteToWord { .. }
        | Operation::ReadExtendByteToWord { .. }
        | Operation::Extend { .. }
        | Operation::SignExtendAccumulatorWord
        | Operation::SignExtendHighWord
        | Operation::SignExtendHigh
        | Operation::ByteSwap { .. }
        | Operation::Lea { .. }
        | Operation::LeaWord { .. }
        | Operation::Binary { .. }
        | Operation::Unary { .. }
        | Operation::UnaryByte { .. }
        | Operation::UnaryWord { .. }
        | Operation::MemoryUnaryByte { .. }
        | Operation::MemoryUnaryWord { .. }
        | Operation::ArithmeticWord { .. }
        | Operation::ReadArithmeticWord { .. }
        | Operation::MemoryArithmeticWord { .. }
        | Operation::LogicalWord { .. }
        | Operation::ReadLogicalWord { .. }
        | Operation::MemoryLogicalWord { .. }
        | Operation::ArithmeticByte { .. }
        | Operation::ReadArithmeticByte { .. }
        | Operation::MemoryArithmeticByte { .. }
        | Operation::LogicalByte { .. }
        | Operation::ReadLogicalByte { .. }
        | Operation::MemoryLogicalByte { .. }
        | Operation::CompareWord { .. }
        | Operation::TestWord { .. }
        | Operation::MemoryCompareWord { .. }
        | Operation::ReadCompareWord { .. }
        | Operation::MemoryTestWord { .. }
        | Operation::CompareByte { .. }
        | Operation::ReadCompareByte { .. }
        | Operation::TestByte { .. }
        | Operation::MemoryPredicateByte { .. }
        | Operation::SetByte { .. }
        | Operation::MemorySetByte { .. }
        | Operation::ConditionalMove { .. }
        | Operation::ConditionalMoveWord { .. }
        | Operation::ReadConditionalMove { .. }
        | Operation::ReadConditionalMoveWord { .. }
        | Operation::Shift { .. }
        | Operation::ShiftByte { .. }
        | Operation::ShiftWord { .. }
        | Operation::MemoryShiftByte { .. }
        | Operation::MemoryShiftByteImmediate { .. }
        | Operation::MemoryShiftByteCl { .. }
        | Operation::MemoryShiftWord { .. }
        | Operation::DoubleShift { .. }
        | Operation::DoubleShiftWord { .. }
        | Operation::MemoryDoubleShift { .. }
        | Operation::MemoryDoubleShiftWord { .. }
        | Operation::ByteRotateImmediate { .. }
        | Operation::ByteRotateCl { .. }
        | Operation::ByteRotateOne { .. }
        | Operation::MemoryByteRotateOne { .. }
        | Operation::MemoryByteRotate { .. }
        | Operation::RotateOne { .. }
        | Operation::Rotate { .. }
        | Operation::MemoryRotate { .. }
        | Operation::MemoryRotateOne { .. }
        | Operation::RotateThroughCarryOne { .. }
        | Operation::RotateThroughCarryImmediate { .. }
        | Operation::RotateThroughCarryCl { .. }
        | Operation::ByteRotateThroughCarryOne { .. }
        | Operation::ByteRotateThroughCarryImmediate { .. }
        | Operation::ByteRotateThroughCarryCl { .. }
        | Operation::MemoryByteRotateThroughCarryOne { .. }
        | Operation::MemoryByteRotateThroughCarryImmediate { .. }
        | Operation::MemoryByteRotateThroughCarryCl { .. }
        | Operation::MemoryRotateThroughCarryOne { .. }
        | Operation::MemoryRotateThroughCarryImmediate { .. }
        | Operation::MemoryRotateThroughCarryCl { .. }
        | Operation::RotateWord { .. }
        | Operation::MemoryRotateWord { .. }
        | Operation::RotateThroughCarryWord { .. }
        | Operation::MemoryRotateThroughCarryWord { .. }
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
        | Operation::ReadDivideAccumulator { .. }
        | Operation::Exchange { .. }
        | Operation::ExchangeByte { .. }
        | Operation::ExchangeWord { .. }
        | Operation::ExchangeAdd { .. }
        | Operation::ExchangeAddByte { .. }
        | Operation::ExchangeAddWord { .. }
        | Operation::CompareExchange { .. }
        | Operation::CompareExchangeByte { .. }
        | Operation::CompareExchangeWord { .. }
        | Operation::MemoryExchange { .. }
        | Operation::MemoryExchangeByte { .. }
        | Operation::MemoryExchangeWord { .. }
        | Operation::BitTest { .. }
        | Operation::BitTestWord { .. }
        | Operation::ReadBitTest { .. }
        | Operation::ReadBitTestWord { .. }
        | Operation::BitScan { .. }
        | Operation::BitScanWord { .. }
        | Operation::ReadBitScan { .. }
        | Operation::ReadBitScanWord { .. }
        | Operation::TrailingZeroCount32 { .. }
        | Operation::ReadTrailingZeroCount32 { .. }
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
        | Operation::ReturnWord { .. } => true,

        // inline fninit/fnclex modify only defined fp fields, preserving its
        // header, reserved words and bounded opcode. status/control stores use
        // the same scalar guest-memory helper as integer stores.
        Operation::InitializeX87
        | Operation::ClearX87Exceptions
        | Operation::X87StatusToAx
        | Operation::X87StatusToMemory { .. }
        | Operation::X87ControlToMemory { .. } => true,

        // scalar, pair and group helpers publish through encode_x87. final
        // stores encode before guest memory publication; partial prefixes also
        // use that encoder. the certificate is independent of numerical values.
        Operation::X87 { operation, .. } => preserves_x87(*operation),

        // segment writers, simd/mmx/avx, xstate, far/system/complex helpers and
        // future ir variants need a separate writer audit before admission.
        _ => false,
    }
}

fn preserves_x87(operation: X87Operation) -> bool {
    matches!(
        operation,
        X87Operation::Initialize
            | X87Operation::ClearExceptions
            | X87Operation::Wait
            | X87Operation::Load(_)
            | X87Operation::Store { .. }
            | X87Operation::LoadRegister { .. }
            | X87Operation::StoreRegister { .. }
            | X87Operation::StoreRegisterNoUnderflow { .. }
            | X87Operation::Exchange { .. }
            | X87Operation::Binary { .. }
            | X87Operation::BinaryMemory { .. }
            | X87Operation::Compare { .. }
            | X87Operation::CompareMemory { .. }
            | X87Operation::ConditionalMove { .. }
            | X87Operation::Test
            | X87Operation::Examine
            | X87Operation::ChangeSign
            | X87Operation::Absolute
            | X87Operation::SquareRoot
            | X87Operation::RoundToInteger
            | X87Operation::Scale
            | X87Operation::Extract
            | X87Operation::Remainder { .. }
            | X87Operation::LoadConstant { .. }
            | X87Operation::Free { .. }
            | X87Operation::AdjustTop { .. }
            | X87Operation::LoadControl
            | X87Operation::StoreControl
            | X87Operation::StoreStatus
            | X87Operation::StatusToAx
            | X87Operation::NoOperation
            | X87Operation::Exp2MinusOne
            | X87Operation::Log2Multiply { .. }
            | X87Operation::PartialAtan
            | X87Operation::Trigonometric { .. }
    )
}
