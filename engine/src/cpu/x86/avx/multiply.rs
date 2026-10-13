use super::{MemoryRequirement, MoveSource, VectorRegister, VectorState, VectorWidth};
use crate::{
    cpu::x86::simd::{MultiplyOperation, PackedKind},
    memory::Access,
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct MultiplyInstruction {
    width: VectorWidth,
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
    #[cfg(test)]
    pub(crate) fn new(
        operation: MultiplyOperation,
        destination: VectorRegister,
        first: Option<VectorRegister>,
        second: MoveSource,
    ) -> Option<Self> {
        Self::with_width(VectorWidth::Xmm, operation, destination, first, second)
    }
    pub(crate) fn with_width(
        width: VectorWidth,
        operation: MultiplyOperation,
        destination: VectorRegister,
        first: Option<VectorRegister>,
        second: MoveSource,
    ) -> Option<Self> {
        (operation.unary() == first.is_none()
            && (width == VectorWidth::Xmm
                || !matches!(
                    operation.kind(),
                    PackedKind::MinimumPositionUnsignedWord
                        | PackedKind::MultipleSumAbsoluteByteDifferences
                )))
        .then_some(Self {
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
            bytes: self.width.bytes() as u8,
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
        let a = instruction.first.map_or([0; 32], |r| self.read(r));
        let b = self.capture_source(instruction.second, operand);
        let mut value = [0; 32];
        for offset in (0..instruction.width.bytes()).step_by(16) {
            value[offset..offset + 16].copy_from_slice(&instruction.operation.evaluate(
                a[offset..offset + 16].try_into().unwrap(),
                b[offset..offset + 16].try_into().unwrap(),
            ));
        }
        self.publish(instruction.destination, value);
        Ok(())
    }
}
#[cfg(test)]
#[path = "multiply/native.rs"]
mod tests;

#[cfg(test)]
#[path = "multiply/width/native.rs"]
mod width;
