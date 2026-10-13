use super::{MemoryRequirement, MoveSource, VectorRegister, VectorState, VectorWidth};
use crate::{
    cpu::x86::simd::{IntegerLaneOperation, PackedKind},
    memory::Access,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct IntegerInstruction {
    width: VectorWidth,
    operation: IntegerLaneOperation,
    destination: VectorRegister,
    first: Option<VectorRegister>,
    second: MoveSource,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum IntegerError {
    OperandLength,
    InvalidState,
}

impl IntegerInstruction {
    #[cfg(test)]
    pub(crate) fn new(
        operation: IntegerLaneOperation,
        destination: VectorRegister,
        first: Option<VectorRegister>,
        second: MoveSource,
    ) -> Option<Self> {
        Self::with_width(VectorWidth::Xmm, operation, destination, first, second)
    }

    pub(crate) fn with_width(
        width: VectorWidth,
        operation: IntegerLaneOperation,
        destination: VectorRegister,
        first: Option<VectorRegister>,
        second: MoveSource,
    ) -> Option<Self> {
        let supported_width = width == VectorWidth::Xmm
            || matches!(operation.kind(), PackedKind::Add | PackedKind::Subtract);
        (supported_width && operation.unary() == first.is_none()).then_some(Self {
            width,
            operation,
            destination,
            first,
            second,
        })
    }

    pub(crate) fn width(self) -> VectorWidth {
        self.width
    }

    pub(crate) fn operation(self) -> IntegerLaneOperation {
        self.operation
    }

    pub(crate) fn destination(self) -> VectorRegister {
        self.destination
    }

    pub(crate) fn first(self) -> Option<VectorRegister> {
        self.first
    }

    pub(crate) fn second(self) -> MoveSource {
        self.second
    }

    pub(crate) fn memory(self) -> Option<MemoryRequirement> {
        (self.second == MoveSource::Memory).then_some(MemoryRequirement {
            access: Access::Read,
            bytes: self.width.bytes() as u8,
            alignment: 1,
        })
    }
}

impl VectorState {
    pub(crate) fn execute_integer(
        &mut self,
        instruction: IntegerInstruction,
        operand: &[u8],
    ) -> Result<(), IntegerError> {
        let bytes = instruction.memory().map_or(0, |r| usize::from(r.bytes));
        if operand.len() != bytes {
            return Err(IntegerError::OperandLength);
        }
        if self.simd.mxcsr & !0xffff != 0 {
            return Err(IntegerError::InvalidState);
        }
        let width = instruction.width.bytes();
        let first = instruction
            .first
            .map_or([0; 32], |register| self.read(register));
        let second = match instruction.second {
            MoveSource::Register(register) => self.read(register),
            MoveSource::Memory => {
                let mut value = [0; 32];
                value[..width].copy_from_slice(operand);
                value
            }
        };
        let mut value = [0; 32];
        for offset in (0..width).step_by(16) {
            let first = first[offset..offset + 16].try_into().unwrap();
            let second = second[offset..offset + 16].try_into().unwrap();
            value[offset..offset + 16]
                .copy_from_slice(&instruction.operation.evaluate(first, second));
        }
        self.publish(instruction.destination, value);
        Ok(())
    }
}

#[cfg(test)]
#[path = "integer/native.rs"]
mod tests;

#[cfg(test)]
#[path = "integer/wrapping/native.rs"]
mod wrapping;
