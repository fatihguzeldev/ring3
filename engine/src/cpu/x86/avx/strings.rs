use super::{ArithmeticError, MemoryRequirement, MoveSource, VectorRegister, VectorState};
use crate::{
    cpu::x86::{
        Register32, State32,
        packed_strings::{PackedStringCompare, PackedStringOutput, PackedStringSource},
    },
    memory::Access,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct StringInstruction {
    left: VectorRegister,
    right: MoveSource,
    length: crate::cpu::x86::packed_strings::PackedStringLength,
    output: PackedStringOutput,
    control: u8,
}

impl StringInstruction {
    #[cfg(test)]
    pub(crate) fn new(comparison: PackedStringCompare) -> Result<Self, ArithmeticError> {
        if !comparison.valid() {
            return Err(ArithmeticError::InvalidRegister);
        }
        Ok(Self::from_parts(
            VectorRegister::new(comparison.left)?,
            match comparison.right {
                PackedStringSource::Xmm(index) => MoveSource::Register(VectorRegister::new(index)?),
                PackedStringSource::Memory(_) => MoveSource::Memory,
            },
            comparison.length,
            comparison.output,
            comparison.control,
        ))
    }
    pub(crate) fn from_parts(
        left: VectorRegister,
        right: MoveSource,
        length: crate::cpu::x86::packed_strings::PackedStringLength,
        output: PackedStringOutput,
        control: u8,
    ) -> Self {
        Self {
            left,
            right,
            length,
            output,
            control,
        }
    }
    #[cfg(test)]
    pub(crate) fn right(self) -> MoveSource {
        self.right
    }
    pub(crate) fn length(self) -> crate::cpu::x86::packed_strings::PackedStringLength {
        self.length
    }
    pub(crate) fn output(self) -> PackedStringOutput {
        self.output
    }
    #[cfg(test)]
    pub(crate) fn control(self) -> u8 {
        self.control
    }
    pub(crate) fn memory(self) -> Option<MemoryRequirement> {
        (self.right == MoveSource::Memory).then_some(MemoryRequirement {
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
        // the shared value evaluator consumes captured bytes and explicit lengths;
        // its operand bindings are not consulted during comparison.
        let comparison = PackedStringCompare {
            left: instruction.left.0,
            right: PackedStringSource::Xmm(0),
            length: instruction.length,
            output: instruction.output,
            control: instruction.control,
        };
        let left = self.simd.xmm[usize::from(comparison.left)];
        let right = match instruction.right {
            MoveSource::Register(register) => self.simd.xmm[register.index()],
            MoveSource::Memory => operand.try_into().unwrap(),
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
