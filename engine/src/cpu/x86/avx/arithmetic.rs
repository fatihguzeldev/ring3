use super::{ArithmeticError, FloatForm, FloatInstruction, VectorState};
use crate::cpu::x86::simd::elementwise;

pub(super) fn floating(
    operation: FloatInstruction,
    state: &VectorState,
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
            state.read(second),
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
            state.read(source),
            false,
        ),
        FloatForm::Scalar {
            element,
            first,
            second,
        } => (element, 1, state.read(first), state.read(second), true),
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
