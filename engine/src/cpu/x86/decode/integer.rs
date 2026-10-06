use iced_x86::{Code, Instruction, OpKind};

use super::{
    DecodeError,
    operands::{
        byte_register, byte_value, effective_address, location, register, unsupported, value,
    },
};
use crate::cpu::x86::ir::{
    BinaryKind, BitScanKind, ByteArithmeticKind, ByteLogicalKind, BytePredicateKind,
    ByteReadArithmeticKind, MemoryByteArithmeticKind, MultiplyKind, Operation, RotateKind,
    ShiftCount, ShiftKind, UnaryKind,
};

pub(super) fn lower(instruction: &Instruction) -> Option<Result<Operation, DecodeError>> {
    let memory_predicate = match instruction.code() {
        Code::Cmp_rm8_r8 | Code::Cmp_rm8_imm8 => Some(BytePredicateKind::Cmp),
        Code::Test_rm8_r8 | Code::Test_rm8_imm8 => Some(BytePredicateKind::Test),
        _ => None,
    };
    if instruction.op0_kind() == OpKind::Memory
        && let Some(kind) = memory_predicate
    {
        return Some((|| {
            Ok(Operation::MemoryPredicateByte {
                kind,
                address: effective_address(instruction)?,
                right: byte_value(instruction)?,
            })
        })());
    }
    let memory_arithmetic = match instruction.code() {
        Code::Add_rm8_r8 | Code::Add_rm8_imm8 => Some(MemoryByteArithmeticKind::Add),
        Code::Adc_rm8_r8 | Code::Adc_rm8_imm8 => Some(MemoryByteArithmeticKind::Adc),
        Code::Sub_rm8_r8 | Code::Sub_rm8_imm8 => Some(MemoryByteArithmeticKind::Sub),
        Code::Sbb_rm8_r8 | Code::Sbb_rm8_imm8 => Some(MemoryByteArithmeticKind::Sbb),
        _ => None,
    };
    if instruction.op0_kind() == OpKind::Memory
        && let Some(kind) = memory_arithmetic
    {
        return Some((|| {
            Ok(Operation::MemoryArithmeticByte {
                kind,
                address: effective_address(instruction)?,
                source: byte_value(instruction)?,
            })
        })());
    }
    let read_arithmetic = match instruction.code() {
        Code::Add_r8_rm8 => Some(ByteReadArithmeticKind::Add),
        Code::Adc_r8_rm8 => Some(ByteReadArithmeticKind::Adc),
        Code::Sub_r8_rm8 => Some(ByteReadArithmeticKind::Sub),
        Code::Sbb_r8_rm8 => Some(ByteReadArithmeticKind::Sbb),
        _ => None,
    };
    if instruction.op1_kind() == OpKind::Memory
        && let Some(kind) = read_arithmetic
    {
        return Some((|| {
            Ok(Operation::ReadArithmeticByte {
                kind,
                destination: byte_register(instruction.op0_register())?,
                address: effective_address(instruction)?,
            })
        })());
    }
    let arithmetic_kind = match instruction.code() {
        Code::Add_rm8_r8 | Code::Add_r8_rm8 | Code::Add_AL_imm8 | Code::Add_rm8_imm8 => {
            Some(ByteArithmeticKind::Add)
        }
        Code::Adc_rm8_r8 | Code::Adc_r8_rm8 | Code::Adc_AL_imm8 | Code::Adc_rm8_imm8 => {
            Some(ByteArithmeticKind::Adc)
        }
        Code::Sub_rm8_r8 | Code::Sub_r8_rm8 | Code::Sub_AL_imm8 | Code::Sub_rm8_imm8 => {
            Some(ByteArithmeticKind::Sub)
        }
        Code::Sbb_rm8_r8 | Code::Sbb_r8_rm8 | Code::Sbb_AL_imm8 | Code::Sbb_rm8_imm8 => {
            Some(ByteArithmeticKind::Sbb)
        }
        _ => None,
    };
    if let Some(kind) = arithmetic_kind {
        return Some((|| {
            Ok(Operation::ArithmeticByte {
                kind,
                destination: byte_register(instruction.op0_register())?,
                source: byte_value(instruction)?,
            })
        })());
    }
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
            if instruction.op0_kind() == OpKind::Memory {
                return Ok(Operation::MemoryLogicalByte {
                    kind,
                    address: effective_address(instruction)?,
                    source: byte_value(instruction)?,
                });
            }
            if instruction.op1_kind() == OpKind::Memory {
                return Ok(Operation::ReadLogicalByte {
                    kind,
                    destination: byte_register(instruction.op0_register())?,
                    address: effective_address(instruction)?,
                });
            }
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
            if instruction.code() == Code::Cmp_r8_rm8 && instruction.op1_kind() == OpKind::Memory {
                return Ok(Operation::ReadCompareByte {
                    left: byte_register(instruction.op0_register())?,
                    address: effective_address(instruction)?,
                });
            }
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
                .or_else(|| lower_rotate_one(instruction))
                .or_else(|| lower_rotate_through_carry_one(instruction))
                .or_else(|| lower_unary(instruction))
                .or_else(|| lower_bit_scan(instruction));
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
    let byte_kind = match instruction.code() {
        Code::Inc_rm8 => Some(UnaryKind::Inc),
        Code::Dec_rm8 => Some(UnaryKind::Dec),
        Code::Not_rm8 => Some(UnaryKind::Not),
        Code::Neg_rm8 => Some(UnaryKind::Neg),
        _ => None,
    };
    if let Some(kind) = byte_kind {
        if instruction.op0_kind() == OpKind::Memory {
            return Some(
                effective_address(instruction)
                    .map(|address| Operation::MemoryUnaryByte { kind, address }),
            );
        }
        return Some(
            byte_register(instruction.op0_register())
                .map(|destination| Operation::UnaryByte { kind, destination }),
        );
    }
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
    let byte_shift = match instruction.code() {
        Code::Shl_rm8_1 => Some((ShiftKind::Shl, ShiftCount::Immediate(1))),
        Code::Shr_rm8_1 => Some((ShiftKind::Shr, ShiftCount::Immediate(1))),
        Code::Sar_rm8_1 => Some((ShiftKind::Sar, ShiftCount::Immediate(1))),
        Code::Shl_rm8_imm8 => Some((
            ShiftKind::Shl,
            ShiftCount::Immediate(instruction.immediate8()),
        )),
        Code::Shr_rm8_imm8 => Some((
            ShiftKind::Shr,
            ShiftCount::Immediate(instruction.immediate8()),
        )),
        Code::Sar_rm8_imm8 => Some((
            ShiftKind::Sar,
            ShiftCount::Immediate(instruction.immediate8()),
        )),
        Code::Shl_rm8_CL => Some((ShiftKind::Shl, ShiftCount::Cl)),
        Code::Shr_rm8_CL => Some((ShiftKind::Shr, ShiftCount::Cl)),
        Code::Sar_rm8_CL => Some((ShiftKind::Sar, ShiftCount::Cl)),
        _ => None,
    };
    if let Some((kind, count)) = byte_shift {
        if instruction.op0_kind() == OpKind::Memory
            && matches!(count, ShiftCount::Immediate(raw) if raw & 31 == 1)
        {
            return Some(
                effective_address(instruction)
                    .map(|address| Operation::MemoryShiftByte { kind, address }),
            );
        }
        return Some(
            byte_register(instruction.op0_register()).map(|destination| Operation::ShiftByte {
                kind,
                destination,
                count,
            }),
        );
    }
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

fn lower_rotate_one(instruction: &Instruction) -> Option<Result<Operation, DecodeError>> {
    let byte_kind = match instruction.code() {
        Code::Rol_rm8_1 => Some(RotateKind::Left),
        Code::Ror_rm8_1 => Some(RotateKind::Right),
        _ => None,
    };
    if let Some(kind) = byte_kind {
        return Some(match instruction.op0_kind() {
            OpKind::Register => byte_register(instruction.op0_register())
                .map(|destination| Operation::ByteRotateOne { kind, destination }),
            OpKind::Memory => effective_address(instruction)
                .map(|address| Operation::MemoryByteRotateOne { kind, address }),
            _ => Err(unsupported()),
        });
    }
    let kind = match instruction.code() {
        Code::Rol_rm32_1 => RotateKind::Left,
        Code::Ror_rm32_1 => RotateKind::Right,
        Code::Rol_rm32_imm8 if instruction.immediate8() & 31 == 1 => RotateKind::Left,
        Code::Ror_rm32_imm8 if instruction.immediate8() & 31 == 1 => RotateKind::Right,
        _ => return None,
    };
    Some(match instruction.op0_kind() {
        OpKind::Register => register(instruction.op0_register())
            .map(|destination| Operation::RotateOne { kind, destination }),
        OpKind::Memory => effective_address(instruction)
            .map(|address| Operation::MemoryRotateOne { kind, address }),
        _ => Err(unsupported()),
    })
}

fn lower_rotate_through_carry_one(
    instruction: &Instruction,
) -> Option<Result<Operation, DecodeError>> {
    let kind = match instruction.code() {
        Code::Rcl_rm32_1 => RotateKind::Left,
        Code::Rcr_rm32_1 => RotateKind::Right,
        Code::Rcl_rm32_imm8 if instruction.immediate8() & 31 == 1 => RotateKind::Left,
        Code::Rcr_rm32_imm8 if instruction.immediate8() & 31 == 1 => RotateKind::Right,
        _ => return None,
    };
    Some(match instruction.op0_kind() {
        OpKind::Register => register(instruction.op0_register())
            .map(|destination| Operation::RotateThroughCarryOne { kind, destination }),
        OpKind::Memory => effective_address(instruction)
            .map(|address| Operation::MemoryRotateThroughCarryOne { kind, address }),
        _ => Err(unsupported()),
    })
}

fn lower_bit_scan(instruction: &Instruction) -> Option<Result<Operation, DecodeError>> {
    let kind = match instruction.code() {
        Code::Bsf_r32_rm32 => BitScanKind::Forward,
        Code::Bsr_r32_rm32 => BitScanKind::Reverse,
        _ => return None,
    };
    Some((|| {
        let destination = register(instruction.op0_register())?;
        match instruction.op1_kind() {
            OpKind::Register => Ok(Operation::BitScan {
                kind,
                destination,
                source: register(instruction.op1_register())?,
            }),
            OpKind::Memory => Ok(Operation::ReadBitScan {
                kind,
                destination,
                address: effective_address(instruction)?,
            }),
            _ => Err(unsupported()),
        }
    })())
}

fn lower_multiply(instruction: &Instruction) -> Option<Result<Operation, DecodeError>> {
    let kind = match instruction.code() {
        Code::Mul_rm32 => Some(MultiplyKind::Unsigned),
        Code::Imul_rm32 => Some(MultiplyKind::Signed),
        _ => None,
    };
    if let Some(kind) = kind {
        return Some((|| match instruction.op0_kind() {
            OpKind::Register => Ok(Operation::MultiplyAccumulator {
                kind,
                source: register(instruction.op0_register())?,
            }),
            OpKind::Memory => Ok(Operation::ReadMultiplyAccumulator {
                kind,
                address: effective_address(instruction)?,
            }),
            _ => Err(unsupported()),
        })());
    }
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
