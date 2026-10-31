use super::*;

const RETURN_PREDICTION: u32 = 77;

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

pub(super) fn return_prediction_possible(blocks: &[Block], entries: &[EntryPoint]) -> bool {
    blocks
        .iter()
        .flat_map(|block| &block.instructions)
        .any(|instruction| {
            let Operation::Call { target } = *instruction.operation() else {
                return false;
            };
            landing(entries, instruction.next_pc().0) >= 0
                && match target {
                    BranchTarget::Direct(target) => landing(entries, target.0) >= 0,
                    _ => true,
                }
        })
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
            if out.return_prediction_possible {
                // a prediction never replaces the architectural stack read on return.
                let route = landing(entries, fallthrough).wrapping_add(1) as u32 as u64;
                let prediction = (route << 32) | u64::from(fallthrough);
                out.c()
                    .i64_const(prediction as i64)
                    .local_set(RETURN_PREDICTION);
            }
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
            if !out.return_prediction_possible {
                out.c().i32_const(-2).local_set(A);
                return true;
            }
            out.c()
                .local_get(RETURN_PREDICTION)
                .i64_const(32)
                .i64_shr_u()
                .i32_wrap_i64()
                .i32_eqz()
                .i32_eqz()
                .local_get(PC)
                .local_get(RETURN_PREDICTION)
                .i32_wrap_i64()
                .i32_eq()
                .i32_and()
                .if_(BlockType::Result(ValType::I32))
                .local_get(RETURN_PREDICTION)
                .i64_const(32)
                .i64_shr_u()
                .i32_wrap_i64()
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
