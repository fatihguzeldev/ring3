use super::{DecodeError, operands::unsupported};
use crate::cpu::x86::{ControlOperation, ir::Operation};
use iced_x86::{Code, Instruction};

pub(super) fn lower(
    instruction: &Instruction,
    bytes: &[u8],
) -> Option<Result<Operation, DecodeError>> {
    if instruction.code() != Code::Int_imm8 {
        return None;
    }
    // immediate payload bytes never enter prefix classification. the application
    // os boundary owns vector admission; decoding does not require hardware idt state.
    let vector = instruction.immediate8();
    Some(
        if bytes == [0xcd, vector] || bytes == [0x66, 0xcd, vector] {
            Ok(if vector == 3 {
                Operation::Control(ControlOperation::Breakpoint)
            } else {
                Operation::SoftwareInterrupt { vector }
            })
        } else {
            Err(unsupported())
        },
    )
}

#[cfg(test)]
mod tests;
