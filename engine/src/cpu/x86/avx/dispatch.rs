use super::{ArithmeticError, AvxInstruction, AvxSemantic, Instruction, VectorState};
use crate::cpu::x86::State32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DispatchError {
    InvalidState,
    GeneralProtection,
    FloatingPoint(u32),
}

fn arithmetic(error: ArithmeticError) -> DispatchError {
    match error {
        ArithmeticError::FloatingPoint(mxcsr) => DispatchError::FloatingPoint(mxcsr),
        _ => DispatchError::InvalidState,
    }
}

pub(crate) fn execute(
    instruction: AvxInstruction,
    vectors: &mut VectorState,
    general: &mut State32,
    address: u32,
    operand: &mut [u8],
) -> Result<(), DispatchError> {
    use AvxSemantic::*;
    match instruction.semantic() {
        Floating(value) => vectors
            .execute_with_operand(Instruction::Floating(value), operand)
            .map_err(arithmetic),
        Logical(value) => vectors
            .execute_with_operand(Instruction::Logical(value), operand)
            .map_err(arithmetic),
        Move(value) => vectors
            .execute_move(value, general, address, operand)
            .map_err(|error| match error {
                super::moves::MoveError::Alignment | super::moves::MoveError::InvalidControl => {
                    DispatchError::GeneralProtection
                }
                _ => DispatchError::InvalidState,
            }),
        Permute(value) => vectors
            .execute_permute(value, general, operand)
            .map_err(|_| DispatchError::InvalidState),
        Integer(value) => vectors
            .execute_integer(value, operand)
            .map_err(|_| DispatchError::InvalidState),
        Rearrange(value) => vectors
            .execute_rearrange(value, operand)
            .map_err(|_| DispatchError::InvalidState),
        Shift(value) => vectors
            .execute_shift(value, operand)
            .map_err(|_| DispatchError::InvalidState),
        Multiply(value) => vectors
            .execute_multiply(value, operand)
            .map_err(|_| DispatchError::InvalidState),
        Scalar(value) => vectors
            .execute_scalar(value, general, operand)
            .map_err(|_| DispatchError::InvalidState),
        Compare(value) => vectors
            .execute_compare(value, general, operand)
            .map_err(arithmetic),
        Convert(value) => vectors
            .execute_convert(value, general, operand)
            .map_err(arithmetic),
        Compound(value) => vectors.execute_compound(value, operand).map_err(arithmetic),
        CrossLane(value) => vectors
            .execute_cross_lane(value, operand)
            .map_err(arithmetic),
        Test(value) => vectors
            .execute_test(value, general, operand)
            .map_err(arithmetic),
        Crypto(value) => vectors.execute_crypto(value, operand).map_err(arithmetic),
        String(value) => vectors
            .execute_string(value, general, operand)
            .map_err(arithmetic),
        // selected memory has one authority/transaction owner in process.
        Masked(_) => Err(DispatchError::InvalidState),
    }
}
