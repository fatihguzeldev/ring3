use super::{MemoryRequirement, VectorRegister, VectorState, WordSource, WordTarget};
use crate::{
    cpu::x86::{
        Register32, State32,
        simd::{ScalarOperation, ScalarWidth},
    },
    memory::Access,
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ScalarInstruction {
    Insert {
        width: ScalarWidth,
        destination: VectorRegister,
        merge: VectorRegister,
        source: WordSource,
        immediate: u8,
    },
    Extract {
        width: ScalarWidth,
        source: VectorRegister,
        destination: WordTarget,
        immediate: u8,
    },
    Mask {
        source: VectorRegister,
        destination: Register32,
    },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ScalarError {
    OperandLength,
    InvalidState,
}
impl ScalarInstruction {
    pub(crate) fn memory(self) -> Option<MemoryRequirement> {
        let (access, bytes) = match self {
            Self::Insert {
                width,
                source: WordSource::Memory,
                ..
            } => (Access::Read, width.bytes()),
            Self::Extract {
                width,
                destination: WordTarget::Memory,
                ..
            } => (Access::Write, width.bytes()),
            _ => return None,
        };
        Some(MemoryRequirement {
            access,
            bytes,
            alignment: 1,
        })
    }
}
impl VectorState {
    pub(crate) fn execute_scalar(
        &mut self,
        instruction: ScalarInstruction,
        general: &mut State32,
        operand: &mut [u8],
    ) -> Result<(), ScalarError> {
        if operand.len() != instruction.memory().map_or(0, |r| usize::from(r.bytes)) {
            return Err(ScalarError::OperandLength);
        }
        if self.simd.mxcsr & !0xffff != 0 {
            return Err(ScalarError::InvalidState);
        }
        match instruction {
            ScalarInstruction::Insert {
                width,
                destination,
                merge,
                source,
                immediate,
            } => {
                let first = self.simd.xmm[merge.index()];
                let mut scalar = [0; 16];
                match source {
                    WordSource::Register(r) => {
                        scalar[..4].copy_from_slice(&general.registers[r.index()].to_le_bytes())
                    }
                    WordSource::Memory => scalar[..operand.len()].copy_from_slice(operand),
                }
                let mut value = [0; 32];
                value[..16].copy_from_slice(
                    &ScalarOperation::Insert { width, immediate }
                        .evaluate(first, scalar)
                        .0,
                );
                self.publish(destination, value);
            }
            ScalarInstruction::Extract {
                width,
                source,
                destination,
                immediate,
            } => {
                let value = ScalarOperation::Extract { width, immediate }
                    .evaluate([0; 16], self.simd.xmm[source.index()])
                    .1
                    .unwrap();
                match destination {
                    WordTarget::Register(r) => general.registers[r.index()] = value,
                    WordTarget::Memory => {
                        operand.copy_from_slice(&value.to_le_bytes()[..usize::from(width.bytes())])
                    }
                }
            }
            ScalarInstruction::Mask {
                source,
                destination,
            } => {
                general.registers[destination.index()] = ScalarOperation::MaskBytes
                    .evaluate([0; 16], self.simd.xmm[source.index()])
                    .1
                    .unwrap()
            }
        }
        Ok(())
    }
}
#[cfg(test)]
#[path = "scalar/native.rs"]
mod tests;
