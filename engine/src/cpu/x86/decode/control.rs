use iced_x86::{Code, Instruction};

use super::{
    DecodeError,
    operands::{effective_address, register, word_register},
};
use crate::cpu::x86::{ControlOperation, ir::Operation};

pub(super) fn lower(instruction: &Instruction) -> Option<Result<Operation, DecodeError>> {
    let operation = match instruction.code() {
        Code::Popfd => ControlOperation::PopFlags { word: false },
        Code::Popfw => ControlOperation::PopFlags { word: true },
        Code::Ud2 => ControlOperation::InvalidOpcode,
        Code::Int3 => ControlOperation::Breakpoint,
        Code::Into => ControlOperation::Overflow,
        Code::Bound_r16_m1616 | Code::Bound_r32_m3232 => {
            if instruction.has_lock_prefix() {
                return Some(Err(DecodeError::InvalidEncoding));
            }
            let word = instruction.code() == Code::Bound_r16_m1616;
            let index = if word {
                word_register(instruction.op0_register())
            } else {
                register(instruction.op0_register())
            };
            return Some(index.and_then(|index| {
                effective_address(instruction).map(|address| {
                    Operation::Control(ControlOperation::Bound {
                        index,
                        address,
                        word,
                    })
                })
            }));
        }
        _ => return None,
    };
    if instruction.has_lock_prefix() {
        return Some(Err(DecodeError::InvalidEncoding));
    }
    Some(Ok(Operation::Control(operation)))
}
