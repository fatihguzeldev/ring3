pub(crate) mod dispatch;
mod operation;
#[cfg(test)]
pub(crate) use operation::AvxDecodeError;
pub use operation::AvxOperation;
pub(crate) use operation::{AvxInstruction, AvxSemantic, MemoryEffect};
mod arithmetic;
mod compare;
mod compound;
mod convert;
mod cross_lane;
mod crypto;
#[cfg(test)]
mod f16c;
mod integer;
#[cfg(test)]
mod logical;
mod masked;
mod moves;
mod multiply;
mod permute;
mod predicates;
mod rearrange;
mod scalar;
mod shift;
mod strings;

pub(crate) use super::simd::BitTestKind;
pub(crate) use compare::{CompareInstruction, CompareShape};
pub(crate) use compound::CompoundInstruction;
pub(crate) use convert::{ConvertInstruction, PackedConversion};
pub(crate) use cross_lane::{BlockTarget, CrossLaneInstruction};
pub(crate) use crypto::CryptoInstruction;
pub(crate) use integer::IntegerInstruction;
pub(crate) use masked::{MaskedError, MaskedInstruction};
pub(crate) use moves::{
    DuplicateKind, Half, HalfMerge, MoveInstruction, MoveSource, WordSource, WordTarget,
};
pub(crate) use multiply::MultiplyInstruction;
pub(crate) use permute::PermuteInstruction;
pub(crate) use predicates::TestInstruction;
pub(crate) use rearrange::RearrangeInstruction;
pub(crate) use scalar::ScalarInstruction;
pub(crate) use shift::{ShiftCount, ShiftInstruction};
pub(crate) use strings::StringInstruction;

use super::simd::SimdState;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct VectorRegister(u8);

impl VectorRegister {
    #[cfg(test)]
    pub(crate) fn new(index: u8) -> Result<Self, ArithmeticError> {
        if index < 8 {
            Ok(Self(index))
        } else {
            Err(ArithmeticError::InvalidRegister)
        }
    }

    pub(crate) fn index(self) -> usize {
        usize::from(self.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum VectorWidth {
    Xmm,
    Ymm,
}

impl VectorWidth {
    pub(crate) fn bytes(self) -> usize {
        match self {
            Self::Xmm => 16,
            Self::Ymm => 32,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct MemoryRequirement {
    pub access: crate::memory::Access,
    pub bytes: u8,
    pub alignment: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Element {
    Single,
    Double,
}

impl Element {
    pub(crate) fn bytes(self) -> usize {
        match self {
            Self::Single => 4,
            Self::Double => 8,
        }
    }
}

pub(crate) use super::simd::ElementwiseKind as FloatOperation;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FloatForm {
    PackedBinary {
        width: VectorWidth,
        element: Element,
        first: VectorRegister,
        second: MoveSource,
    },
    PackedUnary {
        width: VectorWidth,
        element: Element,
        source: MoveSource,
    },
    Scalar {
        element: Element,
        first: VectorRegister,
        second: MoveSource,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FloatInstruction {
    pub kind: FloatOperation,
    pub destination: VectorRegister,
    pub form: FloatForm,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LogicalOperation {
    And,
    AndNot,
    Or,
    Xor,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LogicalInstruction {
    pub kind: LogicalOperation,
    pub width: VectorWidth,
    pub destination: VectorRegister,
    pub first: VectorRegister,
    pub second: MoveSource,
}

impl LogicalInstruction {
    pub(crate) fn memory(self) -> Option<MemoryRequirement> {
        matches!(self.second, MoveSource::Memory).then_some(MemoryRequirement {
            access: crate::memory::Access::Read,
            bytes: self.width.bytes() as u8,
            alignment: 1,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Instruction {
    Floating(FloatInstruction),
    Logical(LogicalInstruction),
}

impl Instruction {
    pub(crate) fn memory(self) -> Option<MemoryRequirement> {
        match self {
            Self::Floating(operation) => operation.memory(),
            Self::Logical(operation) => operation.memory(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ArithmeticError {
    OperandLength,
    #[cfg(test)]
    InvalidRegister,
    InvalidOperation,
    InvalidState,
    FloatingPoint(u32),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct VectorState {
    pub simd: SimdState,
    pub upper: [[u8; 16]; 8],
}

impl VectorState {
    pub(crate) fn read(&self, register: VectorRegister) -> [u8; 32] {
        let index = register.index();
        let mut value = [0; 32];
        value[..16].copy_from_slice(&self.simd.xmm[index]);
        value[16..].copy_from_slice(&self.upper[index]);
        value
    }

    fn publish(&mut self, register: VectorRegister, value: [u8; 32]) {
        let index = register.index();
        self.simd.xmm[index].copy_from_slice(&value[..16]);
        self.upper[index].copy_from_slice(&value[16..]);
    }

    fn capture_source(&self, source: MoveSource, operand: &[u8]) -> [u8; 32] {
        match source {
            MoveSource::Register(register) => self.read(register),
            MoveSource::Memory => {
                let mut value = [0; 32];
                value[..operand.len()].copy_from_slice(operand);
                value
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn execute(&mut self, instruction: Instruction) -> Result<(), ArithmeticError> {
        self.execute_with_operand(instruction, &[])
    }

    // the caller captures an exact checked guest span before entering this kernel.
    pub(crate) fn execute_with_operand(
        &mut self,
        instruction: Instruction,
        operand: &[u8],
    ) -> Result<(), ArithmeticError> {
        if operand.len() != instruction.memory().map_or(0, |r| usize::from(r.bytes)) {
            return Err(ArithmeticError::OperandLength);
        }
        if self.simd.mxcsr & !0xffff != 0 {
            return Err(ArithmeticError::InvalidState);
        }
        match instruction {
            Instruction::Floating(operation) => {
                let (value, exceptions) = arithmetic::floating(operation, self, operand)?;
                self.simd.mxcsr |= exceptions;
                if exceptions & !(self.simd.mxcsr >> 7) & 0x3f != 0 {
                    return Err(ArithmeticError::FloatingPoint(self.simd.mxcsr));
                }
                self.publish(operation.destination, value);
            }
            Instruction::Logical(operation) => {
                let first = self.read(operation.first);
                let second = self.capture_source(operation.second, operand);
                let mut value = [0; 32];
                for index in 0..operation.width.bytes() {
                    value[index] = match operation.kind {
                        LogicalOperation::And => first[index] & second[index],
                        LogicalOperation::AndNot => !first[index] & second[index],
                        LogicalOperation::Or => first[index] | second[index],
                        LogicalOperation::Xor => first[index] ^ second[index],
                    };
                }
                self.publish(operation.destination, value);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "integration/wasm.rs"]
mod integration;
