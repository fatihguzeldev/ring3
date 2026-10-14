use crate::cpu::{
    dbt::region::CompiledBlock,
    x86::ir::{BinaryKind, Location32, Operation, UnaryKind},
};

pub(in crate::cpu::dbt::wasm) fn read_modify_needs(blocks: &[CompiledBlock]) -> [bool; 3] {
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
