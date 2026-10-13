use super::{
    ArithmeticError, Element, MemoryRequirement, MoveSource, VectorRegister, VectorState,
    VectorWidth, WordSource,
};
use crate::{
    cpu::x86::{
        Register32, State32,
        simd::{ConvertKind, conversion_lanes},
    },
    memory::Access,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PackedConversion {
    SignedToSingle,
    SignedToDouble,
    SingleToSigned { truncate: bool },
    DoubleToSigned { truncate: bool },
    SingleToDouble,
    DoubleToSingle,
}

impl PackedConversion {
    fn kind(self) -> (ConvertKind, bool) {
        use ConvertKind as C;
        match self {
            Self::SignedToSingle => (C::SignedDwordsToSingles, false),
            Self::SignedToDouble => (C::SignedDwordsToDoubles, false),
            Self::SingleToSigned { truncate } => (C::SinglesToSignedDwords, truncate),
            Self::DoubleToSigned { truncate } => (C::DoublesToSignedDwords, truncate),
            Self::SingleToDouble => (C::SinglesToDoubles, false),
            Self::DoubleToSingle => (C::DoublesToSingles, false),
        }
    }

    fn widths(self) -> (usize, usize) {
        match self {
            Self::SignedToDouble | Self::SingleToDouble => (4, 8),
            Self::DoubleToSigned { .. } | Self::DoubleToSingle => (8, 4),
            _ => (4, 4),
        }
    }

    fn count(self, width: VectorWidth) -> usize {
        let (source, destination) = self.widths();
        width.bytes() / source.max(destination)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ConvertInstruction {
    Packed {
        kind: PackedConversion,
        width: VectorWidth,
        destination: VectorRegister,
        source: MoveSource,
    },
    FromInteger {
        element: Element,
        destination: VectorRegister,
        merge: VectorRegister,
        source: WordSource,
    },
    ToInteger {
        element: Element,
        truncate: bool,
        destination: Register32,
        source: MoveSource,
    },
    Scalar {
        source_element: Element,
        destination: VectorRegister,
        merge: VectorRegister,
        source: MoveSource,
    },
}

impl ConvertInstruction {
    pub(crate) fn memory(self) -> Option<MemoryRequirement> {
        let bytes = match self {
            Self::Packed {
                kind,
                width,
                source: MoveSource::Memory,
                ..
            } => kind.count(width) * kind.widths().0,
            Self::FromInteger {
                source: WordSource::Memory,
                ..
            } => 4,
            Self::ToInteger {
                element,
                source: MoveSource::Memory,
                ..
            } => element.bytes(),
            Self::Scalar {
                source_element,
                source: MoveSource::Memory,
                ..
            } => source_element.bytes(),
            _ => return None,
        };
        Some(MemoryRequirement {
            access: Access::Read,
            bytes: bytes as u8,
            alignment: 1,
        })
    }
}

impl VectorState {
    pub(crate) fn execute_convert(
        &mut self,
        instruction: ConvertInstruction,
        general: &mut State32,
        operand: &[u8],
    ) -> Result<(), ArithmeticError> {
        if operand.len() != instruction.memory().map_or(0, |r| usize::from(r.bytes)) {
            return Err(ArithmeticError::OperandLength);
        }
        if self.simd.mxcsr & !0xffff != 0 {
            return Err(ArithmeticError::InvalidState);
        }
        let input = |source| match source {
            MoveSource::Register(register) => self.read(register),
            MoveSource::Memory => {
                let mut value = [0; 32];
                value[..operand.len()].copy_from_slice(operand);
                value
            }
        };
        let (kind, truncate, count, merge, source, vector_output, gpr_output) = match instruction {
            ConvertInstruction::Packed {
                kind,
                width,
                destination,
                source,
            } => {
                let count = kind.count(width);
                let (kind, truncate) = kind.kind();
                (
                    kind,
                    truncate,
                    count,
                    [0; 32],
                    input(source),
                    Some(destination),
                    None,
                )
            }
            ConvertInstruction::FromInteger {
                element,
                destination,
                merge,
                source,
            } => {
                let raw = match source {
                    WordSource::Register(register) => general.registers[register.index()],
                    WordSource::Memory => u32::from_le_bytes(operand.try_into().unwrap()),
                };
                let mut b = [0; 32];
                b[..4].copy_from_slice(&raw.to_le_bytes());
                let kind = if element == Element::Single {
                    ConvertKind::SignedDwordToSingle
                } else {
                    ConvertKind::SignedDwordToDouble
                };
                let mut a = [0; 32];
                a[..16].copy_from_slice(&self.simd.xmm[merge.index()]);
                (kind, false, 1, a, b, Some(destination), None)
            }
            ConvertInstruction::ToInteger {
                element,
                truncate,
                destination,
                source,
            } => {
                let kind = if element == Element::Single {
                    ConvertKind::SingleToSignedDword
                } else {
                    ConvertKind::DoubleToSignedDword
                };
                (
                    kind,
                    truncate,
                    1,
                    [0; 32],
                    input(source),
                    None,
                    Some(destination),
                )
            }
            ConvertInstruction::Scalar {
                source_element,
                destination,
                merge,
                source,
            } => {
                let kind = if source_element == Element::Single {
                    ConvertKind::SingleToDouble
                } else {
                    ConvertKind::DoubleToSingle
                };
                let mut a = [0; 32];
                a[..16].copy_from_slice(&self.simd.xmm[merge.index()]);
                (kind, false, 1, a, input(source), Some(destination), None)
            }
        };
        let (value, exceptions) =
            conversion_lanes(kind, truncate, count, merge, source, self.simd.mxcsr);
        self.simd.mxcsr |= exceptions;
        if exceptions & !(self.simd.mxcsr >> 7) & 63 != 0 {
            return Err(ArithmeticError::FloatingPoint(self.simd.mxcsr));
        }
        if let Some(destination) = vector_output {
            self.publish(destination, value);
        }
        if let Some(destination) = gpr_output {
            general.registers[destination.index()] =
                u32::from_le_bytes(value[..4].try_into().unwrap());
        }
        Ok(())
    }
}

#[cfg(test)]
mod native;
