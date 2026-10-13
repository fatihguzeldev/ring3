use super::{
    DecodeError,
    operands::{effective_address, register},
};
use crate::cpu::{
    UnsupportedFeature,
    x86::{
        Register32, SegmentRegister,
        avx::{AvxInstruction, AvxOperation, MemoryEffect},
        ir::{AddressSize, EffectiveAddress},
    },
};
use iced_x86::{Code, EncodingKind, Instruction, OpKind, Register};
mod encoding;
use encoding::Encoding;
mod compare;
mod compound;
mod convert;
mod cross_lane;
mod crypto;
mod floating;
mod integer;
mod logical;
mod masked;
mod moves;
mod multiply;
mod permute;
mod predicates;
mod rearrange;
mod scalar;
mod shift;
mod strings;
fn invalid() -> DecodeError {
    DecodeError::InvalidEncoding
}
fn unsupported() -> DecodeError {
    DecodeError::Unsupported(UnsupportedFeature::Simd)
}
fn index(
    instruction: &Instruction,
    operand: u32,
    general: bool,
    memory: bool,
) -> Result<u32, DecodeError> {
    if memory && instruction.op_kind(operand) == OpKind::Memory {
        return Ok(0);
    }
    if instruction.op_kind(operand) != OpKind::Register {
        return Err(invalid());
    }
    let raw = instruction.op_register(operand);
    if general {
        return register(raw).map(|r| r.index() as u32);
    }
    if (Register::XMM0..=Register::XMM7).contains(&raw) {
        return Ok(raw as u32 - Register::XMM0 as u32);
    }
    if (Register::YMM0..=Register::YMM7).contains(&raw) {
        return Ok(raw as u32 - Register::YMM0 as u32);
    }
    Err(unsupported())
}
fn bind_address(
    instruction: &Instruction,
    value: AvxInstruction,
) -> Result<AvxOperation, DecodeError> {
    let address = if value.memory() == MemoryEffect::None {
        None
    } else if instruction.code() == Code::VEX_Vmaskmovdqu_rDI_xmm_xmm {
        Some(EffectiveAddress {
            base: Some(Register32::Edi),
            address_size: if instruction.op0_kind() == OpKind::MemorySegDI {
                AddressSize::Bits16
            } else {
                AddressSize::Bits32
            },
            segment: match instruction.segment_prefix() {
                Register::ES => Some(SegmentRegister::Es),
                Register::CS => Some(SegmentRegister::Cs),
                Register::SS => Some(SegmentRegister::Ss),
                Register::DS => Some(SegmentRegister::Ds),
                Register::FS => Some(SegmentRegister::Fs),
                Register::GS => Some(SegmentRegister::Gs),
                _ => None,
            },
            ..EffectiveAddress::default()
        })
    } else {
        Some(effective_address(instruction)?)
    };
    AvxOperation::new(value, address).map_err(|_| invalid())
}
// this lowerer is complete independently of feature exposure. the caller owns
// the canonical CPU capability/XCR0 gate before admitting a guest operation.
pub(crate) fn lower(
    instruction: &Instruction,
    bytes: &[u8],
) -> Option<Result<AvxOperation, DecodeError>> {
    if instruction.encoding() != EncodingKind::VEX
        || super::bit_manipulation::lower(instruction).is_some()
    {
        return None;
    }
    let encoding = match Encoding::parse(bytes) {
        Ok(value) => value,
        Err(error) => return Some(Err(error)),
    };
    if let Some(value) = moves::lower(instruction, encoding) {
        return Some(value);
    }
    if let Some(value) = permute::lower(instruction, encoding) {
        return Some(value);
    }
    if let Some(value) = convert::lower(instruction, encoding) {
        return Some(value);
    }
    if let Some(value) = compare::lower(instruction, encoding) {
        return Some(value);
    }
    if let Some(value) = floating::lower(instruction, encoding) {
        return Some(value);
    }
    if let Some(value) = logical::lower(instruction, encoding) {
        return Some(value);
    }
    if let Some(value) = rearrange::lower(instruction, encoding) {
        return Some(value);
    }
    if let Some(value) = integer::lower(instruction, encoding) {
        return Some(value);
    }
    if let Some(value) = shift::lower(instruction, encoding) {
        return Some(value);
    }
    if let Some(value) = compound::lower(instruction, encoding) {
        return Some(value);
    }
    if let Some(value) = scalar::lower(instruction, encoding) {
        return Some(value);
    }
    if let Some(value) = multiply::lower(instruction, encoding) {
        return Some(value);
    }
    if let Some(value) = masked::lower(instruction, encoding) {
        return Some(value);
    }
    if let Some(value) = predicates::lower(instruction, encoding) {
        return Some(value);
    }
    if let Some(value) = cross_lane::lower(instruction, encoding) {
        return Some(value);
    }
    if let Some(value) = crypto::lower(instruction, encoding) {
        return Some(value);
    }
    if let Some(value) = strings::lower(instruction, encoding) {
        return Some(value);
    }
    Some(Err(unsupported()))
}

#[cfg(test)]
mod native;

#[cfg(test)]
#[path = "execution/native.rs"]
mod execution_tests;
