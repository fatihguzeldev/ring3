use super::*;
use crate::cpu::x86::ir::{BranchTarget, Operation};

pub(in crate::cpu::dbt) fn prepare_runtime_region_until(
    memory: &AddressSpace,
    entry: GuestAddress,
    limits: CompileLimits,
    cs: Option<CodeFetchContext>,
    authority: crate::cpu::x86::decode::DecodeAuthority,
    joins: &dyn Fn(GuestAddress) -> bool,
) -> Result<PreparedRegion, CompileError> {
    let mut prepared =
        super::prepare_runtime_entry_until(memory, entry, limits, cs, authority, joins)?;
    let mut entries = Vec::new();
    entries
        .try_reserve_exact(limits.blocks)
        .map_err(|_| CompileError::Allocation)?;
    entries.push(entry);
    let mut frontier = Vec::new();
    frontier
        .try_reserve_exact(2 * limits.blocks)
        .map_err(|_| CompileError::Allocation)?;
    enqueue(&prepared.blocks[0], &mut frontier);
    let mut next = 0;
    while next < frontier.len()
        && entries.len() < limits.blocks
        && prepared.instruction_count() < limits.instructions
    {
        let candidate = frontier[next];
        next += 1;
        if joins(candidate) || covers(&prepared, candidate) {
            continue;
        }
        entries.push(candidate);
        match super::prepare(
            memory,
            &entries,
            limits,
            true,
            &[],
            DecodePolicy::Execution,
            cs,
            authority,
            joins,
            DecodeFailure::KeepPrefix,
        ) {
            Ok(region) => {
                enqueue(region.blocks.last().unwrap(), &mut frontier);
                prepared = region;
            }
            Err(error) if speculative_boundary(error) => {
                entries.pop();
            }
            Err(error) => return Err(error),
        }
    }
    Ok(prepared)
}

fn enqueue(block: &CompiledBlock, frontier: &mut Vec<GuestAddress>) {
    let Some(last) = block.instructions.last() else {
        return;
    };
    let targets = match *last.operation() {
        Operation::Jump {
            target: BranchTarget::Direct(target),
        } => [Some(target), None],
        Operation::Call {
            target: BranchTarget::Direct(target),
        } => [Some(target), Some(last.next_pc())],
        Operation::ConditionalJump { target, .. }
        | Operation::CountBranch { target, .. }
        | Operation::CountBranchWord { target, .. } => [Some(last.next_pc()), Some(target)],
        _ => [None, None],
    };
    for target in targets.into_iter().flatten() {
        if !frontier.contains(&target) {
            frontier.push(target);
        }
    }
}

fn covers(prepared: &PreparedRegion, pc: GuestAddress) -> bool {
    let pc = u64::from(pc.0);
    prepared.blocks.iter().any(|block| {
        block.instructions.iter().any(|instruction| {
            let start = u64::from(instruction.pc().0);
            start <= pc && pc < start + u64::from(instruction.length())
        }) || block.context_fault().is_some_and(|fault| {
            let start = u64::from(fault.pc.0);
            pc == start || (start <= pc && pc < start + u64::from(fault.consumed_prefix_length))
        })
    })
}

fn speculative_boundary(error: CompileError) -> bool {
    match error {
        CompileError::Instruction {
            cause: InstructionError::Decode(crate::cpu::x86::decode::DecodeError::Infrastructure(_)),
            ..
        } => false,
        CompileError::Instruction { .. } | CompileError::InvalidBlocks => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests;
