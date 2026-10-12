use iced_x86::{Code, EncodingKind, Instruction, OpKind};

use super::{
    DecodeError,
    operands::{effective_address, unsupported},
};
use crate::cpu::x86::xstate::XStateOperation;

pub(super) fn lower(instruction: &Instruction) -> Option<Result<XStateOperation, DecodeError>> {
    let operation = match instruction.code() {
        Code::Xsave_mem => 0,
        Code::Xrstor_mem => 1,
        Code::Xgetbv => 2,
        _ => return None,
    };
    Some((|| {
        if instruction.encoding() != EncodingKind::Legacy
            || instruction.has_lock_prefix()
            || instruction.has_rep_prefix()
            || instruction.has_repne_prefix()
        {
            return Err(unsupported());
        }
        if operation == 2 {
            return Ok(XStateOperation::ReadControl);
        }
        if instruction.op0_kind() != OpKind::Memory {
            return Err(unsupported());
        }
        let address = effective_address(instruction)?;
        Ok(if operation == 0 {
            XStateOperation::Save { address }
        } else {
            XStateOperation::Restore { address }
        })
    })())
}
