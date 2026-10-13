use super::{
    ArithmeticError, Element, MemoryRequirement, MoveSource, VectorRegister, VectorState,
    VectorWidth,
};
use crate::{
    cpu::x86::simd::{PairwiseKind, dot_product_lanes, pairwise_lanes, round_lanes},
    memory::Access,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CompoundInstruction {
    Pairwise {
        kind: PairwiseKind,
        width: VectorWidth,
        element: Element,
        destination: VectorRegister,
        first: VectorRegister,
        second: MoveSource,
    },
    RoundPacked {
        width: VectorWidth,
        element: Element,
        destination: VectorRegister,
        source: MoveSource,
        immediate: u8,
    },
    RoundScalar {
        element: Element,
        destination: VectorRegister,
        merge: VectorRegister,
        source: MoveSource,
        immediate: u8,
    },
    DotSingle {
        width: VectorWidth,
        destination: VectorRegister,
        first: VectorRegister,
        second: MoveSource,
        immediate: u8,
    },
    DotDouble {
        destination: VectorRegister,
        first: VectorRegister,
        second: MoveSource,
        immediate: u8,
    },
}

impl CompoundInstruction {
    fn operands(self) -> (Option<VectorRegister>, MoveSource) {
        match self {
            Self::Pairwise { first, second, .. }
            | Self::DotSingle { first, second, .. }
            | Self::DotDouble { first, second, .. } => (Some(first), second),
            Self::RoundPacked { source, .. } => (None, source),
            Self::RoundScalar { merge, source, .. } => (Some(merge), source),
        }
    }

    pub(crate) fn memory(self) -> Option<MemoryRequirement> {
        if self.operands().1 != MoveSource::Memory {
            return None;
        }
        let bytes = match self {
            Self::Pairwise { width, .. }
            | Self::RoundPacked { width, .. }
            | Self::DotSingle { width, .. } => width.bytes(),
            Self::RoundScalar { element, .. } => element.bytes(),
            Self::DotDouble { .. } => 16,
        };
        Some(MemoryRequirement {
            access: Access::Read,
            bytes: bytes as u8,
            alignment: 1,
        })
    }
}

impl VectorState {
    pub(crate) fn execute_compound(
        &mut self,
        instruction: CompoundInstruction,
        operand: &[u8],
    ) -> Result<(), ArithmeticError> {
        if operand.len() != instruction.memory().map_or(0, |r| usize::from(r.bytes)) {
            return Err(ArithmeticError::OperandLength);
        }
        if self.simd.mxcsr & !0xffff != 0 {
            return Err(ArithmeticError::InvalidState);
        }
        let (first, second) = instruction.operands();
        let a = first.map_or([0; 32], |register| self.read(register));
        let b = match second {
            MoveSource::Register(register) => self.read(register),
            MoveSource::Memory => {
                let mut value = [0; 32];
                value[..operand.len()].copy_from_slice(operand);
                value
            }
        };
        let mxcsr = self.simd.mxcsr;
        let (destination, result, exceptions) = match instruction {
            CompoundInstruction::Pairwise {
                kind,
                width,
                element,
                destination,
                ..
            } => {
                let (mut result, exceptions) =
                    pairwise_lanes(kind, element.bytes(), width.bytes(), a, b, mxcsr);
                result[width.bytes()..].fill(0);
                (destination, result, exceptions)
            }
            CompoundInstruction::RoundPacked {
                width,
                element,
                destination,
                immediate,
                ..
            } => {
                let (result, exceptions) = round_lanes(
                    element.bytes(),
                    width.bytes() / element.bytes(),
                    [0; 32],
                    b,
                    mxcsr,
                    immediate,
                );
                (destination, result, exceptions)
            }
            CompoundInstruction::RoundScalar {
                element,
                destination,
                immediate,
                ..
            } => {
                let mut merge = a;
                merge[16..].fill(0);
                let (result, exceptions) =
                    round_lanes(element.bytes(), 1, merge, b, mxcsr, immediate);
                (destination, result, exceptions)
            }
            CompoundInstruction::DotSingle {
                destination,
                immediate,
                ..
            }
            | CompoundInstruction::DotDouble {
                destination,
                immediate,
                ..
            } => {
                let (bytes, element_bytes) =
                    if let CompoundInstruction::DotSingle { width, .. } = instruction {
                        (width.bytes(), 4)
                    } else {
                        (16, 8)
                    };
                let (result, exceptions) =
                    dot_product_lanes(bytes, element_bytes, immediate, a, b, mxcsr);
                (destination, result, exceptions)
            }
        };
        self.simd.mxcsr |= exceptions;
        if exceptions & !(mxcsr >> 7) & 63 != 0 {
            return Err(ArithmeticError::FloatingPoint(self.simd.mxcsr));
        }
        self.publish(destination, result);
        Ok(())
    }
}

#[cfg(test)]
mod native;

#[cfg(test)]
mod wasm;
