mod structure;
pub(super) mod values;

use super::{StopReason, effects, flags, program};
use crate::cpu::x86::{
    ControlOperation,
    ir::{BranchTarget, Condition, CountBranchKind, Operation},
};

pub(super) use structure::{Shape, StructurePlan};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct BlockId(pub usize);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Target {
    Block(BlockId),
    External(u32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DirectKind {
    Fallthrough,
    Jump,
    Call,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Terminator {
    Next {
        target: Target,
        kind: DirectKind,
    },
    Branch {
        condition: Condition,
        taken: Target,
        otherwise: Target,
    },
    CountBranch {
        kind: CountBranchKind,
        word: bool,
        taken: Target,
        otherwise: Target,
    },
    Indirect,
    GuardedIndirect,
    Exit {
        reason: StopReason,
    },
}

impl Terminator {
    fn targets(self) -> [Option<Target>; 2] {
        match self {
            Self::Next { target, .. } => [Some(target), None],
            Self::Branch {
                taken, otherwise, ..
            }
            | Self::CountBranch {
                taken, otherwise, ..
            } => [Some(taken), Some(otherwise)],
            Self::Indirect | Self::GuardedIndirect | Self::Exit { .. } => [None, None],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ResumePc {
    Exact(u32),
    Computed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ExitPoint {
    pub pc: ResumePc,
    pub retired: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct InstructionExits {
    pub before: ExitPoint,
    // repeated strings account work inside their own loop; host gates do not retire.
    pub after: Option<ExitPoint>,
}

pub(super) struct RegionInstruction {
    pub pc: u32,
    pub next: u32,
    pub operation: Option<Operation>,
    pub gate: Option<u32>,
    pub prefix: u32,
    pub exits: InstructionExits,
}

pub(super) struct FlagLiveness {
    pub before: Vec<u32>,
    pub after: Vec<u32>,
}

pub(super) struct RegionBlock {
    pub entry: u32,
    pub instructions: Vec<RegionInstruction>,
    pub terminator: Terminator,
    pub indirect_targets: Vec<BlockId>,
    pub successors: Vec<BlockId>,
    pub predecessors: Vec<BlockId>,
    pub flags: FlagLiveness,
}

pub(super) struct Region {
    pub entry: BlockId,
    pub blocks: Vec<RegionBlock>,
    pub is_observed: bool,
    entries: Vec<(u32, BlockId)>,
}

impl Region {
    pub fn from_blocks(blocks: &[program::Block], entry: u32) -> Option<Self> {
        Self::construct(blocks, entry, None)
    }

    pub fn from_observed(
        blocks: &[program::Block],
        entry: u32,
        edges: &[(u32, u32)],
    ) -> Option<Self> {
        let edges = edges
            .iter()
            .copied()
            .collect::<std::collections::BTreeSet<_>>();
        Self::construct(blocks, entry, Some(&edges))
    }

    fn construct(
        blocks: &[program::Block],
        entry: u32,
        observed: Option<&std::collections::BTreeSet<(u32, u32)>>,
    ) -> Option<Self> {
        let mut entries: Vec<_> = blocks
            .iter()
            .enumerate()
            .map(|(index, block)| (block.entry, BlockId(index)))
            .collect();
        entries.sort_unstable();
        if blocks.iter().any(|block| block.instructions.is_empty())
            || entries.windows(2).any(|pair| pair[0].0 == pair[1].0)
        {
            return None;
        }
        let entry = entries
            .binary_search_by_key(&entry, |&(pc, _)| pc)
            .ok()
            .map(|index| entries[index].1)?;
        let resolve = |pc| {
            entries
                .binary_search_by_key(&pc, |&(entry, _)| entry)
                .map_or(Target::External(pc), |index| {
                    Target::Block(entries[index].1)
                })
        };
        let mut lowered = Vec::with_capacity(blocks.len());
        for block in blocks {
            let last = block.instructions.last()?;
            if observed.is_some_and(|edges| {
                block.instructions.windows(2).any(|pair| {
                    pair[0].next != pair[1].pc || !edges.contains(&(pair[0].pc, pair[1].pc))
                })
            }) {
                return None;
            }
            let mut terminator = lower_terminator(last, &|pc| {
                if observed.is_none_or(|edges| edges.contains(&(last.pc, pc))) {
                    resolve(pc)
                } else {
                    Target::External(pc)
                }
            });
            let mut indirect_targets = Vec::new();
            if let Some(edges) = observed.filter(|_| terminator == Terminator::Indirect) {
                terminator = Terminator::GuardedIndirect;
                for &(_, pc) in edges.range((last.pc, 0)..=(last.pc, u32::MAX)) {
                    if let Target::Block(id) = resolve(pc) {
                        indirect_targets.push(id);
                    }
                }
                indirect_targets.sort_unstable();
                indirect_targets.dedup();
            }
            let mut successors: Vec<_> = terminator
                .targets()
                .into_iter()
                .flatten()
                .filter_map(|target| match target {
                    Target::Block(id) => Some(id),
                    Target::External(_) => None,
                })
                .collect();
            successors.extend(indirect_targets.iter().copied());
            successors.sort_unstable();
            successors.dedup();
            let instructions: Vec<_> = block
                .instructions
                .iter()
                .enumerate()
                .map(|(index, instruction)| {
                    let prefix = index as u32;
                    RegionInstruction {
                        pc: instruction.pc,
                        next: instruction.next,
                        operation: instruction.operation,
                        gate: instruction.gate,
                        prefix,
                        exits: InstructionExits {
                            before: ExitPoint {
                                pc: ResumePc::Exact(instruction.pc),
                                retired: prefix,
                            },
                            after: resume_after(instruction).map(|pc| ExitPoint {
                                pc,
                                retired: prefix + 1,
                            }),
                        },
                    }
                })
                .collect();
            let flags = flag_liveness(&instructions);
            lowered.push(RegionBlock {
                entry: block.entry,
                instructions,
                terminator,
                indirect_targets,
                successors,
                predecessors: Vec::new(),
                flags,
            });
        }
        for index in 0..lowered.len() {
            for successor in lowered[index].successors.clone() {
                lowered[successor.0].predecessors.push(BlockId(index));
            }
        }
        Some(Self {
            entry,
            blocks: lowered,
            is_observed: observed.is_some(),
            entries,
        })
    }

    pub fn block_for_pc(&self, pc: u32) -> Option<BlockId> {
        self.entries
            .binary_search_by_key(&pc, |&(entry, _)| entry)
            .ok()
            .map(|index| self.entries[index].1)
    }

    pub fn structure(&self) -> StructurePlan {
        self.structure_from(self.entry)
    }

    pub fn structure_from(&self, entry: BlockId) -> StructurePlan {
        structure::plan(self, entry)
    }

    pub fn structure_all(&self) -> StructurePlan {
        structure::plan_all(self)
    }
}

fn lower_terminator(
    instruction: &program::Instruction,
    resolve: &impl Fn(u32) -> Target,
) -> Terminator {
    let next = |pc, kind| Terminator::Next {
        target: resolve(pc),
        kind,
    };
    if instruction.gate.is_some() {
        return Terminator::Exit {
            reason: StopReason::Host,
        };
    }
    match instruction.operation {
        Some(Operation::ConditionalJump { condition, target }) => Terminator::Branch {
            condition,
            taken: resolve(target.0),
            otherwise: resolve(instruction.next),
        },
        Some(
            Operation::CountBranch { kind, target } | Operation::CountBranchWord { kind, target },
        ) => Terminator::CountBranch {
            kind,
            word: matches!(
                instruction.operation,
                Some(Operation::CountBranchWord { .. })
            ),
            taken: resolve(target.0),
            otherwise: resolve(instruction.next),
        },
        Some(Operation::Jump {
            target: BranchTarget::Direct(target),
        }) => next(target.0, DirectKind::Jump),
        Some(Operation::Call {
            target: BranchTarget::Direct(target),
        }) => next(target.0, DirectKind::Call),
        Some(Operation::CallWord { target }) => next(target.0 & 0xffff, DirectKind::Call),
        Some(
            Operation::Jump { .. }
            | Operation::JumpWord { .. }
            | Operation::ReadJumpWord { .. }
            | Operation::Call { .. }
            | Operation::CallRegisterWord { .. }
            | Operation::ReadCallWord { .. }
            | Operation::Return { .. }
            | Operation::ReturnWord { .. },
        ) => Terminator::Indirect,
        Some(Operation::Control(ControlOperation::PopFlags { .. })) => {
            next(instruction.next, DirectKind::Fallthrough)
        }
        Some(
            Operation::Control(ControlOperation::Sysenter) | Operation::SoftwareInterrupt { .. },
        ) => Terminator::Exit {
            reason: StopReason::Host,
        },
        Some(Operation::Control(ControlOperation::Breakpoint)) => Terminator::Exit {
            reason: StopReason::Debug,
        },
        Some(Operation::Control(_)) | None => Terminator::Exit {
            reason: StopReason::Unsupported,
        },
        _ => next(instruction.next, DirectKind::Fallthrough),
    }
}

fn resume_after(instruction: &program::Instruction) -> Option<ResumePc> {
    if instruction.gate.is_some() {
        return None;
    }
    Some(match instruction.operation? {
        Operation::Jump {
            target: BranchTarget::Direct(target),
        }
        | Operation::Call {
            target: BranchTarget::Direct(target),
        } => ResumePc::Exact(target.0),
        Operation::CallWord { target } => ResumePc::Exact(target.0 & 0xffff),
        Operation::ConditionalJump { .. }
        | Operation::CountBranch { .. }
        | Operation::CountBranchWord { .. }
        | Operation::Jump { .. }
        | Operation::JumpWord { .. }
        | Operation::ReadJumpWord { .. }
        | Operation::Call { .. }
        | Operation::CallRegisterWord { .. }
        | Operation::ReadCallWord { .. }
        | Operation::Return { .. }
        | Operation::ReturnWord { .. } => ResumePc::Computed,
        Operation::Control(ControlOperation::PopFlags { .. } | ControlOperation::Breakpoint) => {
            ResumePc::Exact(instruction.next)
        }
        Operation::RepeatString { .. }
        | Operation::Control(_)
        | Operation::SoftwareInterrupt { .. } => return None,
        _ => ResumePc::Exact(instruction.next),
    })
}

fn flag_liveness(instructions: &[RegionInstruction]) -> FlagLiveness {
    let mut before = vec![0; instructions.len()];
    let mut after = vec![0; instructions.len()];
    // every block entry may publish state when its budget guard declines.
    let mut live = flags::ARITHMETIC;
    for (index, instruction) in instructions.iter().enumerate().rev() {
        let Some((operation, effect)) = instruction
            .operation
            .filter(|_| instruction.gate.is_none())
            .and_then(|operation| effects::analyze(operation).map(|effect| (operation, effect)))
        else {
            before[index] = flags::ARITHMETIC;
            after[index] = flags::ARITHMETIC;
            live = flags::ARITHMETIC;
            continue;
        };
        let (can_fault, can_write) = effects::memory_barrier(operation);
        if can_write {
            live = flags::ARITHMETIC;
        }
        after[index] = live;
        live = (live & !effect.write) | effect.read;
        if can_fault {
            live = flags::ARITHMETIC;
        }
        before[index] = live;
    }
    FlagLiveness { before, after }
}

#[cfg(test)]
mod tests;
