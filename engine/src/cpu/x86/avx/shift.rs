use super::{MemoryRequirement, VectorRegister, VectorState, VectorWidth};
use crate::{cpu::x86::simd::ShiftOperation, memory::Access};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ShiftCount {
    Immediate(u8),
    Register(VectorRegister),
    Memory,
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
        (operation.byte_kind().is_none() || matches!(count, ShiftCount::Immediate(_))).then_some(
            Self {
                width,
                operation,
                destination,
                source,
                count,
            },
        )
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
        (self.count == ShiftCount::Memory).then_some(MemoryRequirement {
            access: Access::Read,
            bytes: 16,
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
        let (count, immediate) = match instruction.count {
            ShiftCount::Immediate(n) => ([0; 16], Some(n)),
            ShiftCount::Register(r) => (self.simd.xmm[r.index()], None),
            ShiftCount::Memory => (operand.try_into().unwrap(), None),
        };
        let mut value = [0; 32];
        for offset in (0..instruction.width.bytes()).step_by(16) {
            value[offset..offset + 16].copy_from_slice(&instruction.operation.evaluate(
                source[offset..offset + 16].try_into().unwrap(),
                count,
                immediate,
            ));
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
