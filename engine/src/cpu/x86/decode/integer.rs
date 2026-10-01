use iced_x86::{Code, Instruction};

use super::{
    DecodeError,
    operands::{location, value},
};
use crate::cpu::x86::ir::{BinaryKind, Operation, UnaryKind};

pub(super) fn lower(instruction: &Instruction) -> Option<Result<Operation, DecodeError>> {
    let kind = match instruction.code() {
        Code::Add_rm32_r32
        | Code::Add_r32_rm32
        | Code::Add_EAX_imm32
        | Code::Add_rm32_imm32
        | Code::Add_rm32_imm8 => BinaryKind::Add,
        Code::Sub_rm32_r32
        | Code::Sub_r32_rm32
        | Code::Sub_EAX_imm32
        | Code::Sub_rm32_imm32
        | Code::Sub_rm32_imm8 => BinaryKind::Sub,
        Code::Cmp_rm32_r32
        | Code::Cmp_r32_rm32
        | Code::Cmp_EAX_imm32
        | Code::Cmp_rm32_imm32
        | Code::Cmp_rm32_imm8 => BinaryKind::Cmp,
        Code::Test_rm32_r32 | Code::Test_EAX_imm32 | Code::Test_rm32_imm32 => BinaryKind::Test,
        Code::And_rm32_r32
        | Code::And_r32_rm32
        | Code::And_EAX_imm32
        | Code::And_rm32_imm32
        | Code::And_rm32_imm8 => BinaryKind::And,
        Code::Or_rm32_r32
        | Code::Or_r32_rm32
        | Code::Or_EAX_imm32
        | Code::Or_rm32_imm32
        | Code::Or_rm32_imm8 => BinaryKind::Or,
        Code::Xor_rm32_r32
        | Code::Xor_r32_rm32
        | Code::Xor_EAX_imm32
        | Code::Xor_rm32_imm32
        | Code::Xor_rm32_imm8 => BinaryKind::Xor,
        _ => return lower_unary(instruction),
    };
    Some(binary(instruction, kind))
}

fn binary(instruction: &Instruction, kind: BinaryKind) -> Result<Operation, DecodeError> {
    Ok(Operation::Binary {
        kind,
        destination: location(instruction, 0)?,
        source: value(instruction, 1)?,
    })
}

fn lower_unary(instruction: &Instruction) -> Option<Result<Operation, DecodeError>> {
    let kind = match instruction.code() {
        Code::Inc_r32 | Code::Inc_rm32 => UnaryKind::Inc,
        Code::Dec_r32 | Code::Dec_rm32 => UnaryKind::Dec,
        Code::Not_rm32 => UnaryKind::Not,
        Code::Neg_rm32 => UnaryKind::Neg,
        _ => return None,
    };
    Some(location(instruction, 0).map(|destination| Operation::Unary { kind, destination }))
}
