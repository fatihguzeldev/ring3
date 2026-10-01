use iced_x86::{Code, Instruction};

use super::{
    DecodeError,
    operands::{effective_address, location, register, small_source, value},
    profile::check_profile,
};
use crate::cpu::x86::ir::{ExtensionKind, Operation, SmallWidth};

pub(super) fn lower(instruction: &Instruction, bytes: &[u8]) -> Result<Operation, DecodeError> {
    check_profile(instruction, bytes)?;
    match instruction.code() {
        Code::Nopd => Ok(Operation::Nop),
        Code::Nop_rm32 => {
            location(instruction, 0)?;
            Ok(Operation::Nop)
        }
        Code::Mov_rm32_r32
        | Code::Mov_r32_rm32
        | Code::Mov_r32_imm32
        | Code::Mov_rm32_imm32
        | Code::Mov_EAX_moffs32
        | Code::Mov_moffs32_EAX => Ok(Operation::Move {
            destination: location(instruction, 0)?,
            source: value(instruction, 1)?,
        }),
        Code::Movzx_r32_rm8 | Code::Movsx_r32_rm8 | Code::Movzx_r32_rm16 | Code::Movsx_r32_rm16 => {
            let kind = match instruction.code() {
                Code::Movzx_r32_rm8 | Code::Movzx_r32_rm16 => ExtensionKind::Zero,
                _ => ExtensionKind::Sign,
            };
            let width = match instruction.code() {
                Code::Movzx_r32_rm8 | Code::Movsx_r32_rm8 => SmallWidth::Byte,
                _ => SmallWidth::Word,
            };
            Ok(Operation::Extend {
                kind,
                destination: register(instruction.op0_register())?,
                source: small_source(instruction, width)?,
            })
        }
        Code::Lea_r32_m => Ok(Operation::Lea {
            destination: register(instruction.op0_register())?,
            address: effective_address(instruction)?,
        }),
        _ => super::integer::lower(instruction).unwrap_or_else(|| super::flow::lower(instruction)),
    }
}
