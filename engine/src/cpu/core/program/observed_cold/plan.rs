use super::*;

/// Exact, bounded admission input. Equality never relies on a lossy hash or on
/// the global validation epoch, which also changes for unrelated mappings.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ObservedPlanFingerprint {
    memory_identity: u64,
    anchor: u32,
    options: CompileOptions,
    graph: Arc<ObservedHistory>,
    sources: Vec<SourceAuthority>,
    parents: Vec<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct SourceAuthority {
    identity: u64,
    first: usize,
    versions: Vec<(u64, u64)>,
}

impl SourceAuthority {
    fn of(source: &CodeSnapshot) -> Self {
        Self {
            identity: source.identity,
            first: source.first,
            versions: source
                .versions
                .iter()
                .map(|version| (version.mapping, version.content))
                .collect(),
        }
    }
}

/// Formation stops before value analysis, identity allocation and Wasm encoding.
/// The cache can reject collisions and neutral recaptures using this value alone.
pub(crate) struct ObservedPlan {
    fingerprint: ObservedPlanFingerprint,
    sources: Vec<CodeSnapshot>,
    blocks: Vec<Block>,
    entries: Vec<GuestAddress>,
}

impl ObservedPlanFingerprint {
    pub(crate) fn metadata_charge(&self) -> usize {
        self.sources.capacity() * std::mem::size_of::<SourceAuthority>()
            + self
                .sources
                .iter()
                .map(|source| source.versions.capacity() * std::mem::size_of::<(u64, u64)>())
                .sum::<usize>()
            + self.parents.capacity() * std::mem::size_of::<u64>()
    }

    /// Refusal memo accounting deliberately charges the full Arc payload per
    /// entry, even when another artifact or memo shares that immutable history.
    pub(crate) fn resident_charge(&self) -> usize {
        std::mem::size_of::<Self>() + self.metadata_charge() + self.graph.resident_charge()
    }
}

impl ObservedPlan {
    pub(crate) fn anchor(&self) -> u32 {
        self.fingerprint.anchor
    }
    pub(crate) fn instruction_pcs(&self) -> impl Iterator<Item = u32> + '_ {
        self.fingerprint.graph.nodes.iter().map(|(pc, _)| *pc)
    }
    pub(crate) fn entrypoints(&self) -> &[GuestAddress] {
        &self.entries
    }
    pub(crate) fn parent_identities(&self) -> &[u64] {
        &self.fingerprint.parents
    }
    pub(crate) fn fingerprint(&self) -> &ObservedPlanFingerprint {
        &self.fingerprint
    }
    pub(crate) fn has_executable_gain(&self) -> bool {
        true
    }
    pub(crate) fn is_current(&self, memory: &AddressSpace) -> bool {
        memory.identity() == self.fingerprint.memory_identity
            && self
                .sources
                .iter()
                .all(|source| memory.is_code_current(source))
    }
    pub(crate) fn preserves(&self, previous: &CompiledCode) -> bool {
        self.fingerprint.memory_identity == previous.memory_identity
            && self
                .fingerprint
                .parents
                .binary_search(&previous.identity())
                .is_ok()
            && previous
                .observed
                .as_ref()
                .is_some_and(|graph| self.fingerprint.graph.preserves(graph))
    }
}

fn form_plan(
    memory: &AddressSpace,
    anchor: u32,
    options: CompileOptions,
    graph: ObservedGraph,
    sources: Vec<CodeSnapshot>,
    parents: Vec<u64>,
) -> Option<ObservedPlan> {
    if graph.nodes.is_empty()
        || !graph.nodes.contains_key(&anchor)
        || graph.nodes.len() > options.instructions
        || graph.edges.len() > options.instructions * 2
        || sources.iter().any(|source| !memory.is_code_current(source))
    {
        return None;
    }
    let blocks = partition_blocks(graph.nodes.clone(), &graph.starts);
    if blocks.len() > options.blocks {
        return None;
    }
    let mut normalized: Vec<_> = sources
        .into_iter()
        .map(|source| (SourceAuthority::of(&source), source))
        .collect();
    normalized.sort_unstable_by(|left, right| left.0.cmp(&right.0));
    normalized.dedup_by(|left, right| left.0 == right.0);
    let entries = graph.ports.iter().copied().map(GuestAddress).collect();
    let (authorities, sources) = normalized.into_iter().unzip();
    Some(ObservedPlan {
        fingerprint: ObservedPlanFingerprint {
            memory_identity: memory.identity(),
            anchor,
            options,
            graph: Arc::new(graph.freeze()),
            sources: authorities,
            parents,
        },
        sources,
        blocks,
        entries,
    })
}

pub(crate) fn prepare_observed_plan(
    memory: &AddressSpace,
    root: GuestAddress,
    options: CompileOptions,
    events: &[TraceEvent],
    owners: &[&CompiledCode],
    expected_source_version: u64,
    cold: &[CompletedColdObservation],
    parents: &[&CompiledCode],
) -> Result<Option<ObservedPlan>, CompileError> {
    let Some(CapturedGraph {
        mut graph,
        mut sources,
    }) = capture_graph(
        memory,
        root,
        options,
        events,
        owners,
        expected_source_version,
        cold,
    )?
    else {
        return Ok(None);
    };

    // Membership comes from this completed timeline, never from the set of
    // owners merely held during selection. A final retained entry can witness
    // an actual edge into its already source-validated historical graph.
    let mut connected: Vec<_> = parents
        .iter()
        .copied()
        .filter(|parent| {
            parent.is_observed_family()
                && parent.is_current(memory)
                && parent.observed.as_ref().is_some_and(|old| {
                    graph.nodes.keys().any(|pc| old.contains_node(*pc))
                        || graph
                            .edges
                            .iter()
                            .any(|(_, target)| old.ports.binary_search(target).is_ok())
                        || old
                            .edges
                            .iter()
                            .any(|(_, target)| graph.nodes.contains_key(target))
                })
        })
        .collect();
    connected.sort_unstable_by_key(|parent| parent.identity());
    connected.dedup_by_key(|parent| parent.identity());
    let anchor = connected
        .first()
        .and_then(|parent| parent.trace_root())
        .unwrap_or(root.0);
    for parent in &connected {
        let previous = parent.observed.as_ref().expect("connected observed family");
        for &(pc, instruction) in previous.nodes.iter() {
            if graph
                .nodes
                .get(&pc)
                .is_some_and(|old| !same_instruction(old, &instruction))
            {
                return Ok(None);
            }
            graph.nodes.insert(pc, instruction);
            if graph.nodes.len() > options.instructions {
                return Ok(None);
            }
        }
        for &edge in previous.edges.iter() {
            graph.edges.insert(edge);
            if graph.edges.len() > options.instructions * 2 {
                return Ok(None);
            }
        }
        graph.ports.extend(previous.ports.iter().copied());
        graph.starts.extend(previous.starts.iter().copied());
        sources.extend(parent.sources.iter().map(snapshot_copy));
    }
    if connected.len() == 1
        && !graph.strictly_grows(connected[0].observed.as_ref().expect("observed family"))
    {
        return Ok(None);
    }
    let parents = connected.iter().map(|parent| parent.identity()).collect();
    Ok(form_plan(memory, anchor, options, graph, sources, parents))
}

pub(crate) fn prepare_cold_prefix_plan(
    memory: &AddressSpace,
    options: CompileOptions,
    events: &[TraceEvent],
    cold: &[CompletedColdObservation],
    frontier: &dyn Fn(u32) -> bool,
) -> Result<Option<ObservedPlan>, CompileError> {
    if validate_options(options).is_err() || events.len() > CAPACITY || cold.len() > CAPACITY {
        return Ok(None);
    }
    let mut accepted = None;
    let mut graph = ObservedGraph {
        nodes: BTreeMap::new(),
        edges: BTreeSet::new(),
        ports: BTreeSet::new(),
        starts: BTreeSet::new(),
    };
    let mut sources = Vec::new();
    let mut anchor = None;
    let mut expected = None;
    for event in events {
        if anchor.is_none() {
            if event.kind != COLD_COMPLETE || event.owner != 0 || frontier(event.pc) {
                continue;
            }
            anchor = Some(event.pc);
            graph.starts.insert(event.pc);
        }
        if event.kind != COLD_COMPLETE || event.owner != 0 || frontier(event.pc) {
            break;
        }
        let Some(observation) = cold.get(event.offset as usize) else {
            return Ok(None);
        };
        if observation.pc() != event.pc
            || observation.next() != event.end
            || expected.is_some_and(|pc| pc != event.pc)
            || !memory.is_code_current(&observation.source)
        {
            return Ok(None);
        }
        let Some((instruction, source)) = decoded_node(memory, event.pc) else {
            return Ok(None);
        };
        if instruction.next != event.end
            || instruction.operation != Some(observation.instruction.operation)
            || !observed_successor(instruction, observation.actual_post_pc())
            || graph
                .nodes
                .get(&event.pc)
                .is_some_and(|old| !same_instruction(old, &instruction))
        {
            return Ok(None);
        }
        graph.nodes.insert(event.pc, instruction);
        graph.ports.insert(event.pc);
        graph.edges.insert((event.pc, observation.actual_post_pc()));
        if terminates(instruction.operation) {
            graph.starts.insert(observation.actual_post_pc());
        }
        sources.push(source);
        sources.push(snapshot_copy(&observation.source));
        let Some(plan) = form_plan(
            memory,
            anchor.expect("cold anchor"),
            options,
            graph.clone(),
            sources.iter().map(snapshot_copy).collect(),
            Vec::new(),
        ) else {
            break;
        };
        accepted = Some(plan);
        expected = Some(observation.actual_post_pc());
        if frontier(observation.actual_post_pc()) {
            break;
        }
    }
    Ok(accepted)
}

pub(crate) fn emit_observed_plan(
    memory: &AddressSpace,
    plan: ObservedPlan,
) -> Result<Option<CompiledCode>, CompileError> {
    if !plan.is_current(memory) {
        return Ok(None);
    }
    let ObservedPlan {
        fingerprint,
        sources,
        blocks,
        entries,
    } = plan;
    let native = collect_native(&blocks);
    let score = TraceScore {
        instructions: fingerprint.graph.nodes.len(),
        internal_edges: fingerprint
            .graph
            .edges
            .iter()
            .filter(|(source, target)| {
                fingerprint.graph.contains_node(*source) && fingerprint.graph.contains_node(*target)
            })
            .count(),
    };
    let identity = next_identity()?;
    let wasm = super::super::super::emitter::emit_observed_family(
        &blocks,
        fingerprint.anchor,
        identity,
        &native,
        &fingerprint.graph.edges,
    );
    if wasm.len() > fingerprint.options.wasm_bytes
        || sources.iter().any(|source| !memory.is_code_current(source))
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
        &[GuestAddress(fingerprint.anchor)],
        Some((fingerprint.anchor, score)),
    );
    code.entries = entries;
    code.observed = Some(Arc::clone(&fingerprint.graph));
    code.observed_fingerprint = Some(fingerprint);
    Ok(Some(code))
}

#[cfg(test)]
mod tests;
