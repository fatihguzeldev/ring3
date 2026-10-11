use super::DecodeError;
use crate::cpu::x86::{StackFrameKind, ir::Operation};
use iced_x86::{Code, Instruction};

pub(super) fn lower(instruction: &Instruction) -> Option<Result<Operation, DecodeError>> {
    let kind = match instruction.code() {
        Code::Pushad => StackFrameKind::PushAll32,
        Code::Pushaw => StackFrameKind::PushAll16,
        Code::Enterd_imm16_imm8 => StackFrameKind::Enter32,
        Code::Enterw_imm16_imm8 => StackFrameKind::Enter16,
        _ => return None,
    };
    if instruction.has_lock_prefix() {
        return Some(Err(DecodeError::InvalidEncoding));
    }
    let (allocation, nesting) = if matches!(kind, StackFrameKind::Enter32 | StackFrameKind::Enter16)
    {
        (instruction.immediate16(), instruction.immediate8_2nd() & 31)
    } else {
        (0, 0)
    };
    // Keep the longstanding zero-frame IR shape for source compatibility.
    Some(Ok(
        if allocation == 0 && nesting == 0 && kind == StackFrameKind::Enter32 {
            Operation::EnterZero
        } else if allocation == 0 && nesting == 0 && kind == StackFrameKind::Enter16 {
            Operation::EnterZeroWord
        } else {
            Operation::StackFrame {
                kind,
                allocation,
                nesting,
            }
        },
    ))
}
