use iced_x86::{Code, Instruction, OpKind};

use super::{
    DecodeError,
    operands::{byte_register, effective_address, register, unsupported, word_register},
};
use crate::cpu::x86::{
    bits::{BitCountKind, Crc32Source, Crc32Width, ScalarWidth},
    ir::{Location32, Operation, Value32},
};

pub(super) fn lower(instruction: &Instruction) -> Option<Result<Operation, DecodeError>> {
    if let Some(width) = match instruction.code() {
        Code::Crc32_r32_rm8 => Some(Crc32Width::Byte),
        Code::Crc32_r32_rm16 => Some(Crc32Width::Word),
        Code::Crc32_r32_rm32 => Some(Crc32Width::Dword),
        _ => None,
    } {
        return Some((|| {
            let destination = register(instruction.op0_register())?;
            let source = match instruction.op1_kind() {
                OpKind::Memory => Crc32Source::Memory {
                    width,
                    address: effective_address(instruction)?,
                },
                OpKind::Register => match width {
                    Crc32Width::Byte => {
                        Crc32Source::ByteRegister(byte_register(instruction.op1_register())?)
                    }
                    Crc32Width::Word => {
                        Crc32Source::WordRegister(word_register(instruction.op1_register())?)
                    }
                    Crc32Width::Dword => {
                        Crc32Source::DwordRegister(register(instruction.op1_register())?)
                    }
                },
                _ => return Err(unsupported()),
            };
            Ok(Operation::Crc32 {
                destination,
                source,
            })
        })());
    }
    use BitCountKind::{LeadingZero, Population, TrailingZero};
    use ScalarWidth::{Dword, Word};
    let count = match instruction.code() {
        Code::Tzcnt_r16_rm16 => Some((TrailingZero, Word)),
        Code::Lzcnt_r16_rm16 => Some((LeadingZero, Word)),
        Code::Lzcnt_r32_rm32 => Some((LeadingZero, Dword)),
        Code::Popcnt_r16_rm16 => Some((Population, Word)),
        Code::Popcnt_r32_rm32 => Some((Population, Dword)),
        _ => None,
    };
    if let Some((kind, width)) = count {
        return Some((|| {
            let destination = scalar_register(instruction.op0_register(), width)?;
            let source = source(instruction, 1, width)?;
            Ok(Operation::BitCount {
                kind,
                width,
                destination,
                source,
            })
        })());
    }
    let (width, load) = match instruction.code() {
        Code::Movbe_r16_m16 => (Word, true),
        Code::Movbe_r32_m32 => (Dword, true),
        Code::Movbe_m16_r16 => (Word, false),
        Code::Movbe_m32_r32 => (Dword, false),
        _ => return None,
    };
    Some((|| {
        let address = effective_address(instruction)?;
        let (destination, source) = if load {
            (
                Location32::Register(scalar_register(instruction.op0_register(), width)?),
                Value32::Memory(address),
            )
        } else {
            (
                Location32::Memory(address),
                Value32::Register(scalar_register(instruction.op1_register(), width)?),
            )
        };
        Ok(Operation::MoveByteSwap {
            width,
            destination,
            source,
        })
    })())
}

fn scalar_register(
    register_value: iced_x86::Register,
    width: ScalarWidth,
) -> Result<crate::cpu::x86::Register32, DecodeError> {
    match width {
        ScalarWidth::Word => word_register(register_value),
        ScalarWidth::Dword => register(register_value),
    }
}

fn source(
    instruction: &Instruction,
    index: u32,
    width: ScalarWidth,
) -> Result<Value32, DecodeError> {
    match instruction.op_kind(index) {
        OpKind::Register => {
            scalar_register(instruction.op_register(index), width).map(Value32::Register)
        }
        OpKind::Memory => effective_address(instruction).map(Value32::Memory),
        _ => Err(unsupported()),
    }
}
