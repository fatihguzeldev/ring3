use iced_x86::{Code, Instruction};

use super::{
    DecodeError,
    operands::{byte_register, byte_value, location, register, value},
};
use crate::cpu::x86::ir::{
    BinaryKind, ByteLogicalKind, Operation, ShiftCount, ShiftKind, UnaryKind,
};

pub(super) fn lower(instruction: &Instruction) -> Option<Result<Operation, DecodeError>> {
    let byte_kind = match instruction.code() {
        Code::And_rm8_r8 | Code::And_r8_rm8 | Code::And_AL_imm8 | Code::And_rm8_imm8 => {
            Some(ByteLogicalKind::And)
        }
        Code::Or_rm8_r8 | Code::Or_r8_rm8 | Code::Or_AL_imm8 | Code::Or_rm8_imm8 => {
            Some(ByteLogicalKind::Or)
        }
        Code::Xor_rm8_r8 | Code::Xor_r8_rm8 | Code::Xor_AL_imm8 | Code::Xor_rm8_imm8 => {
            Some(ByteLogicalKind::Xor)
        }
        _ => None,
    };
    if let Some(kind) = byte_kind {
        return Some((|| {
            Ok(Operation::LogicalByte {
                kind,
                destination: byte_register(instruction.op0_register())?,
                source: byte_value(instruction)?,
            })
        })());
    }
    if matches!(
        instruction.code(),
        Code::Test_rm8_r8 | Code::Test_AL_imm8 | Code::Test_rm8_imm8
    ) {
        return Some((|| {
            Ok(Operation::TestByte {
                left: byte_register(instruction.op0_register())?,
                right: byte_value(instruction)?,
            })
        })());
    }
    if matches!(
        instruction.code(),
        Code::Cmp_rm8_r8 | Code::Cmp_r8_rm8 | Code::Cmp_AL_imm8 | Code::Cmp_rm8_imm8
    ) {
        return Some((|| {
            Ok(Operation::CompareByte {
                left: byte_register(instruction.op0_register())?,
                right: byte_value(instruction)?,
            })
        })());
    }
    let kind = match instruction.code() {
        Code::Add_rm32_r32
        | Code::Add_r32_rm32
        | Code::Add_EAX_imm32
        | Code::Add_rm32_imm32
        | Code::Add_rm32_imm8 => BinaryKind::Add,
        Code::Adc_rm32_r32
        | Code::Adc_r32_rm32
        | Code::Adc_EAX_imm32
        | Code::Adc_rm32_imm32
        | Code::Adc_rm32_imm8 => BinaryKind::Adc,
        Code::Sub_rm32_r32
        | Code::Sub_r32_rm32
        | Code::Sub_EAX_imm32
        | Code::Sub_rm32_imm32
        | Code::Sub_rm32_imm8 => BinaryKind::Sub,
        Code::Sbb_rm32_r32
        | Code::Sbb_r32_rm32
        | Code::Sbb_EAX_imm32
        | Code::Sbb_rm32_imm32
        | Code::Sbb_rm32_imm8 => BinaryKind::Sbb,
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
        _ => {
            return lower_multiply(instruction)
                .or_else(|| lower_shift(instruction))
                .or_else(|| lower_unary(instruction));
        }
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

fn lower_shift(instruction: &Instruction) -> Option<Result<Operation, DecodeError>> {
    let (kind, count) = match instruction.code() {
        Code::Shl_rm32_1 => (ShiftKind::Shl, ShiftCount::Immediate(1)),
        Code::Shr_rm32_1 => (ShiftKind::Shr, ShiftCount::Immediate(1)),
        Code::Sar_rm32_1 => (ShiftKind::Sar, ShiftCount::Immediate(1)),
        Code::Shl_rm32_imm8 => (
            ShiftKind::Shl,
            ShiftCount::Immediate(instruction.immediate8()),
        ),
        Code::Shr_rm32_imm8 => (
            ShiftKind::Shr,
            ShiftCount::Immediate(instruction.immediate8()),
        ),
        Code::Sar_rm32_imm8 => (
            ShiftKind::Sar,
            ShiftCount::Immediate(instruction.immediate8()),
        ),
        Code::Shl_rm32_CL => (ShiftKind::Shl, ShiftCount::Cl),
        Code::Shr_rm32_CL => (ShiftKind::Shr, ShiftCount::Cl),
        Code::Sar_rm32_CL => (ShiftKind::Sar, ShiftCount::Cl),
        _ => return None,
    };
    Some(
        location(instruction, 0).map(|destination| Operation::Shift {
            kind,
            destination,
            count,
        }),
    )
}

fn lower_multiply(instruction: &Instruction) -> Option<Result<Operation, DecodeError>> {
    let immediate = match instruction.code() {
        Code::Imul_r32_rm32 => None,
        Code::Imul_r32_rm32_imm32 => Some(instruction.immediate32()),
        Code::Imul_r32_rm32_imm8 => Some(instruction.immediate8to32() as u32),
        _ => return None,
    };
    Some((|| {
        Ok(Operation::SignedMultiply {
            destination: register(instruction.op0_register())?,
            source: location(instruction, 1)?,
            immediate,
        })
    })())
}
