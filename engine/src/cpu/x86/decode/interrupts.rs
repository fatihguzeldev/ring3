use super::{DecodeError, operands::unsupported};
use crate::cpu::x86::ir::Operation;
use iced_x86::{Code, Instruction};

pub(super) fn lower(
    instruction: &Instruction,
    bytes: &[u8],
    configured: bool,
) -> Option<Result<Operation, DecodeError>> {
    if instruction.code() != Code::Int_imm8 {
        return None;
    }
    // the interrupt gate owns operand size, including a single 66 prefix.
    // immediate payload bytes never enter prefix classification.
    let vector = instruction.immediate8();
    Some(
        if configured && (bytes == [0xcd, vector] || bytes == [0x66, 0xcd, vector]) {
            Ok(Operation::SoftwareInterrupt { vector })
        } else {
            Err(unsupported())
        },
    )
}

#[cfg(test)]
mod tests;
