use iced_x86::{Code, Instruction, OpKind, Register};

use super::{
    DecodeError,
    operands::{effective_address, register, unsupported, word_register},
};
use crate::cpu::x86::{
    SegmentRegister,
    ir::{Location32, Operation, Value32},
};

pub(super) fn lower(instruction: &Instruction) -> Option<Result<Operation, DecodeError>> {
    if matches!(
        instruction.code(),
        Code::Verr_rm16 | Code::Verr_r32m16 | Code::Verw_rm16 | Code::Verw_r32m16
    ) {
        return Some(verify(instruction));
    }
    if !matches!(
        instruction.code(),
        Code::Arpl_rm16_r16 | Code::Arpl_r32m16_r32
    ) {
        return None;
    }
    if instruction.has_lock_prefix() {
        return Some(Err(DecodeError::InvalidEncoding));
    }
    if instruction.has_rep_prefix() || instruction.has_repne_prefix() {
        return Some(Err(unsupported()));
    }
    Some((|| {
        let source = selector_register(instruction.op1_register())?;
        let destination = match instruction.op0_kind() {
            OpKind::Register => {
                Location32::Register(selector_register(instruction.op0_register())?)
            }
            OpKind::Memory => {
                let mut address = effective_address(instruction)?;
                // retain the actual default segment as well as explicit overrides for cached authority.
                address.segment = Some(match instruction.memory_segment() {
                    Register::ES => SegmentRegister::Es,
                    Register::CS => SegmentRegister::Cs,
                    Register::SS => SegmentRegister::Ss,
                    Register::DS => SegmentRegister::Ds,
                    Register::FS => SegmentRegister::Fs,
                    Register::GS => SegmentRegister::Gs,
                    _ => return Err(unsupported()),
                });
                Location32::Memory(address)
            }
            _ => return Err(unsupported()),
        };
        Ok(Operation::AdjustPrivilege {
            destination,
            source,
        })
    })())
}

fn verify(instruction: &Instruction) -> Result<Operation, DecodeError> {
    if instruction.has_lock_prefix() {
        return Err(DecodeError::InvalidEncoding);
    }
    if instruction.has_rep_prefix() || instruction.has_repne_prefix() {
        return Err(unsupported());
    }
    let source = match instruction.op0_kind() {
        OpKind::Register => Value32::Register(selector_register(instruction.op0_register())?),
        OpKind::Memory => Value32::Memory(effective_address(instruction)?),
        _ => return Err(unsupported()),
    };
    Ok(Operation::VerifySegment {
        source,
        write: matches!(instruction.code(), Code::Verw_rm16 | Code::Verw_r32m16),
    })
}

fn selector_register(value: Register) -> Result<crate::cpu::x86::Register32, DecodeError> {
    // iced models operand-size32 ARPL with parent register IDs; the architectural RPL update is identical.
    word_register(value).or_else(|_| register(value))
}
