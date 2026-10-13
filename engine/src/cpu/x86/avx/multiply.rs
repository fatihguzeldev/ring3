use super::{MemoryRequirement, MoveSource, VectorRegister, VectorState};
use crate::{cpu::x86::simd::MultiplyOperation, memory::Access};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct MultiplyInstruction {
    operation: MultiplyOperation,
    destination: VectorRegister,
    first: Option<VectorRegister>,
    second: MoveSource,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MultiplyError {
    OperandLength,
    InvalidState,
}
impl MultiplyInstruction {
    pub(crate) fn new(
        operation: MultiplyOperation,
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
    pub(crate) fn operation(self) -> MultiplyOperation {
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
    pub(crate) fn execute_multiply(
        &mut self,
        instruction: MultiplyInstruction,
        operand: &[u8],
    ) -> Result<(), MultiplyError> {
        if operand.len() != instruction.memory().map_or(0, |r| usize::from(r.bytes)) {
            return Err(MultiplyError::OperandLength);
        }
        if self.simd.mxcsr & !0xffff != 0 {
            return Err(MultiplyError::InvalidState);
        }
        let a = instruction
            .first
            .map_or([0; 16], |r| self.simd.xmm[r.index()]);
        let b = match instruction.second {
            MoveSource::Register(r) => self.simd.xmm[r.index()],
            MoveSource::Memory => operand.try_into().unwrap(),
        };
        let mut value = [0; 32];
        value[..16].copy_from_slice(&instruction.operation.evaluate(a, b));
        self.publish(instruction.destination, value);
        Ok(())
    }
}
#[cfg(test)]
#[path = "multiply/native.rs"]
mod tests;
