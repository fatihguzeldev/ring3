use super::*;

fn register_bit(register: Register32) -> u8 {
    1 << register.index()
}
fn byte_bit(register: ByteRegister) -> u8 {
    1 << (byte_register(register).0 - 1)
}

pub(super) fn preserves(operation: Operation, right: Option<Register32>, numeric: bool) -> bool {
    let writes = match operation {
        Operation::Nop | Operation::Direction { .. } | Operation::ConditionalJump { .. } => 0,
        Operation::Move {
            destination: Location32::Register(register),
            ..
        }
        | Operation::Lea {
            destination: register,
            ..
        }
        | Operation::LeaWord {
            destination: register,
            ..
        }
        | Operation::MoveWord {
            destination: register,
            ..
        }
        | Operation::LoadWord {
            destination: register,
            ..
        }
        | Operation::Extend {
            destination: register,
            ..
        }
        | Operation::ExtendByteToWord {
            destination: register,
            ..
        }
        | Operation::ReadExtendByteToWord {
            destination: register,
            ..
        }
        | Operation::ConditionalMove {
            destination: register,
            ..
        }
        | Operation::ConditionalMoveWord {
            destination: register,
            ..
        }
        | Operation::ReadConditionalMove {
            destination: register,
            ..
        }
        | Operation::ReadConditionalMoveWord {
            destination: register,
            ..
        }
        | Operation::Unary {
            kind: UnaryKind::Not,
            destination: Location32::Register(register),
        } => register_bit(register),
        Operation::MoveByte { destination, .. }
        | Operation::LoadByte { destination, .. }
        | Operation::SetByte { destination, .. } => byte_bit(destination),
        Operation::Move {
            destination: Location32::Memory(_),
            ..
        }
        | Operation::StoreByte { .. }
        | Operation::StoreWord { .. }
        | Operation::MemorySetByte { .. }
        | Operation::Unary {
            kind: UnaryKind::Not,
            destination: Location32::Memory(_),
        }
        | Operation::Jump { .. } => 0,
        Operation::Exchange { left, right } => register_bit(left) | register_bit(right),
        Operation::CountBranch { kind, .. } => {
            if kind == CountBranchKind::EcxZero {
                0
            } else {
                register_bit(Register32::Ecx)
            }
        }
        Operation::Push { .. } | Operation::Call { .. } | Operation::Return { .. } => {
            register_bit(Register32::Esp)
        }
        Operation::Pop {
            destination: Location32::Register(register),
        } => register_bit(Register32::Esp) | register_bit(register),
        Operation::Pop {
            destination: Location32::Memory(_),
        } => register_bit(Register32::Esp),
        Operation::Leave => register_bit(Register32::Esp) | register_bit(Register32::Ebp),
        Operation::SignExtendAccumulatorWord => register_bit(Register32::Eax),
        Operation::SignExtendHighWord | Operation::SignExtendHigh => register_bit(Register32::Edx),
        Operation::X87StatusToAx
        | Operation::X87 {
            operation: crate::cpu::x86::X87Operation::StatusToAx,
            ..
        } if numeric => register_bit(Register32::Eax),
        _ if numeric => 0,
        _ => return false,
    };
    right.is_none_or(|register| writes & register_bit(register) == 0)
}

pub(super) fn finish(out: &mut Output, _blocks: &[Block], _entries: &[EntryPoint], _index: usize) {
    out.canonicalize_flags();
}
