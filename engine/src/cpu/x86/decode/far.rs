use super::{
    DecodeError,
    operands::{effective_address, unsupported},
};
use crate::cpu::x86::{
    FarWidth,
    ir::{FarPointer, Operation},
};
use iced_x86::{Code, Instruction};

pub(super) fn lower(instruction: &Instruction) -> Option<Result<Operation, DecodeError>> {
    let width = match instruction.code() {
        Code::Jmp_ptr1616
        | Code::Jmp_m1616
        | Code::Call_ptr1616
        | Code::Call_m1616
        | Code::Retfw
        | Code::Retfw_imm16
        | Code::Iretw => FarWidth::Word,
        Code::Jmp_ptr1632
        | Code::Jmp_m1632
        | Code::Call_ptr1632
        | Code::Call_m1632
        | Code::Retfd
        | Code::Retfd_imm16
        | Code::Iretd => FarWidth::Dword,
        _ => return None,
    };
    if instruction.has_lock_prefix()
        || instruction.has_rep_prefix()
        || instruction.has_repne_prefix()
    {
        return Some(Err(unsupported()));
    }
    Some((|| {
        if matches!(instruction.code(), Code::Iretw | Code::Iretd) {
            return Ok(Operation::InterruptReturn { width });
        }
        if matches!(
            instruction.code(),
            Code::Retfw | Code::Retfd | Code::Retfw_imm16 | Code::Retfd_imm16
        ) {
            let stack_adjust =
                if matches!(instruction.code(), Code::Retfw_imm16 | Code::Retfd_imm16) {
                    instruction.immediate16()
                } else {
                    0
                };
            return Ok(Operation::FarReturn {
                width,
                stack_adjust,
            });
        }
        let target = match instruction.code() {
            Code::Jmp_ptr1616 | Code::Call_ptr1616 => FarPointer::Immediate {
                offset: u32::from(instruction.far_branch16()),
                selector: instruction.far_branch_selector(),
            },
            Code::Jmp_ptr1632 | Code::Call_ptr1632 => FarPointer::Immediate {
                offset: instruction.far_branch32(),
                selector: instruction.far_branch_selector(),
            },
            _ => FarPointer::Memory(effective_address(instruction)?),
        };
        Ok(
            if matches!(
                instruction.code(),
                Code::Call_ptr1616 | Code::Call_ptr1632 | Code::Call_m1616 | Code::Call_m1632
            ) {
                Operation::FarCall { width, target }
            } else {
                Operation::FarJump { width, target }
            },
        )
    })())
}
