use super::{ArithmeticError, MemoryRequirement, VectorState};
use crate::{
    cpu::x86::{
        Register32, State32,
        packed_strings::{PackedStringCompare, PackedStringOutput, PackedStringSource},
    },
    memory::Access,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct StringInstruction(PackedStringCompare);

impl StringInstruction {
    pub(crate) fn new(comparison: PackedStringCompare) -> Result<Self, ArithmeticError> {
        if !comparison.valid() {
            return Err(ArithmeticError::InvalidRegister);
        }
        Ok(Self(comparison))
    }

    pub(crate) fn memory(self) -> Option<MemoryRequirement> {
        matches!(self.0.right, PackedStringSource::Memory(_)).then_some(MemoryRequirement {
            access: Access::Read,
            bytes: 16,
            alignment: 1,
        })
    }
}

impl VectorState {
    pub(crate) fn execute_string(
        &mut self,
        instruction: StringInstruction,
        general: &mut State32,
        operand: &[u8],
    ) -> Result<(), ArithmeticError> {
        if operand.len() != instruction.memory().map_or(0, |r| usize::from(r.bytes)) {
            return Err(ArithmeticError::OperandLength);
        }
        if self.simd.mxcsr & !0xffff != 0 {
            return Err(ArithmeticError::InvalidState);
        }
        let comparison = instruction.0;
        let left = self.simd.xmm[usize::from(comparison.left)];
        let right = match comparison.right {
            PackedStringSource::Xmm(index) => self.simd.xmm[usize::from(index)],
            PackedStringSource::Memory(_) => operand.try_into().unwrap(),
        };
        let result = comparison.compare(
            left,
            right,
            general.registers[Register32::Eax.index()],
            general.registers[Register32::Edx.index()],
            general.eflags,
        );
        match comparison.output {
            PackedStringOutput::Index => {
                general.registers[Register32::Ecx.index()] = result.index(comparison.control);
            }
            PackedStringOutput::Mask => {
                self.simd.xmm[0] = result.mask(comparison.control);
                self.upper[0] = [0; 16];
            }
        }
        general.eflags = result.flags;
        Ok(())
    }
}

#[cfg(test)]
mod native;
