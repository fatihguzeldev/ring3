mod arithmetic;
mod moves;
mod permute;

pub(crate) use moves::{
    DuplicateKind, Half, HalfMerge, MoveInstruction, MoveSource, WordSource, WordTarget,
};
pub(crate) use permute::PermuteInstruction;

use super::simd::SimdState;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct VectorRegister(u8);

impl VectorRegister {
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
        second: VectorRegister,
    },
    PackedUnary {
        width: VectorWidth,
        element: Element,
        source: VectorRegister,
    },
    Scalar {
        element: Element,
        first: VectorRegister,
        second: VectorRegister,
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
    pub second: VectorRegister,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Instruction {
    Floating(FloatInstruction),
    Logical(LogicalInstruction),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ArithmeticError {
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

    pub(crate) fn execute(&mut self, instruction: Instruction) -> Result<(), ArithmeticError> {
        if self.simd.mxcsr & !0xffff != 0 {
            return Err(ArithmeticError::InvalidState);
        }
        match instruction {
            Instruction::Floating(operation) => {
                let (value, exceptions) = arithmetic::floating(operation, self)?;
                self.simd.mxcsr |= exceptions;
                if exceptions & !(self.simd.mxcsr >> 7) & 0x3f != 0 {
                    return Err(ArithmeticError::FloatingPoint(self.simd.mxcsr));
                }
                self.publish(operation.destination, value);
            }
            Instruction::Logical(operation) => {
                let first = self.read(operation.first);
                let second = self.read(operation.second);
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
