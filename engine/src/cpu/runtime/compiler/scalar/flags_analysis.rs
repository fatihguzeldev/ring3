use super::*;
use std::collections::{BTreeMap, VecDeque};

pub(super) struct Analysis {
    nodes: Vec<Node>,
    indices: BTreeMap<u32, usize>,
}

struct Node {
    pc: u32,
    operation: Option<Operation>,
    produced: Option<deferred::Recipe>,
    targets: [Option<u32>; 2],
    expected: Option<deferred::Recipe>,
    conflict: bool,
}

pub(super) fn targets(operation: Operation, next: u32) -> [Option<u32>; 2] {
    match operation {
        Operation::Call {
            target: BranchTarget::Direct(target),
        }
        | Operation::Jump {
            target: BranchTarget::Direct(target),
        } => [Some(target.0), None],
        Operation::ConditionalJump { target, .. } | Operation::CountBranch { target, .. } => {
            [Some(target.0), Some(next)]
        }
        Operation::Call { .. } | Operation::Jump { .. } | Operation::Return { .. } => [None, None],
        _ => [Some(next), None],
    }
}

fn killed_before_barrier(
    block: &Block,
    index: usize,
    recipe: deferred::Recipe,
    incoming: &BTreeMap<u32, Option<u32>>,
) -> bool {
    let mut previous = block.instructions[index].pc().0;
    for instruction in block.instructions.iter().skip(index + 1) {
        // a competing edge may require canonical flags before the quota guard.
        if incoming.get(&instruction.pc().0) != Some(&Some(previous)) {
            return false;
        }
        let operation = *instruction.operation();
        if matches!(
            operation,
            Operation::Binary {
                kind: BinaryKind::Add | BinaryKind::Sub | BinaryKind::Cmp,
                destination: Location32::Register(_),
                source: Value32::Register(_) | Value32::Immediate(_),
            }
        ) {
            return true;
        }
        let pure = matches!(
            operation,
            Operation::Nop
                | Operation::Direction { .. }
                | Operation::Move {
                    destination: Location32::Register(_),
                    source: Value32::Register(_) | Value32::Immediate(_),
                }
                | Operation::Lea { .. }
                | Operation::Exchange { .. }
                | Operation::Unary {
                    kind: UnaryKind::Not,
                    destination: Location32::Register(_),
                }
        );
        if !pure || !flags_flow::preserves(operation, recipe.dependencies(), false) {
            return false;
        }
        previous = instruction.pc().0;
    }
    false
}

impl Analysis {
    pub(super) fn new(blocks: &[Block], entries: &[EntryPoint], native: bool) -> Self {
        let mut nodes: Vec<_> = entries
            .iter()
            .map(|entry| {
                let instruction = blocks[entry.block as usize]
                    .instructions
                    .get(entry.instruction as usize);
                Node {
                    pc: entry.pc,
                    operation: instruction.map(|instruction| *instruction.operation()),
                    produced: instruction.and_then(|instruction| {
                        let operation = *instruction.operation();
                        let Operation::Binary { kind, .. } = operation else {
                            return None;
                        };
                        if native
                            && matches!(kind, BinaryKind::Add | BinaryKind::Sub)
                            && let Operation::Binary {
                                destination: Location32::Register(result),
                                source: Value32::Immediate(right),
                                ..
                            } = operation
                        {
                            return Some(deferred::Recipe::Arithmetic {
                                kind,
                                result,
                                right: deferred::Operand::Immediate(right),
                            });
                        }
                        deferred::recipe_for_flags(operation, kind, 32)
                    }),
                    targets: instruction.map_or([None, None], |instruction| {
                        targets(*instruction.operation(), instruction.next_pc().0)
                    }),
                    expected: None,
                    conflict: false,
                }
            })
            .collect();
        if native {
            let mut incoming = BTreeMap::new();
            for node in &nodes {
                for pc in node.targets.into_iter().flatten() {
                    incoming
                        .entry(pc)
                        .and_modify(|previous| {
                            if *previous != Some(node.pc) {
                                *previous = None;
                            }
                        })
                        .or_insert(Some(node.pc));
                }
            }
            for (node, entry) in nodes.iter_mut().zip(entries) {
                if let Some(recipe) = node.produced
                    && !killed_before_barrier(
                        &blocks[entry.block as usize],
                        entry.instruction as usize,
                        recipe,
                        &incoming,
                    )
                {
                    node.produced = None;
                }
            }
        }
        let indices = nodes
            .iter()
            .enumerate()
            .map(|(index, node)| (node.pc, index))
            .collect();
        let mut analysis = Self { nodes, indices };
        let mut pending: VecDeque<_> = (0..analysis.nodes.len()).collect();
        while let Some(index) = pending.pop_front() {
            let node = &analysis.nodes[index];
            let outgoing = node
                .operation
                .and_then(|operation| deferred::transfer(operation, node.expected, node.produced));
            let edges = node.targets;
            let Some(recipe) = outgoing else {
                continue;
            };
            for pc in edges.into_iter().flatten() {
                let Some(&target) = analysis.indices.get(&pc) else {
                    continue;
                };
                let node = &mut analysis.nodes[target];
                if node.conflict {
                    continue;
                }
                match node.expected {
                    None => {
                        node.expected = Some(recipe);
                        pending.push_back(target);
                    }
                    Some(previous) if previous != recipe => {
                        node.expected = None;
                        node.conflict = true;
                        pending.push_back(target);
                    }
                    _ => {}
                }
            }
        }
        let valid = analysis.nodes.iter().all(|node| {
            let outgoing = node
                .operation
                .and_then(|operation| deferred::transfer(operation, node.expected, node.produced));
            node.targets
                .into_iter()
                .flatten()
                .all(|pc| match (outgoing, analysis.expected(pc)) {
                    (Some(source), Some(target)) => source == target,
                    _ => true,
                })
        });
        debug_assert!(
            valid,
            "flag recipe fixed point must cover every pending edge"
        );
        if !valid {
            for node in &mut analysis.nodes {
                node.expected = None;
                node.conflict = true;
            }
        }
        analysis
    }

    pub(super) fn expected(&self, pc: u32) -> Option<deferred::Recipe> {
        self.indices
            .get(&pc)
            .and_then(|&index| self.nodes[index].expected)
    }

    pub(super) fn produced(&self, pc: u32) -> Option<deferred::Recipe> {
        self.indices
            .get(&pc)
            .and_then(|&index| self.nodes[index].produced)
    }

    pub(super) fn after(&self, pc: u32) -> Option<deferred::Recipe> {
        self.indices.get(&pc).and_then(|&index| {
            let node = &self.nodes[index];
            node.operation
                .and_then(|operation| deferred::transfer(operation, node.expected, node.produced))
        })
    }
}
