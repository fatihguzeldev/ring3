use super::*;
use crate::cpu::core::trace::RecordedIngress;

/// one source-certified candidate; recurrence policy and context are deliberately
/// separate from the immutable executor's semantic fingerprint.
pub(crate) struct ExecutionCandidate {
    plan: WindowPlan,
    origin: Option<u32>,
    closed: bool,
    completed_work: usize,
}

impl ExecutionCandidate {
    pub(crate) fn plan(&self) -> &WindowPlan {
        &self.plan
    }
    pub(crate) fn into_plan(self) -> WindowPlan {
        self.plan
    }
    pub(crate) fn origin(&self) -> Option<u32> {
        self.origin
    }
    pub(crate) fn is_closed_cycle(&self) -> bool {
        self.closed
    }
    pub(crate) fn completed_work(&self) -> usize {
        self.completed_work
    }
}

/// geometry is decoded once for the bounded capture. prefix construction only
/// slices certified transitions and their exact per-instruction source proofs.
/// the visitor retains neither every prefix nor any cross-capture path archive.
pub(crate) fn for_each_execution_candidate(
    memory: &AddressSpace,
    root: GuestAddress,
    options: CompileOptions,
    events: &[TraceEvent],
    owners: &[&CompiledCode],
    expected_source_version: u64,
    cold: &[CompletedColdObservation],
    ingresses: &[RecordedIngress],
    visitor: &mut impl FnMut(ExecutionCandidate),
) -> Result<(), CompileError> {
    if validate_options(options).is_err()
        || events.is_empty()
        || events.len() > CAPACITY
        || cold.len() > CAPACITY
        || ingresses.len() > CAPACITY
    {
        return Ok(());
    }
    let Some(sites) = recorded_sites(memory, events, owners, cold) else {
        return Ok(());
    };
    if sites.is_empty()
        || sites[0].first() != root.0
        || ingresses
            .windows(2)
            .any(|pair| pair[0].event_index >= pair[1].event_index)
        || ingresses.iter().any(|ingress| {
            sites.get(ingress.event_index).is_none_or(|site| {
                site.owner() != ingress.owner
                    || ingress.owner == 0
                    || site.first() != ingress.target
            })
        })
    {
        return Ok(());
    }
    let limit = options.instructions.saturating_mul(2).min(16384);
    let walk_options = CompileOptions {
        instructions: limit,
        ..options
    };
    let mut steps = Vec::new();
    if capture_graph_walk(
        memory,
        root,
        walk_options,
        events,
        owners,
        expected_source_version,
        cold,
        true,
        &mut |step| {
            steps.push(step);
            if steps.len() == limit {
                CaptureFlow::Stop
            } else {
                CaptureFlow::Continue
            }
        },
    )?
    .is_none()
    {
        return Ok(());
    }
    let mut sources = BTreeMap::new();
    for step in &steps {
        if let std::collections::btree_map::Entry::Vacant(entry) =
            sources.entry(step.instruction.pc)
        {
            let length = step.instruction.next.wrapping_sub(step.instruction.pc);
            if !(1..=15).contains(&length) {
                return Ok(());
            }
            let Ok(source) =
                memory.snapshot_code_wrapping(GuestAddress(step.instruction.pc), length as usize)
            else {
                return Ok(());
            };
            entry.insert(source);
        }
    }
    if memory.code_source_version() != expected_source_version {
        return Ok(());
    }
    for ingress in ingresses {
        let Some(start) = steps
            .iter()
            .position(|step| step.site_index >= ingress.event_index)
        else {
            continue;
        };
        let first = steps[start];
        if first.instruction.pc != ingress.target
            || first.owner != ingress.owner
            || events[ingress.event_index..first.site_index]
                .iter()
                .any(|event| event.kind == COLD_COMPLETE)
        {
            continue;
        }
        // a cold event breaks the compiled word; empty BLOCK/NUMERIC markers
        // do not confirm another completed span.
        let mut end = start;
        while end < steps.len() {
            if end != start {
                let previous = steps[end - 1];
                let next = steps[end];
                if previous.successor != next.instruction.pc
                    || previous.successor_owner != next.owner
                    || (previous.site_index != next.site_index
                        && events[previous.site_index + 1..next.site_index]
                            .iter()
                            .any(|event| event.kind == COLD_COMPLETE))
                {
                    break;
                }
            }
            end += 1;
        }
        let segment = &steps[start..end];
        let mut recurrence = RootCycle::new(ingress.target, options.instructions);
        for &step in segment {
            if matches!(recurrence.observe(step), CaptureFlow::Stop) {
                break;
            }
        }
        if recurrence.completed == 2 {
            if let Some(plan) = prefix_plan(memory, options, &recurrence.word, &sources, true, 0) {
                visitor(ExecutionCandidate {
                    plan,
                    origin: ingress.origin,
                    closed: true,
                    completed_work: recurrence.word.len() * 2,
                });
            }
        }
        let mut frontiers = Vec::new();
        for (position, step) in segment.iter().enumerate().take(options.instructions) {
            if step.ends_site {
                frontiers.push(position + 1);
            }
        }
        let count = frontiers.len();
        let mut lengths = Vec::new();
        let mut spans = 1usize;
        while spans <= count {
            lengths.push(spans);
            spans *= 2;
        }
        if count != 0 && !count.is_power_of_two() {
            lengths.push(count);
        }
        // all keys are observed, but longest-first lets the selector avoid
        // repeated residency scans for prefixes that cannot improve its best.
        lengths.sort_unstable_by(|left, right| right.cmp(left));
        for spans in lengths {
            let work = frontiers[spans - 1];
            if let Some(plan) =
                prefix_plan(memory, options, &segment[..work], &sources, false, spans)
            {
                visitor(ExecutionCandidate {
                    plan,
                    origin: ingress.origin,
                    closed: false,
                    completed_work: work,
                });
            }
        }
    }
    Ok(())
}

fn prefix_plan(
    memory: &AddressSpace,
    options: CompileOptions,
    word: &[CertifiedTransition],
    source_by_pc: &BTreeMap<u32, CodeSnapshot>,
    closed: bool,
    spans: usize,
) -> Option<WindowPlan> {
    let first = word.first()?;
    let mut graph = ObservedGraph {
        nodes: BTreeMap::new(),
        edges: BTreeSet::new(),
        ports: BTreeSet::new(),
        starts: BTreeSet::from([first.instruction.pc]),
    };
    let mut transfers = 0;
    for (index, step) in word.iter().enumerate() {
        graph.nodes.insert(step.instruction.pc, step.instruction);
        if step.begins_site {
            graph.starts.insert(step.instruction.pc);
        }
        // the last observed dynamic successor stays outside an open-prefix
        // identity; its operation still exits using the actual runtime EIP.
        if index + 1 < word.len() || closed {
            graph.edges.insert((step.instruction.pc, step.successor));
            transfers += usize::from(step.owner != step.successor_owner);
            if terminates(step.instruction.operation) {
                graph.starts.insert(step.successor);
            }
        }
    }
    if transfers == 0 || graph.nodes.len() > options.instructions {
        return None;
    }
    graph.ports.extend(graph.nodes.keys().copied());
    let sources = graph
        .nodes
        .keys()
        .map(|pc| source_by_pc.get(pc).map(snapshot_copy))
        .collect::<Option<Vec<_>>>()?;
    let anchor = first.instruction.pc;
    let ObservedPlan {
        fingerprint,
        sources,
        blocks,
        ..
    } = form_plan(memory, anchor, options, graph, sources, Vec::new())?;
    let ports = executable_hot_spans(&blocks);
    if !ports.iter().any(|port| port.entry == anchor) {
        return None;
    }
    let entries = ports.iter().map(|port| GuestAddress(port.entry)).collect();
    Some(WindowPlan {
        fingerprint: WindowFingerprint {
            memory_identity: fingerprint.memory_identity,
            anchor,
            options,
            graph: fingerprint.graph,
            sources: fingerprint.sources,
            ports,
        },
        sources,
        blocks,
        entries,
        cross_owner_edges: transfers,
        completed_cycles: if closed { 2 } else { 0 },
        cycle_instructions: if closed { word.len() } else { 0 },
        prefix_spans: spans,
    })
}

#[cfg(test)]
mod tests;
