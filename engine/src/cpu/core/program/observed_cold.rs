use super::*;
use crate::cpu::core::trace::{
    BLOCK, CAPACITY, COLD_COMPLETE, CompletedColdObservation, NUMERIC, TraceEvent,
};

#[derive(Debug)]
pub(super) struct ObservedGraph {
    nodes: BTreeMap<u32, Instruction>,
    edges: BTreeSet<(u32, u32)>,
    ports: BTreeSet<u32>,
    starts: BTreeSet<u32>,
}

impl ObservedGraph {
    pub(super) fn strictly_grows(&self, previous: &Self) -> bool {
        previous.nodes.iter().all(|(pc, instruction)| {
            self.nodes
                .get(pc)
                .is_some_and(|next| same_instruction(next, instruction))
        }) && previous.edges.is_subset(&self.edges)
            && previous.ports.is_subset(&self.ports)
            && (self.nodes.len() != previous.nodes.len()
                || self.edges != previous.edges
                || self.ports != previous.ports)
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
    if validate_options(options).is_err()
        || events.is_empty()
        || events.len() > CAPACITY
        || cold.len() > CAPACITY
        || memory.code_source_version() != expected_source_version
        || owners.iter().any(|owner| !owner.is_current(memory))
        || previous.is_some_and(|owner| !owner.is_observed_family() || !owner.is_current(memory))
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
    let mut nodes = previous
        .and_then(|owner| owner.observed.as_ref())
        .map_or_else(BTreeMap::new, |graph| graph.nodes.clone());
    let mut edges = previous
        .and_then(|owner| owner.observed.as_ref())
        .map_or_else(BTreeSet::new, |graph| graph.edges.clone());
    let mut starts = previous
        .and_then(|owner| owner.observed.as_ref())
        .map_or_else(BTreeSet::new, |graph| graph.starts.clone());
    starts.insert(root.0);
    let mut sources = previous.map_or_else(Vec::new, |owner| {
        owner.sources.iter().map(snapshot_copy).collect()
    });
    if nodes.len() > options.instructions {
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
                let Some((instruction, source)) = decoded_node(memory, observation.pc()) else {
                    return Ok(None);
                };
                if instruction.next != observation.next()
                    || instruction.operation != Some(observation.instruction.operation)
                    || !observed_successor(instruction, observation.actual_post_pc())
                    || nodes
                        .get(&instruction.pc)
                        .is_some_and(|old| !same_instruction(old, &instruction))
                {
                    return Ok(None);
                }
                nodes.insert(instruction.pc, instruction);
                sources.push(source);
                sources.push(snapshot_copy(&observation.source));
                if terminates(instruction.operation) {
                    starts.insert(observation.actual_post_pc());
                }
                edges.insert((instruction.pc, observation.actual_post_pc()));
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
                    let Some((instruction, source)) = decoded_node(memory, pc) else {
                        return Ok(None);
                    };
                    if nodes
                        .get(&pc)
                        .is_some_and(|old| !same_instruction(old, &instruction))
                    {
                        return Ok(None);
                    }
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
                    nodes.insert(pc, instruction);
                    sources.push(source);
                    edges.insert((pc, successor));
                }
            }
        }
        if nodes.len() > options.instructions {
            return Ok(None);
        }
    }
    if nodes.is_empty() || !nodes.contains_key(&root.0) {
        return Ok(None);
    }
    let anchor = previous
        .and_then(CompiledCode::trace_root)
        .unwrap_or(root.0);
    let ports: BTreeSet<_> = nodes.keys().copied().collect();
    let graph = ObservedGraph {
        nodes,
        edges,
        ports,
        starts,
    };
    if previous
        .and_then(|owner| owner.observed.as_ref())
        .is_some_and(|old| !graph.strictly_grows(old))
    {
        return Ok(None);
    }
    // ports are independently decoded starts, not extra discovery blocks.
    let blocks = partition_blocks(graph.nodes.clone(), &graph.starts);
    if blocks.len() > options.blocks {
        return Ok(None);
    }
    let native = collect_native(&blocks);
    let score = TraceScore::emitted(&blocks, &graph.edges);
    let identity = next_identity()?;
    let observed_edges: Vec<_> = graph.edges.iter().copied().collect();
    let wasm = super::super::emitter::emit_observed_family(
        &blocks,
        anchor,
        identity,
        &native,
        &observed_edges,
    );
    if wasm.len() > options.wasm_bytes
        || memory.code_source_version() != expected_source_version
        || !sources.iter().all(|source| memory.is_code_current(source))
        || owners.iter().any(|owner| !owner.is_current(memory))
        || previous.is_some_and(|owner| !owner.is_current(memory))
    {
        return Ok(None);
    }
    let mut code = finish_code(
        memory,
        blocks,
        identity,
        wasm,
        native,
        sources,
        &[GuestAddress(anchor)],
        Some((anchor, score)),
    );
    code.entries = graph.ports.iter().copied().map(GuestAddress).collect();
    code.observed = Some(graph);
    Ok(Some(code))
}

#[cfg(test)]
mod tests;
