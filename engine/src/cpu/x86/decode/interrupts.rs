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
    // gate width comes from the configured payload. Additional raw prefixes have no
    // selected contract; immediate payload bytes never enter prefix classification.
    Some(if configured && bytes == [0xcd, instruction.immediate8()] {
        Ok(Operation::SoftwareInterrupt {
            vector: instruction.immediate8(),
        })
    } else {
        Err(unsupported())
    })
}

#[cfg(test)]
mod tests;
