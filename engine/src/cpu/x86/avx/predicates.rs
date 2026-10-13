use super::{
    ArithmeticError, BitTestKind, MemoryRequirement, MoveSource, VectorRegister, VectorState,
    VectorWidth,
};
use crate::{
    cpu::x86::{State32, simd::test_flags},
    memory::Access,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TestInstruction {
    pub kind: BitTestKind,
    pub width: VectorWidth,
    pub first: VectorRegister,
    pub second: MoveSource,
}

impl TestInstruction {
    pub(crate) fn memory(self) -> Option<MemoryRequirement> {
        (self.second == MoveSource::Memory).then_some(MemoryRequirement {
            access: Access::Read,
            bytes: self.width.bytes() as u8,
            alignment: 1,
        })
    }
}

impl VectorState {
    pub(crate) fn execute_test(
        &self,
        instruction: TestInstruction,
        general: &mut State32,
        operand: &[u8],
    ) -> Result<(), ArithmeticError> {
        if operand.len() != instruction.memory().map_or(0, |r| usize::from(r.bytes)) {
            return Err(ArithmeticError::OperandLength);
        }
        if self.simd.mxcsr & !0xffff != 0 {
            return Err(ArithmeticError::InvalidState);
        }
        let first = self.read(instruction.first);
        let second = match instruction.second {
            MoveSource::Register(register) => self.read(register),
            MoveSource::Memory => {
                let mut bytes = [0; 32];
                bytes[..operand.len()].copy_from_slice(operand);
                bytes
            }
        };
        let bytes = instruction.width.bytes();
        general.eflags = test_flags(
            &first[..bytes],
            &second[..bytes],
            instruction.kind,
            general.eflags,
        );
        Ok(())
    }
}

#[cfg(test)]
mod native;
