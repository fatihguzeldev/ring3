use super::{
    ArithmeticError, FloatForm, FloatInstruction, MemoryRequirement, MoveSource, VectorState,
};
use crate::cpu::x86::simd::elementwise;

impl FloatInstruction {
    pub(crate) fn memory(self) -> Option<MemoryRequirement> {
        let bytes = match self.form {
            FloatForm::PackedBinary {
                width,
                second: MoveSource::Memory,
                ..
            }
            | FloatForm::PackedUnary {
                width,
                source: MoveSource::Memory,
                ..
            } => width.bytes(),
            FloatForm::Scalar {
                element,
                second: MoveSource::Memory,
                ..
            } => element.bytes(),
            _ => return None,
        };
        Some(MemoryRequirement {
            access: crate::memory::Access::Read,
            bytes: bytes as u8,
            alignment: 1,
        })
    }
}

pub(super) fn floating(
    operation: FloatInstruction,
    state: &VectorState,
    operand: &[u8],
) -> Result<([u8; 32], u32), ArithmeticError> {
    let (element, count, first, second, scalar) = match operation.form {
        FloatForm::PackedBinary {
            width,
            element,
            first,
            second,
        } if !operation.kind.unary() => (
            element,
            width.bytes() / element.bytes(),
            state.read(first),
            state.capture_source(second, operand),
            false,
        ),
        FloatForm::PackedUnary {
            width,
            element,
            source,
        } if operation.kind.unary() => (
            element,
            width.bytes() / element.bytes(),
            [0; 32],
            state.capture_source(source, operand),
            false,
        ),
        FloatForm::Scalar {
            element,
            first,
            second,
        } => (
            element,
            1,
            state.read(first),
            state.capture_source(second, operand),
            true,
        ),
        _ => return Err(ArithmeticError::InvalidOperation),
    };
    if operation.kind.approximate() && element != super::Element::Single {
        return Err(ArithmeticError::InvalidOperation);
    }
    let (computed, exceptions) = elementwise(
        operation.kind,
        element.bytes(),
        count,
        first,
        second,
        state.simd.mxcsr,
    );
    let mut value = [0; 32];
    let written = if scalar { 16 } else { count * element.bytes() };
    value[..written].copy_from_slice(&computed[..written]);
    Ok((value, exceptions))
}

#[cfg(test)]
mod tests;

#[cfg(test)]
#[path = "arithmetic/memory/native.rs"]
mod memory;

#[cfg(test)]
#[path = "arithmetic/memory/wasm.rs"]
mod wasm;
