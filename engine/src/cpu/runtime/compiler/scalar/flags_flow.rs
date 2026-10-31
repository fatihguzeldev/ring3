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

fn killer(operation: Operation) -> Option<bool> {
    match operation {
        Operation::Binary {
            kind,
            destination: Location32::Register(_),
            source: Value32::Register(_) | Value32::Immediate(_),
        } => Some(matches!(kind, BinaryKind::Adc | BinaryKind::Sbb)),
        Operation::Unary {
            kind: UnaryKind::Inc | UnaryKind::Dec,
            destination: Location32::Register(_),
        } => Some(true),
        Operation::Unary {
            kind: UnaryKind::Neg,
            destination: Location32::Register(_),
        } => Some(false),
        _ => None,
    }
}

fn pure_prefix(operation: Operation) -> bool {
    matches!(
        operation,
        Operation::Nop
            | Operation::Direction { .. }
            | Operation::Move {
                destination: Location32::Register(_),
                source: Value32::Register(_) | Value32::Immediate(_)
            }
            | Operation::Lea { .. }
            | Operation::Exchange { .. }
            | Operation::Unary {
                kind: UnaryKind::Not,
                destination: Location32::Register(_)
            }
    )
}

fn prefix(blocks: &[Block], entries: &[EntryPoint], pc: u32) -> Option<(u32, bool)> {
    let entry = &entries[entries.binary_search_by_key(&pc, |entry| entry.pc).ok()?];
    let block = &blocks[entry.block as usize];
    for (index, instruction) in block
        .instructions
        .iter()
        .skip(entry.instruction as usize)
        .enumerate()
    {
        let operation = *instruction.operation();
        if let Some(carry) = killer(operation) {
            return Some((index as u32 + 1, carry));
        }
        if !pure_prefix(operation) {
            break;
        }
    }
    None
}

fn choose(out: &mut Output, policies: &[(u32, u32, bool)]) {
    let Some(&(pc, length, carry)) = policies.first() else {
        out.materialize_flags();
        return;
    };
    out.c()
        .local_get(PC)
        .i32_const(pc as i32)
        .i32_eq()
        .local_get(REMAIN)
        .i32_const(length as i32)
        .i32_ge_u()
        .i32_and()
        .if_(BlockType::Empty);
    if carry {
        out.capture_carry();
    }
    out.c().i32_const(0).local_set(72).else_();
    choose(out, &policies[1..]);
    out.c().end();
}

pub(super) fn finish(out: &mut Output, blocks: &[Block], entries: &[EntryPoint], index: usize) {
    if out.recipe.is_none() {
        return;
    }
    let block = &blocks[index];
    let targets = match block.instructions.last() {
        Some(last) if block.gate.is_none() => match *last.operation() {
            Operation::Call {
                target: BranchTarget::Direct(target),
            }
            | Operation::Jump {
                target: BranchTarget::Direct(target),
            } => [Some(target.0), None],
            Operation::ConditionalJump { target, .. } | Operation::CountBranch { target, .. } => {
                [Some(target.0), Some(last.next_pc().0)]
            }
            Operation::Call { .. } | Operation::Jump { .. } | Operation::Return { .. } => {
                [None, None]
            }
            _ => [Some(last.next_pc().0), None],
        },
        _ => [None, None],
    };
    let mut policies = Vec::new();
    for pc in targets.into_iter().flatten() {
        if policies.iter().any(|&(previous, _, _)| previous == pc) {
            continue;
        }
        if let Some((length, carry)) = prefix(blocks, entries, pc) {
            policies.push((pc, length, carry));
        }
    }
    choose(out, &policies);
    out.recipe = None;
}
