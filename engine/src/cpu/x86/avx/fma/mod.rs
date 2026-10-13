use super::{
    ArithmeticError, Element, MemoryRequirement, MoveSource, VectorRegister, VectorState,
    VectorWidth,
};
use crate::{cpu::x86::simd::fused_lanes, memory::Access};

pub(crate) use crate::cpu::x86::simd::FusedKind as FmaKind;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FmaOrder {
    Order132,
    Order213,
    Order231,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FmaShape {
    Packed {
        width: VectorWidth,
        element: Element,
    },
    Scalar {
        element: Element,
    },
}

impl FmaShape {
    fn element(self) -> Element {
        match self {
            Self::Packed { element, .. } | Self::Scalar { element } => element,
        }
    }

    fn bytes(self) -> usize {
        match self {
            Self::Packed { width, .. } => width.bytes(),
            Self::Scalar { element } => element.bytes(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FmaInstruction {
    pub kind: FmaKind,
    pub order: FmaOrder,
    pub shape: FmaShape,
    pub destination: VectorRegister,
    pub second: VectorRegister,
    pub third: MoveSource,
}

impl FmaInstruction {
    pub(crate) fn memory(self) -> Option<MemoryRequirement> {
        matches!(self.third, MoveSource::Memory).then_some(MemoryRequirement {
            access: Access::Read,
            bytes: self.shape.bytes() as u8,
            alignment: 1,
        })
    }
}

impl VectorState {
    // guest reads and segment/page checks complete before this value-only kernel.
    pub(crate) fn execute_fma(
        &mut self,
        instruction: FmaInstruction,
        operand: &[u8],
    ) -> Result<(), ArithmeticError> {
        if operand.len() != instruction.memory().map_or(0, |r| usize::from(r.bytes)) {
            return Err(ArithmeticError::OperandLength);
        }
        if matches!(instruction.shape, FmaShape::Scalar { .. }) && instruction.kind.alternating() {
            return Err(ArithmeticError::InvalidOperation);
        }
        if self.simd.mxcsr & !0xffff != 0 {
            return Err(ArithmeticError::InvalidState);
        }
        let destination = self.read(instruction.destination);
        let second = self.read(instruction.second);
        let third = self.capture_source(instruction.third, operand);
        let (a, b, c) = match instruction.order {
            FmaOrder::Order132 => (destination, third, second),
            FmaOrder::Order213 => (second, destination, third),
            FmaOrder::Order231 => (second, third, destination),
        };
        let bytes = instruction.shape.bytes();
        let (mut result, exceptions) = fused_lanes(
            instruction.kind,
            instruction.shape.element().bytes(),
            bytes / instruction.shape.element().bytes(),
            a,
            b,
            c,
            self.simd.mxcsr,
        );
        if matches!(instruction.shape, FmaShape::Scalar { .. }) {
            result[bytes..16].copy_from_slice(&destination[bytes..16]);
        }
        self.simd.mxcsr |= exceptions;
        if exceptions & !(self.simd.mxcsr >> 7) & 0x3f != 0 {
            return Err(ArithmeticError::FloatingPoint(self.simd.mxcsr));
        }
        self.publish(instruction.destination, result);
        Ok(())
    }
}

#[cfg(test)]
mod native;
