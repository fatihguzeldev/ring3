use iced_x86::{Code, Instruction};

use super::{
    DecodeError,
    operands::{effective_address, unsupported},
};
use crate::cpu::x86::ir::Operation;

pub(super) fn lower(instruction: &Instruction) -> Option<Result<Operation, DecodeError>> {
    if !matches!(instruction.code(), Code::Clflush_m8 | Code::Rdtscp) {
        return None;
    }
    if instruction.has_lock_prefix() {
        return Some(Err(DecodeError::InvalidEncoding));
    }
    if instruction.has_rep_prefix() || instruction.has_repne_prefix() {
        return Some(Err(unsupported()));
    }
    Some(match instruction.code() {
        Code::Clflush_m8 => {
            effective_address(instruction).map(|address| Operation::CacheFlush { address })
        }
        Code::Rdtscp => Ok(Operation::VirtualTimestampAux),
        _ => unreachable!("cache/timestamp decoder handles only its two codes"),
    })
}
