use super::*;
use crate::cpu::core::trace::{
    BLOCK, CAPACITY, COLD_COMPLETE, CompletedColdObservation, NUMERIC, TraceEvent,
};

mod plan;
pub(crate) use plan::{
    ObservedPlan, ObservedPlanFingerprint, emit_observed_plan, prepare_cold_prefix_plan,
    prepare_observed_plan,
};

#[derive(Clone, Debug, PartialEq)]
pub(super) struct ObservedGraph {
    nodes: BTreeMap<u32, Instruction>,
    edges: BTreeSet<(u32, u32)>,
    ports: BTreeSet<u32>,
    starts: BTreeSet<u32>,
}

impl ObservedGraph {
    fn preserves(&self, previous: &Self) -> bool {
        previous.nodes.iter().all(|(pc, instruction)| {
            self.nodes
                .get(pc)
                .is_some_and(|next| same_instruction(next, instruction))
        }) && previous.edges.is_subset(&self.edges)
            && previous.ports.is_subset(&self.ports)
    }

    pub(super) fn strictly_grows(&self, previous: &Self) -> bool {
        self.preserves(previous)
            && (self.nodes.len() != previous.nodes.len()
                || self.ports != previous.ports
                || self
                    .edges
                    .difference(&previous.edges)
                    .any(|(source, target)| {
                        self.nodes.contains_key(source) && self.nodes.contains_key(target)
                    }))
    }
}

fn same_instruction(left: &Instruction, right: &Instruction) -> bool {
    left.pc == right.pc
        && left.next == right.next
        && left.operation == right.operation
        && left.gate == right.gate
}

fn snapshot_copy(source: &CodeSnapshot) -> CodeSnapshot {
    CodeSnapshot {
        identity: source.identity,
        first: source.first,
        versions: source.versions.clone(),
    }
}

fn decoded_node(memory: &AddressSpace, pc: u32) -> Option<(Instruction, CodeSnapshot)> {
    let BoundDecode::Instruction(decoded) = decode_for_compilation(
        memory,
        GuestAddress(pc),
        DecodePolicy::Execution,
        None,
        DecodeAuthority::default(),
    )
    .ok()?
    else {
        return None;
    };
    if decoded.segment_accesses() & (1 << 5) != 0
        || decoded.is_privileged_instruction()
        || !super::super::emitter::supports(decoded.operation())
    {
        return None;
    }
    let instruction = Instruction {
        pc,
        next: decoded.next_pc().0,
        operation: Some(*decoded.operation()),
        gate: None,
    };
    if !observed_operation(instruction.operation) {
        return None;
    }
    let source = memory
        .snapshot_code_wrapping(decoded.pc(), usize::from(decoded.length()))
        .ok()?;
    Some((instruction, source))
}

fn captured_node(
    memory: &AddressSpace,
    nodes: &mut BTreeMap<u32, Instruction>,
    sources: &mut Vec<CodeSnapshot>,
    pc: u32,
    limit: usize,
) -> Option<Instruction> {
    if let Some(instruction) = nodes.get(&pc) {
        return Some(*instruction);
    }
    if nodes.len() == limit {
        return None;
    }
    let (instruction, source) = decoded_node(memory, pc)?;
    nodes.insert(pc, instruction);
    sources.push(source);
    Some(instruction)
}

enum Site<'a> {
    Retained {
        owner: &'a CompiledCode,
        pcs: &'a [u32],
    },
    Cold(&'a CompletedColdObservation),
}

impl Site<'_> {
    fn first(&self) -> u32 {
        match self {
            Self::Retained { pcs, .. } => pcs[0],
            Self::Cold(observation) => observation.pc(),
        }
    }
}

/// completed cold records carry their own source authority; retained block and
/// numeric records still require the following event to confirm completion.
fn capture_graph(
    memory: &AddressSpace,
    root: GuestAddress,
    options: CompileOptions,
    events: &[TraceEvent],
    owners: &[&CompiledCode],
    expected_source_version: u64,
    cold: &[CompletedColdObservation],
) -> Result<Option<CapturedGraph>, CompileError> {
    if validate_options(options).is_err()
        || events.is_empty()
        || events.len() > CAPACITY
        || cold.len() > CAPACITY
        || memory.code_source_version() != expected_source_version
        || owners.iter().any(|owner| !owner.is_current(memory))
    {
        return Ok(None);
    }
    let retained: BTreeMap<_, _> = owners
        .iter()
        .map(|owner| (owner.identity(), *owner))
        .collect();
    let mut sites = Vec::with_capacity(events.len());
    for event in events {
        let site = match event.kind {
            COLD_COMPLETE if event.owner == 0 => {
                let Some(observation) = cold.get(event.offset as usize) else {
                    return Ok(None);
                };
                if observation.pc() != event.pc
                    || observation.next() != event.end
                    || !memory.is_code_current(&observation.source)
                {
                    return Ok(None);
                }
                Site::Cold(observation)
            }
            BLOCK | NUMERIC if event.owner != 0 => {
                let Some(owner) = retained.get(&event.owner).copied() else {
                    return Ok(None);
                };
                let pcs = if event.kind == BLOCK {
                    let Ok(index) = owner
                        .blocks
                        .binary_search_by_key(&event.pc, |block| block.pc)
                    else {
                        return Ok(None);
                    };
                    let block = &owner.blocks[index];
                    if event.end != block.end || event.offset >= block.count {
                        return Ok(None);
                    }
                    &owner.block_pcs[(block.first + event.offset) as usize
                        ..(block.first + block.count) as usize]
                } else {
                    if event.offset != 0
                        || !owner.native.iter().any(|instruction| {
                            instruction.pc == event.pc && instruction.next == event.end
                        })
                    {
                        return Ok(None);
                    }
                    let Ok(index) = owner.instruction_pcs.binary_search(&event.pc) else {
                        return Ok(None);
                    };
                    &owner.instruction_pcs[index..index + 1]
                };
                Site::Retained { owner, pcs }
            }
            _ => return Ok(None),
        };
        sites.push(site);
    }
    if sites[0].first() != root.0 {
        return Ok(None);
    }
    let mut nodes = BTreeMap::new();
    let mut edges = BTreeSet::new();
    let mut starts = BTreeSet::from([root.0]);
    let mut sources = Vec::new();
    let edge_limit = options.instructions * 2;
    if nodes.len() > options.instructions || edges.len() > edge_limit {
        return Ok(None);
    }

    for (index, site) in sites.iter().enumerate() {
        match site {
            Site::Cold(observation) => {
                if sites
                    .get(index + 1)
                    .is_some_and(|next| observation.actual_post_pc() != next.first())
                {
                    return Ok(None);
                }
                let Some(instruction) = captured_node(
                    memory,
                    &mut nodes,
                    &mut sources,
                    observation.pc(),
                    options.instructions,
                ) else {
                    return Ok(None);
                };
                if instruction.next != observation.next()
                    || instruction.operation != Some(observation.instruction.operation)
                    || !observed_successor(instruction, observation.actual_post_pc())
                {
                    return Ok(None);
                }
                sources.push(snapshot_copy(&observation.source));
                if terminates(instruction.operation) {
                    starts.insert(observation.actual_post_pc());
                }
                edges.insert((instruction.pc, observation.actual_post_pc()));
                if edges.len() > edge_limit {
                    return Ok(None);
                }
            }
            Site::Retained { owner, pcs } => {
                let Some(next_site) = sites.get(index + 1) else {
                    continue;
                };
                let next = &events[index + 1];
                let target = next_site.first();
                let selected = if events[index].kind == BLOCK && next.kind == NUMERIC {
                    if events[index].owner != next.owner {
                        return Ok(None);
                    }
                    let Some(end) = pcs.iter().position(|&pc| pc == next.pc) else {
                        return Ok(None);
                    };
                    &pcs[..end]
                } else {
                    pcs
                };
                starts.insert(pcs[0]);
                starts.insert(target);
                for (position, &pc) in selected.iter().enumerate() {
                    let Some(instruction) =
                        captured_node(memory, &mut nodes, &mut sources, pc, options.instructions)
                    else {
                        return Ok(None);
                    };
                    let successor = selected.get(position + 1).copied().unwrap_or(target);
                    if position + 1 < selected.len() {
                        if instruction.next != successor || terminates(instruction.operation) {
                            return Ok(None);
                        }
                    } else if !observed_successor(instruction, successor) {
                        return Ok(None);
                    }
                    if events[index].kind == NUMERIC
                        && !owner.native.iter().any(|native| {
                            native.pc == pc
                                && native.next == instruction.next
                                && Some(native.operation) == instruction.operation
                        })
                    {
                        return Ok(None);
                    }
                    edges.insert((pc, successor));
                    if edges.len() > edge_limit {
                        return Ok(None);
                    }
                }
            }
        }
        if nodes.len() > options.instructions || edges.len() > edge_limit {
            return Ok(None);
        }
    }
    if nodes.is_empty() || !nodes.contains_key(&root.0) {
        return Ok(None);
    }
    let ports: BTreeSet<_> = nodes.keys().copied().collect();
    let graph = ObservedGraph {
        nodes,
        edges,
        ports,
        starts,
    };
    Ok(Some(CapturedGraph { graph, sources }))
}

struct CapturedGraph {
    graph: ObservedGraph,
    sources: Vec<CodeSnapshot>,
}

pub(crate) fn compile_observed_with_cold(
    memory: &AddressSpace,
    root: GuestAddress,
    options: CompileOptions,
    events: &[TraceEvent],
    owners: &[&CompiledCode],
    expected_source_version: u64,
    cold: &[CompletedColdObservation],
    previous: Option<&CompiledCode>,
) -> Result<Option<CompiledCode>, CompileError> {
    let parents: Vec<_> = previous.into_iter().collect();
    let Some(plan) = prepare_observed_plan(
        memory,
        root,
        options,
        events,
        owners,
        expected_source_version,
        cold,
        &parents,
    )?
    else {
        return Ok(None);
    };
    emit_observed_plan(memory, plan)
}

#[cfg(test)]
mod tests;
