use super::{MemoryRequirement, MoveSource, VectorRegister, VectorState};
use crate::{cpu::x86::simd::IntegerLaneOperation, memory::Access};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct IntegerInstruction {
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
    pub(crate) fn new(
        operation: IntegerLaneOperation,
        destination: VectorRegister,
        first: Option<VectorRegister>,
        second: MoveSource,
    ) -> Option<Self> {
        (operation.unary() == first.is_none()).then_some(Self {
            operation,
            destination,
            first,
            second,
        })
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
            bytes: 16,
            alignment: 1,
        })
    }
}

impl VectorState {
    // these are avx xmm integer forms; ymm integer lanes require avx2.
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
        let first = instruction
            .first
            .map_or([0; 16], |r| self.simd.xmm[r.index()]);
        let second = match instruction.second {
            MoveSource::Register(register) => self.simd.xmm[register.index()],
            MoveSource::Memory => operand.try_into().unwrap(),
        };
        let mut value = [0; 32];
        value[..16].copy_from_slice(&instruction.operation.evaluate(first, second));
        self.publish(instruction.destination, value);
        Ok(())
    }
}

#[cfg(test)]
#[path = "integer/native.rs"]
mod tests;
