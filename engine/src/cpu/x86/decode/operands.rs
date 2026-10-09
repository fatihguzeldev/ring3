use iced_x86::{Instruction, OpKind, Register};

use super::DecodeError;
use crate::cpu::{
    UnsupportedFeature,
    x86::{
        Register32,
        ir::{
            ByteRegister, ByteValue, EffectiveAddress, Location32, SmallSource, SmallWidth,
            Value32, WordValue,
        },
    },
};

pub(super) fn unsupported() -> DecodeError {
    DecodeError::Unsupported(UnsupportedFeature::Opcode)
}

pub(super) fn register(register: Register) -> Result<Register32, DecodeError> {
    match register {
        Register::EAX => Ok(Register32::Eax),
        Register::ECX => Ok(Register32::Ecx),
        Register::EDX => Ok(Register32::Edx),
        Register::EBX => Ok(Register32::Ebx),
        Register::ESP => Ok(Register32::Esp),
        Register::EBP => Ok(Register32::Ebp),
        Register::ESI => Ok(Register32::Esi),
        Register::EDI => Ok(Register32::Edi),
        _ => Err(unsupported()),
    }
}

fn optional_register(register: Register) -> Result<Option<Register32>, DecodeError> {
    if register == Register::None {
        Ok(None)
    } else {
        self::register(register).map(Some)
    }
}

pub(super) fn word_register(register: Register) -> Result<Register32, DecodeError> {
    match register {
        Register::AX => Ok(Register32::Eax),
        Register::CX => Ok(Register32::Ecx),
        Register::DX => Ok(Register32::Edx),
        Register::BX => Ok(Register32::Ebx),
        Register::SP => Ok(Register32::Esp),
        Register::BP => Ok(Register32::Ebp),
        Register::SI => Ok(Register32::Esi),
        Register::DI => Ok(Register32::Edi),
        _ => Err(unsupported()),
    }
}

pub(super) fn word_value(instruction: &Instruction) -> Result<WordValue, DecodeError> {
    match instruction.op1_kind() {
        OpKind::Register => word_register(instruction.op1_register()).map(WordValue::Register),
        OpKind::Immediate16 => Ok(WordValue::Immediate(instruction.immediate16())),
        OpKind::Immediate8to16 => Ok(WordValue::Immediate(instruction.immediate8to16() as u16)),
        _ => Err(unsupported()),
    }
}

pub(super) fn effective_address(
    instruction: &Instruction,
) -> Result<EffectiveAddress, DecodeError> {
    let base = optional_register(instruction.memory_base())?;
    let index = optional_register(instruction.memory_index())?;
    let scale = instruction.memory_index_scale();
    if instruction.memory_displ_size() == 2 || !matches!(scale, 1 | 2 | 4 | 8) {
        return Err(unsupported());
    }
    Ok(EffectiveAddress {
        base,
        index,
        scale: scale as u8,
        displacement: instruction.memory_displacement32(),
    })
}

pub(super) fn location(instruction: &Instruction, index: u32) -> Result<Location32, DecodeError> {
    match instruction.op_kind(index) {
        OpKind::Register => register(instruction.op_register(index)).map(Location32::Register),
        OpKind::Memory => effective_address(instruction).map(Location32::Memory),
        _ => Err(unsupported()),
    }
}

pub(super) fn value(instruction: &Instruction, index: u32) -> Result<Value32, DecodeError> {
    match instruction.op_kind(index) {
        OpKind::Register => register(instruction.op_register(index)).map(Value32::Register),
        OpKind::Memory => effective_address(instruction).map(Value32::Memory),
        OpKind::Immediate32 => Ok(Value32::Immediate(instruction.immediate32())),
        OpKind::Immediate8to32 => Ok(Value32::Immediate(instruction.immediate8to32() as u32)),
        _ => Err(unsupported()),
    }
}

pub(super) fn byte_register(register: Register) -> Result<ByteRegister, DecodeError> {
    match register {
        Register::AL => Ok(ByteRegister::Al),
        Register::CL => Ok(ByteRegister::Cl),
        Register::DL => Ok(ByteRegister::Dl),
        Register::BL => Ok(ByteRegister::Bl),
        Register::AH => Ok(ByteRegister::Ah),
        Register::CH => Ok(ByteRegister::Ch),
        Register::DH => Ok(ByteRegister::Dh),
        Register::BH => Ok(ByteRegister::Bh),
        _ => Err(unsupported()),
    }
}

pub(super) fn byte_value(instruction: &Instruction) -> Result<ByteValue, DecodeError> {
    match instruction.op1_kind() {
        OpKind::Immediate8 => Ok(ByteValue::Immediate(instruction.immediate8())),
        OpKind::Register => byte_register(instruction.op1_register()).map(ByteValue::Register),
        _ => Err(unsupported()),
    }
}

pub(super) fn small_source(
    instruction: &Instruction,
    width: SmallWidth,
) -> Result<SmallSource, DecodeError> {
    if instruction.op1_kind() == OpKind::Memory {
        return Ok(SmallSource::Memory {
            address: effective_address(instruction)?,
            width,
        });
    }
    let (register, high_byte) = match (width, instruction.op1_register()) {
        (SmallWidth::Byte, Register::AL) | (SmallWidth::Word, Register::AX) => {
            (Register32::Eax, false)
        }
        (SmallWidth::Byte, Register::CL) | (SmallWidth::Word, Register::CX) => {
            (Register32::Ecx, false)
        }
        (SmallWidth::Byte, Register::DL) | (SmallWidth::Word, Register::DX) => {
            (Register32::Edx, false)
        }
        (SmallWidth::Byte, Register::BL) | (SmallWidth::Word, Register::BX) => {
            (Register32::Ebx, false)
        }
        (SmallWidth::Word, Register::SP) => (Register32::Esp, false),
        (SmallWidth::Word, Register::BP) => (Register32::Ebp, false),
        (SmallWidth::Word, Register::SI) => (Register32::Esi, false),
        (SmallWidth::Word, Register::DI) => (Register32::Edi, false),
        (SmallWidth::Byte, Register::AH) => (Register32::Eax, true),
        (SmallWidth::Byte, Register::CH) => (Register32::Ecx, true),
        (SmallWidth::Byte, Register::DH) => (Register32::Edx, true),
        (SmallWidth::Byte, Register::BH) => (Register32::Ebx, true),
        _ => return Err(unsupported()),
    };
    Ok(SmallSource::Register {
        register,
        width,
        high_byte,
    })
}
