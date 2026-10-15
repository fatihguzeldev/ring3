use iced_x86::{Code, Instruction};

use super::{DecodeError, operands::effective_address};
use crate::cpu::x86::ir::Operation;

pub(super) fn lower(
    instruction: &Instruction,
    bytes: &[u8],
) -> Option<Result<Operation, DecodeError>> {
    if let Some(operation) = super::far::lower(instruction) {
        return Some(operation);
    }
    if let Some(operation) = super::xstate::lower(instruction) {
        return Some(operation.map(Operation::XState));
    }
    if let Some(operation) = super::segment_queries::lower(instruction) {
        return Some(operation);
    }
    if let Some(operation) = super::cache_timestamp::lower(instruction) {
        return Some(operation);
    }
    if let Some(operation) = super::packed_strings::lower(instruction) {
        return Some(operation.map(Operation::PackedStringCompare));
    }
    if let Some(operation) = super::control::lower(instruction) {
        return Some(operation);
    }
    if let Some(operation) = super::stack::lower(instruction) {
        return Some(operation);
    }
    if let Some(operation) = super::strings::lower(instruction, bytes) {
        return Some(operation);
    }
    if let Some(operation) = super::segments::lower(instruction) {
        return Some(operation);
    }
    if let Some(decoded) = super::x87::decode(instruction) {
        return Some(x87(instruction, bytes, decoded));
    }
    if let Some(operation) = super::simd::lower(instruction) {
        return Some(operation.map(Operation::Simd));
    }
    if let Some(operation) = super::atomic::lower(instruction) {
        return Some(operation);
    }
    if matches!(
        instruction.code(),
        Code::Movbe_r16_m16 | Code::Movbe_r32_m32 | Code::Movbe_m16_r16 | Code::Movbe_m32_r32
    ) && super::profile::has_repeat_prefix(bytes)
    {
        return Some(Err(super::operands::unsupported()));
    }
    if let Some(operation) = super::scalar_bits::lower(instruction) {
        return Some(operation);
    }
    super::bit_manipulation::lower(instruction)
}

fn x87(
    instruction: &Instruction,
    bytes: &[u8],
    decoded: super::x87::DecodedX87,
) -> Result<Operation, DecodeError> {
    Ok(match instruction.code() {
        Code::Fninit => Operation::InitializeX87,
        Code::Fnclex => Operation::ClearX87Exceptions,
        Code::Fnstsw_AX => Operation::X87StatusToAx,
        Code::Fnstsw_m2byte => Operation::X87StatusToMemory {
            address: effective_address(instruction)?,
        },
        Code::Fnstcw_m2byte => Operation::X87ControlToMemory {
            address: effective_address(instruction)?,
        },
        _ => {
            let opcode = bytes
                .iter()
                .position(|byte| (0xd8..=0xdf).contains(byte))
                .and_then(|index| {
                    bytes
                        .get(index + 1)
                        .map(|modrm| (u16::from(bytes[index] & 7) << 8) | u16::from(*modrm))
                })
                .unwrap_or(0);
            Operation::X87 {
                operation: decoded.operation,
                address: if decoded.memory {
                    Some(effective_address(instruction)?)
                } else {
                    None
                },
                waiting: decoded.waiting,
                opcode,
            }
        }
    })
}

#[cfg(test)]
mod tests;
