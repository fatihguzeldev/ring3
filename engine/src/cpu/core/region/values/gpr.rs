use super::*;
use crate::cpu::x86::ir::{
    BinaryKind, ByteRegister, CountBranchKind, Location32, Operation, Value32,
};

pub(super) fn lower(
    plan: &mut RegionPlan,
    frame: &mut FrameState,
    operation: Option<Operation>,
    point: ProgramPoint,
) {
    let old = frame.gpr;
    let mut facts = old.map(|v| plan.constant(v));
    flow::scalar_transfer(&mut facts, operation);
    let mut written = [true; 8];
    match operation {
        Some(Operation::Move {
            destination: Location32::Register(r),
            source,
        }) => {
            written = [false; 8];
            written[r.index()] = true;
            frame.gpr[r.index()] = match source {
                Value32::Register(source) => old[source.index()],
                Value32::Immediate(value) => plan.constant_value(value),
                _ => lower::definition(plan, point, r.index() as u8, ValueType::I32),
            };
        }
        Some(Operation::Move {
            destination: Location32::Memory(_),
            ..
        }) => {
            written = [false; 8];
        }
        Some(Operation::Lea { destination, .. }) => {
            written = [false; 8];
            written[destination.index()] = true;
        }
        Some(
            Operation::LoadWord { destination, .. }
            | Operation::ReadArithmeticWord { destination, .. }
            | Operation::ReadLogicalWord { destination, .. }
            | Operation::ReadConditionalMove { destination, .. }
            | Operation::ReadConditionalMoveWord { destination, .. }
            | Operation::ReadSignedMultiplyWord { destination, .. }
            | Operation::ReadSignedMultiplyImmediateWord { destination, .. }
            | Operation::ReadExtendByteToWord { destination, .. }
            | Operation::Extend { destination, .. },
        ) => {
            written = [false; 8];
            written[destination.index()] = true;
        }
        Some(
            Operation::LoadByte { destination, .. }
            | Operation::ReadArithmeticByte { destination, .. }
            | Operation::ReadLogicalByte { destination, .. },
        ) => {
            written = [false; 8];
            let index = match destination {
                ByteRegister::Al | ByteRegister::Ah => 0,
                ByteRegister::Cl | ByteRegister::Ch => 1,
                ByteRegister::Dl | ByteRegister::Dh => 2,
                ByteRegister::Bl | ByteRegister::Bh => 3,
            };
            written[index] = true;
        }
        Some(operation @ Operation::Simd(_))
            if crate::cpu::core::emitter::preserves_fp_success(&operation) =>
        {
            written = [false; 8];
        }
        Some(Operation::Binary {
            kind, destination, ..
        }) => {
            written = [false; 8];
            if let Location32::Register(r) = destination {
                if !matches!(kind, BinaryKind::Cmp | BinaryKind::Test) {
                    written[r.index()] = true;
                }
            }
        }
        Some(Operation::Unary {
            destination: Location32::Register(r),
            ..
        }) => {
            written = [false; 8];
            written[r.index()] = true;
        }
        Some(Operation::CountBranch { kind, .. } | Operation::CountBranchWord { kind, .. }) => {
            written = [false; 8];
            written[1] = kind != CountBranchKind::EcxZero;
        }
        Some(Operation::Push { .. } | Operation::PushFlags) => {
            written = [false; 8];
            written[4] = true;
        }
        Some(Operation::Pop { destination }) => {
            written = [false; 8];
            written[4] = true;
            if let Location32::Register(r) = destination {
                written[r.index()] = true;
            }
        }
        Some(
            Operation::X87StatusToAx
            | Operation::X87 {
                operation: X87Operation::StatusToAx,
                ..
            },
        ) => {
            written = [false; 8];
            written[0] = true;
        }
        Some(
            Operation::X87 { .. }
            | Operation::Nop
            | Operation::StoreByte { .. }
            | Operation::StoreWord { .. }
            | Operation::Jump { .. }
            | Operation::ConditionalJump { .. }
            | Operation::Carry { .. }
            | Operation::Direction { .. }
            | Operation::Unary {
                destination: Location32::Memory(_),
                ..
            }
            | Operation::MemoryUnaryByte { .. }
            | Operation::MemoryUnaryWord { .. }
            | Operation::MemoryArithmeticByte { .. }
            | Operation::MemoryArithmeticWord { .. }
            | Operation::MemoryLogicalByte { .. }
            | Operation::MemoryLogicalWord { .. }
            | Operation::MemoryCompareWord { .. }
            | Operation::MemoryTestWord { .. }
            | Operation::MemoryPredicateByte { .. }
            | Operation::ReadCompareByte { .. }
            | Operation::ReadCompareWord { .. }
            | Operation::MemorySetByte { .. },
        ) => written = [false; 8],
        _ => {}
    }
    for i in 0..8 {
        if written[i] {
            if !matches!(operation,Some(Operation::Move{destination:Location32::Register(r),..})if r.index()==i)
            {
                frame.gpr[i] = if let Some(value) = facts[i] {
                    plan.constant_value(value)
                } else {
                    lower::definition(plan, point, i as u8, ValueType::I32)
                };
            }
        }
    }
}
