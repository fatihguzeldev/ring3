use super::*;

const RETURN_PC: u32 = 77;
const RETURN_ROUTE: u32 = 78;

fn landing(entries: &[EntryPoint], pc: u32) -> i32 {
    entries
        .binary_search_by_key(&pc, |entry| entry.pc)
        .map_or(-1, |index| {
            let entry = &entries[index];
            ((entry.block << 6) | entry.instruction) as i32
        })
}

fn static_targets(block: &Block) -> [Option<u32>; 2] {
    let Some(last) = block.instructions.last() else {
        return [None, None];
    };
    match *last.operation() {
        Operation::Call {
            target: BranchTarget::Direct(target),
        }
        | Operation::Jump {
            target: BranchTarget::Direct(target),
        } => [Some(target.0), None],
        Operation::ConditionalJump { target, .. } | Operation::CountBranch { target, .. } => {
            [Some(target.0), Some(last.next_pc().0)]
        }
        Operation::Return { .. } | Operation::Call { .. } | Operation::Jump { .. } => [None, None],
        _ => [Some(last.next_pc().0), None],
    }
}

pub(super) fn self_loop(blocks: &[Block], entries: &[EntryPoint], index: usize) -> bool {
    blocks[index].gate.is_none()
        && static_targets(&blocks[index])
            .into_iter()
            .flatten()
            .any(|pc| {
                let encoded = landing(entries, pc);
                encoded >= 0 && (encoded as usize >> 6) == index
            })
}

pub(super) fn branch_known(
    out: &mut Output,
    blocks: &[Block],
    entries: &[EntryPoint],
    index: usize,
    self_loop: bool,
) {
    let targets = static_targets(&blocks[index]);
    for pc in targets.into_iter().flatten() {
        let encoded = landing(entries, pc);
        if encoded < 0 {
            continue;
        }
        let target_block = encoded as usize >> 6;
        let depth = if target_block == index && self_loop {
            0
        } else if target_block > index {
            (target_block - index - 1) as u32 + u32::from(self_loop)
        } else {
            continue;
        };
        if targets[1].is_some() {
            out.c()
                .local_get(A)
                .i32_const(encoded)
                .i32_eq()
                .if_(BlockType::Empty)
                .br(depth + 1)
                .end();
        } else {
            out.c().br(depth);
        }
    }
}

pub(super) fn next(
    out: &mut Output,
    blocks: &[Block],
    entries: &[EntryPoint],
    index: usize,
) -> bool {
    let block = &blocks[index];
    let Some(instruction) = block.instructions.last() else {
        out.c().i32_const(-1).local_set(A);
        return false;
    };
    let fallthrough = instruction.next_pc().0;
    match *instruction.operation() {
        Operation::Call { target } => {
            // a prediction never replaces the architectural stack read on return.
            out.c()
                .i32_const(fallthrough as i32)
                .local_set(RETURN_PC)
                .i32_const(landing(entries, fallthrough).wrapping_add(1))
                .local_set(RETURN_ROUTE);
            match target {
                BranchTarget::Direct(target) => {
                    out.c().i32_const(landing(entries, target.0)).local_set(A);
                }
                _ => {
                    out.c().i32_const(-2).local_set(A);
                }
            }
            return !matches!(target, BranchTarget::Direct(_));
        }
        Operation::Jump {
            target: BranchTarget::Direct(target),
        } => {
            out.c().i32_const(landing(entries, target.0)).local_set(A);
        }
        Operation::ConditionalJump { target, .. } | Operation::CountBranch { target, .. } => {
            out.c()
                .i32_const(landing(entries, target.0))
                .i32_const(landing(entries, fallthrough))
                .local_get(PC)
                .i32_const(target.0 as i32)
                .i32_eq()
                .select()
                .local_set(A);
        }
        Operation::Return { .. } => {
            out.c()
                .local_get(RETURN_ROUTE)
                .i32_eqz()
                .i32_eqz()
                .local_get(PC)
                .local_get(RETURN_PC)
                .i32_eq()
                .i32_and()
                .if_(BlockType::Result(ValType::I32))
                .local_get(RETURN_ROUTE)
                .i32_const(1)
                .i32_sub()
                .else_();
            out.c().i32_const(-2).end().local_set(A);
            return true;
        }
        Operation::Jump { .. } => {
            out.c().i32_const(-2).local_set(A);
            return true;
        }
        _ => {
            out.c()
                .i32_const(landing(entries, fallthrough))
                .local_set(A);
        }
    }
    false
}
