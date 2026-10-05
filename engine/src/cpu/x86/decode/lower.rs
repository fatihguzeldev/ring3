use iced_x86::{Code, Instruction, OpKind};

use super::{
    DecodeError,
    operands::{
        byte_register, byte_value, effective_address, location, register, small_source,
        unsupported, value,
    },
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
        Code::Mov_r8_rm8 | Code::Mov_AL_moffs8 => match instruction.op1_kind() {
            OpKind::Memory => Ok(Operation::LoadByte {
                destination: byte_register(instruction.op0_register())?,
                address: effective_address(instruction)?,
            }),
            OpKind::Register => Ok(Operation::MoveByte {
                destination: byte_register(instruction.op0_register())?,
                source: byte_value(instruction)?,
            }),
            _ => Err(unsupported()),
        },
        Code::Mov_rm8_r8 | Code::Mov_moffs8_AL => match instruction.op0_kind() {
            OpKind::Memory => Ok(Operation::StoreByte {
                address: effective_address(instruction)?,
                source: byte_value(instruction)?,
            }),
            OpKind::Register => Ok(Operation::MoveByte {
                destination: byte_register(instruction.op0_register())?,
                source: byte_value(instruction)?,
            }),
            _ => Err(unsupported()),
        },
        Code::Mov_r8_imm8 => Ok(Operation::MoveByte {
            destination: byte_register(instruction.op0_register())?,
            source: byte_value(instruction)?,
        }),
        Code::Mov_rm8_imm8 => {
            if instruction.op0_kind() != OpKind::Memory {
                return Err(unsupported());
            }
            Ok(Operation::StoreByte {
                address: effective_address(instruction)?,
                source: byte_value(instruction)?,
            })
        }
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
