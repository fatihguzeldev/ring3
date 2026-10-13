use iced_x86::{Code, Instruction};

use super::{
    DecodeError,
    operands::{effective_address, register, unsupported},
};
use crate::cpu::x86::{capabilities::RDPID_ENABLED, ir::Operation};

pub(super) fn lower(instruction: &Instruction) -> Option<Result<Operation, DecodeError>> {
    if !matches!(
        instruction.code(),
        Code::Clflush_m8 | Code::Rdtscp | Code::Rdpid_r32
    ) {
        return None;
    }
    if instruction.has_lock_prefix() {
        return Some(Err(DecodeError::InvalidEncoding));
    }
    // the rdpid code is selected only by its mandatory f3 register encoding.
    if instruction.code() != Code::Rdpid_r32
        && (instruction.has_rep_prefix() || instruction.has_repne_prefix())
    {
        return Some(Err(unsupported()));
    }
    Some(match instruction.code() {
        Code::Clflush_m8 => {
            effective_address(instruction).map(|address| Operation::CacheFlush { address })
        }
        Code::Rdtscp => Ok(Operation::VirtualTimestampAux),
        Code::Rdpid_r32 if RDPID_ENABLED => register(instruction.op0_register())
            .map(|destination| Operation::ReadProcessorId { destination }),
        Code::Rdpid_r32 => Err(unsupported()),
        _ => unreachable!("cache/timestamp decoder handles only its three codes"),
    })
}
