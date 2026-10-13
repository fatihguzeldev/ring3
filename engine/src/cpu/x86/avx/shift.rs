use super::{MemoryRequirement, MoveSource, VectorRegister, VectorState, VectorWidth};
use crate::{cpu::x86::simd::ShiftOperation, memory::Access};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ShiftCount {
    Immediate(u8),
    Register(VectorRegister),
    Memory,
    Elements(MoveSource),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ShiftInstruction {
    width: VectorWidth,
    operation: ShiftOperation,
    destination: VectorRegister,
    source: VectorRegister,
    count: ShiftCount,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ShiftError {
    OperandLength,
    InvalidState,
}
impl ShiftInstruction {
    #[cfg(test)]
    pub(crate) fn new(
        operation: ShiftOperation,
        destination: VectorRegister,
        source: VectorRegister,
        count: ShiftCount,
    ) -> Option<Self> {
        Self::with_width(VectorWidth::Xmm, operation, destination, source, count)
    }
    pub(crate) fn with_width(
        width: VectorWidth,
        operation: ShiftOperation,
        destination: VectorRegister,
        source: VectorRegister,
        count: ShiftCount,
    ) -> Option<Self> {
        let valid = match count {
            ShiftCount::Elements(_) => operation.supports_elements(),
            ShiftCount::Immediate(_) => true,
            ShiftCount::Register(_) | ShiftCount::Memory => operation.byte_kind().is_none(),
        };
        valid.then_some(Self {
            width,
            operation,
            destination,
            source,
            count,
        })
    }
    pub(crate) fn requires_avx2(self) -> bool {
        self.width == VectorWidth::Ymm || matches!(self.count, ShiftCount::Elements(_))
    }

    pub(crate) fn width(self) -> VectorWidth {
        self.width
    }
    pub(crate) fn operation(self) -> ShiftOperation {
        self.operation
    }
    pub(crate) fn destination(self) -> VectorRegister {
        self.destination
    }
    pub(crate) fn source(self) -> VectorRegister {
        self.source
    }
    pub(crate) fn count(self) -> ShiftCount {
        self.count
    }
    pub(crate) fn memory(self) -> Option<MemoryRequirement> {
        let bytes = match self.count {
            ShiftCount::Memory => 16,
            ShiftCount::Elements(MoveSource::Memory) => self.width.bytes() as u8,
            _ => return None,
        };
        Some(MemoryRequirement {
            access: Access::Read,
            bytes,
            alignment: 1,
        })
    }
}
impl VectorState {
    pub(crate) fn execute_shift(
        &mut self,
        instruction: ShiftInstruction,
        operand: &[u8],
    ) -> Result<(), ShiftError> {
        if operand.len() != instruction.memory().map_or(0, |r| usize::from(r.bytes)) {
            return Err(ShiftError::OperandLength);
        }
        if self.simd.mxcsr & !0xffff != 0 {
            return Err(ShiftError::InvalidState);
        }
        let source = self.read(instruction.source);
        let mut count = [0; 32];
        let immediate = match instruction.count {
            ShiftCount::Immediate(n) => Some(n),
            ShiftCount::Register(r) => {
                count[..16].copy_from_slice(&self.simd.xmm[r.index()]);
                None
            }
            ShiftCount::Memory => {
                count[..16].copy_from_slice(operand);
                None
            }
            ShiftCount::Elements(MoveSource::Register(r)) => {
                count = self.read(r);
                None
            }
            ShiftCount::Elements(MoveSource::Memory) => {
                count[..operand.len()].copy_from_slice(operand);
                None
            }
        };
        let mut value = [0; 32];
        for offset in (0..instruction.width.bytes()).step_by(16) {
            let source = source[offset..offset + 16].try_into().unwrap();
            let result = if matches!(instruction.count, ShiftCount::Elements(_)) {
                instruction
                    .operation
                    .evaluate_elements(source, count[offset..offset + 16].try_into().unwrap())
            } else {
                instruction
                    .operation
                    .evaluate(source, count[..16].try_into().unwrap(), immediate)
            };
            value[offset..offset + 16].copy_from_slice(&result);
        }
        self.publish(instruction.destination, value);
        Ok(())
    }
}
#[cfg(test)]
#[path = "shift/native.rs"]
mod tests;

#[cfg(test)]
#[path = "shift/uniform/native.rs"]
mod uniform;

#[cfg(test)]
#[path = "shift/variable/native.rs"]
mod variable;
