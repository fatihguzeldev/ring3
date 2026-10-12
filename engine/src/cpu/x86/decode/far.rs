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
        Code::Jmp_ptr1616 | Code::Jmp_m1616 => FarWidth::Word,
        Code::Jmp_ptr1632 | Code::Jmp_m1632 => FarWidth::Dword,
        _ => return None,
    };
    if instruction.has_lock_prefix()
        || instruction.has_rep_prefix()
        || instruction.has_repne_prefix()
    {
        return Some(Err(unsupported()));
    }
    Some((|| {
        let target = match instruction.code() {
            Code::Jmp_ptr1616 => FarPointer::Immediate {
                offset: u32::from(instruction.far_branch16()),
                selector: instruction.far_branch_selector(),
            },
            Code::Jmp_ptr1632 => FarPointer::Immediate {
                offset: instruction.far_branch32(),
                selector: instruction.far_branch_selector(),
            },
            _ => FarPointer::Memory(effective_address(instruction)?),
        };
        Ok(Operation::FarJump { width, target })
    })())
}
