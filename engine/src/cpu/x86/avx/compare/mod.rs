use super::{
    ArithmeticError, Element, MemoryRequirement, MoveSource, VectorRegister, VectorState,
    VectorWidth,
};
use crate::{
    cpu::x86::{
        State32,
        simd::{CompareMode, compare_lanes},
    },
    memory::Access,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CompareShape {
    Packed {
        width: VectorWidth,
        element: Element,
    },
    Scalar(Element),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CompareInstruction {
    Vector {
        shape: CompareShape,
        destination: VectorRegister,
        first: VectorRegister,
        second: MoveSource,
        immediate: u8,
    },
    Flags {
        element: Element,
        first: VectorRegister,
        second: MoveSource,
        quiet: bool,
    },
}

impl CompareInstruction {
    pub(crate) fn memory(self) -> Option<MemoryRequirement> {
        let (source, bytes) = match self {
            Self::Vector {
                shape: CompareShape::Packed { width, .. },
                second,
                ..
            } => (second, width.bytes()),
            Self::Vector {
                shape: CompareShape::Scalar(element),
                second,
                ..
            }
            | Self::Flags {
                element, second, ..
            } => (second, element.bytes()),
        };
        (source == MoveSource::Memory).then_some(MemoryRequirement {
            access: Access::Read,
            bytes: bytes as u8,
            alignment: 1,
        })
    }
}

impl VectorState {
    pub(crate) fn execute_compare(
        &mut self,
        instruction: CompareInstruction,
        general: &mut State32,
        operand: &[u8],
    ) -> Result<(), ArithmeticError> {
        if operand.len() != instruction.memory().map_or(0, |r| usize::from(r.bytes)) {
            return Err(ArithmeticError::OperandLength);
        }
        if self.simd.mxcsr & !0xffff != 0 {
            return Err(ArithmeticError::InvalidState);
        }
        let (mode, element, count, first, source, output, scalar) = match instruction {
            CompareInstruction::Vector {
                shape,
                destination,
                first,
                second,
                immediate,
            } => {
                let (element, count, scalar) = match shape {
                    CompareShape::Packed { width, element } => {
                        (element, width.bytes() / element.bytes(), false)
                    }
                    CompareShape::Scalar(element) => (element, 1, true),
                };
                (
                    CompareMode::Predicate(immediate),
                    element,
                    count,
                    first,
                    second,
                    Some(destination),
                    scalar,
                )
            }
            CompareInstruction::Flags {
                element,
                first,
                second,
                quiet,
            } => (
                CompareMode::Flags { quiet },
                element,
                1,
                first,
                second,
                None,
                false,
            ),
        };
        let a = self.read(first);
        let b = match source {
            MoveSource::Register(register) => self.read(register),
            MoveSource::Memory => {
                let mut value = [0; 32];
                value[..operand.len()].copy_from_slice(operand);
                value
            }
        };
        let (computed, exceptions, flags) =
            compare_lanes(mode, element.bytes(), count, a, b, self.simd.mxcsr);
        self.simd.mxcsr |= exceptions;
        if exceptions & !(self.simd.mxcsr >> 7) & 63 != 0 {
            return Err(ArithmeticError::FloatingPoint(self.simd.mxcsr));
        }
        if let Some(destination) = output {
            let mut value = if scalar { a } else { [0; 32] };
            let written = count * element.bytes();
            value[..written].copy_from_slice(&computed[..written]);
            if scalar {
                value[16..].fill(0);
            }
            self.publish(destination, value);
        } else {
            general.eflags = (general.eflags & !0x8d5) | flags.unwrap();
        }
        Ok(())
    }
}

#[cfg(test)]
mod native;
