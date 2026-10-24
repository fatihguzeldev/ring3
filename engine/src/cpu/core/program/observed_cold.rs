use super::*;
use crate::cpu::core::trace::{
    BLOCK, CAPACITY, COLD_COMPLETE, CompletedColdObservation, NUMERIC, TraceEvent,
};

mod plan;
pub(crate) use plan::{
    ObservedPlan, ObservedPlanFingerprint, WindowFingerprint, WindowPlan, emit_hot_window,
    emit_observed_plan, prepare_cold_prefix_plan, prepare_hot_window, prepare_observed_plan,
};

#[derive(Clone, Debug, PartialEq)]
pub(super) struct ObservedGraph {
    nodes: BTreeMap<u32, Instruction>,
    edges: BTreeSet<(u32, u32)>,
    ports: BTreeSet<u32>,
    starts: BTreeSet<u32>,
}

impl ObservedGraph {
    fn preserves(&self, previous: &ObservedHistory) -> bool {
        previous.nodes.iter().all(|(pc, instruction)| {
            self.nodes
                .get(pc)
                .is_some_and(|next| same_instruction(next, instruction))
        }) && previous.edges.iter().all(|edge| self.edges.contains(edge))
            && previous.ports.iter().all(|pc| self.ports.contains(pc))
    }

    fn strictly_grows(&self, previous: &ObservedHistory) -> bool {
        self.preserves(previous)
            && (self.nodes.len() != previous.nodes.len()
                || self.ports.len() != previous.ports.len()
                || self.edges.iter().any(|(source, target)| {
                    previous.edges.binary_search(&(*source, *target)).is_err()
                        && self.nodes.contains_key(source)
                        && self.nodes.contains_key(target)
                }))
    }

    fn freeze(self) -> ObservedHistory {
        ObservedHistory {
            nodes: self
                .nodes
                .into_iter()
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            edges: self
                .edges
                .into_iter()
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            ports: self
                .ports
                .into_iter()
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            starts: self
                .starts
                .into_iter()
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        }
    }
}

/// Source-current immutable history is stored once, independent of fingerprint
/// clones. Sorted boxed payloads have explicit storage sizes and no tree slack.
#[derive(Debug, PartialEq)]
pub(super) struct ObservedHistory {
    nodes: Box<[(u32, Instruction)]>,
    edges: Box<[(u32, u32)]>,
    ports: Box<[u32]>,
    starts: Box<[u32]>,
}

impl ObservedHistory {
    fn contains_node(&self, pc: u32) -> bool {
        self.nodes.binary_search_by_key(&pc, |(pc, _)| *pc).is_ok()
    }

    fn preserves(&self, previous: &Self) -> bool {
        previous.nodes.iter().all(|(pc, instruction)| {
            self.nodes
                .binary_search_by_key(pc, |(pc, _)| *pc)
                .ok()
                .is_some_and(|index| same_instruction(&self.nodes[index].1, instruction))
        }) && previous
            .edges
            .iter()
            .all(|edge| self.edges.binary_search(edge).is_ok())
            && previous
                .ports
                .iter()
                .all(|pc| self.ports.binary_search(pc).is_ok())
    }

    pub(super) fn strictly_grows(&self, previous: &Self) -> bool {
        self.preserves(previous)
            && (self.nodes.len() != previous.nodes.len()
                || self.ports.len() != previous.ports.len()
                || self.edges.iter().any(|edge| {
                    previous.edges.binary_search(edge).is_err()
                        && self.contains_node(edge.0)
                        && self.contains_node(edge.1)
                }))
    }

    pub(super) fn resident_charge(&self) -> usize {
        std::mem::size_of::<Self>()
            + 2 * std::mem::size_of::<usize>()
            + std::mem::align_of::<Self>()
            - 1
            + std::mem::size_of_val(self.nodes.as_ref())
            + std::mem::size_of_val(self.edges.as_ref())
            + std::mem::size_of_val(self.ports.as_ref())
            + std::mem::size_of_val(self.starts.as_ref())
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

    fn owner(&self) -> u64 {
        match self {
            Self::Retained { owner, .. } => owner.identity(),
            Self::Cold(_) => 0,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct CertifiedTransition {
    instruction: Instruction,
    successor: u32,
    owner: u64,
    successor_owner: u64,
    begins_site: bool,
}

enum CaptureFlow {
    Continue,
    Stop,
}

// Geometry and source authority are checked once before any subwalk selection.
// A candidate still goes through the same instruction/successor certifier below.
fn recorded_sites<'a>(
    memory: &AddressSpace,
    events: &[TraceEvent],
    owners: &[&'a CompiledCode],
    cold: &'a [CompletedColdObservation],
) -> Option<Vec<Site<'a>>> {
    let retained: BTreeMap<_, _> = owners
        .iter()
        .map(|owner| (owner.identity(), *owner))
        .collect();
    let mut sites = Vec::with_capacity(events.len());
    for event in events {
        let site = match event.kind {
            COLD_COMPLETE if event.owner == 0 => {
                let Some(observation) = cold.get(event.offset as usize) else {
                    return None;
                };
                if observation.pc() != event.pc
                    || observation.next() != event.end
                    || !memory.is_code_current(&observation.source)
                {
                    return None;
                }
                Site::Cold(observation)
            }
            BLOCK | NUMERIC if event.owner != 0 => {
                let Some(owner) = retained.get(&event.owner).copied() else {
                    return None;
                };
                let pcs = if event.kind == BLOCK {
                    let Ok(index) = owner
                        .blocks
                        .binary_search_by_key(&event.pc, |block| block.pc)
                    else {
                        return None;
                    };
                    let block = &owner.blocks[index];
                    if event.end != block.end || event.offset >= block.count {
                        return None;
                    }
                    &owner.block_pcs[(block.first + event.offset) as usize
                        ..(block.first + block.count) as usize]
                } else {
                    if event.offset != 0
                        || !owner.native.iter().any(|instruction| {
                            instruction.pc == event.pc && instruction.next == event.end
                        })
                    {
                        return None;
                    }
                    let Ok(index) = owner.instruction_pcs.binary_search(&event.pc) else {
                        return None;
                    };
                    &owner.instruction_pcs[index..index + 1]
                };
                Site::Retained { owner, pcs }
            }
            _ => return None,
        };
        sites.push(site);
    }
    for pair in sites.windows(2) {
        if let Site::Cold(observation) = &pair[0] {
            if observation.actual_post_pc() != pair[1].first() {
                return None;
            }
        }
    }
    Some(sites)
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
    capture_graph_with(
        memory,
        root,
        options,
        events,
        owners,
        expected_source_version,
        cold,
        &mut |_| CaptureFlow::Continue,
    )
}

fn capture_graph_with(
    memory: &AddressSpace,
    root: GuestAddress,
    options: CompileOptions,
    events: &[TraceEvent],
    owners: &[&CompiledCode],
    expected_source_version: u64,
    cold: &[CompletedColdObservation],
    transition: &mut impl FnMut(CertifiedTransition) -> CaptureFlow,
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
    let Some(sites) = recorded_sites(memory, events, owners, cold) else {
        return Ok(None);
    };
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

    'timeline: for (index, site) in sites.iter().enumerate() {
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
                if matches!(
                    transition(CertifiedTransition {
                        instruction,
                        successor: observation.actual_post_pc(),
                        owner: 0,
                        successor_owner: sites.get(index + 1).map_or(0, Site::owner),
                        begins_site: true,
                    }),
                    CaptureFlow::Stop
                ) {
                    break 'timeline;
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
                } else if events[index].kind == BLOCK
                    && next.kind == BLOCK
                    && events[index].owner == next.owner
                    && events[index].pc == next.pc
                    && events[index].end == next.end
                    && events[index].offset < next.offset
                {
                    // Executable spans retain the original source-block
                    // geometry. A later span confirms only the prefix before
                    // its actual first instruction, not the old whole suffix.
                    let Some(end) = pcs.iter().position(|&pc| pc == target) else {
                        return Ok(None);
                    };
                    &pcs[..end]
                } else if events[index].kind == BLOCK && matches!(next_site, Site::Cold(_)) {
                    // A precise before-op exit can hand off at this very
                    // instruction (an empty compiled prefix). Only an
                    // authenticated completed cold record licenses this cut.
                    if let Some(end) = pcs.iter().position(|&pc| pc == target) {
                        &pcs[..end]
                    } else {
                        pcs
                    }
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
                    if matches!(
                        transition(CertifiedTransition {
                            instruction,
                            successor,
                            owner: owner.identity(),
                            successor_owner: if position + 1 < selected.len() {
                                owner.identity()
                            } else {
                                next_site.owner()
                            },
                            begins_site: position == 0,
                        }),
                        CaptureFlow::Stop
                    ) {
                        break 'timeline;
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

#[cfg(test)]
mod span_tests;
