use super::*;
use std::collections::{BTreeMap, VecDeque};

pub(super) struct Analysis {
    nodes: Vec<Node>,
    indices: BTreeMap<u32, usize>,
}

struct Node {
    pc: u32,
    operation: Option<Operation>,
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

impl Analysis {
    pub(super) fn new(blocks: &[Block], entries: &[EntryPoint]) -> Self {
        let nodes: Vec<_> = entries
            .iter()
            .map(|entry| {
                let instruction = blocks[entry.block as usize]
                    .instructions
                    .get(entry.instruction as usize);
                Node {
                    pc: entry.pc,
                    operation: instruction.map(|instruction| *instruction.operation()),
                    targets: instruction.map_or([None, None], |instruction| {
                        targets(*instruction.operation(), instruction.next_pc().0)
                    }),
                    expected: None,
                    conflict: false,
                }
            })
            .collect();
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
                .and_then(|operation| deferred::transfer(operation, node.expected));
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
                .and_then(|operation| deferred::transfer(operation, node.expected));
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

    pub(super) fn after(&self, pc: u32) -> Option<deferred::Recipe> {
        self.indices.get(&pc).and_then(|&index| {
            let node = &self.nodes[index];
            node.operation
                .and_then(|operation| deferred::transfer(operation, node.expected))
        })
    }
}
