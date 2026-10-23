use super::*;
use crate::cpu::core::CompileOptions;

#[derive(Default)]
pub(super) struct TransitionHeat {
    edges: [TransitionEdge; 4],
    rejected: [Option<TransitionRejection>; 4],
    ineligible: Option<IneligibleTransition>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct IneligibleTransition {
    target: u64,
    pc: u32,
}

#[derive(Clone, Copy, Default)]
struct TransitionEdge {
    target: u64,
    target_index: usize,
    pc: u32,
    hits: u32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct TransitionRejection {
    version: u64,
    instructions: usize,
    blocks: usize,
    wasm_bytes: usize,
    source_entries: usize,
    target_entries: usize,
}

pub(super) struct PendingTransition {
    source: RegionKey,
    target: RegionKey,
    pc: u32,
    memory: u64,
    version: u64,
}

impl CoreCache32 {
    pub(in crate::process) fn transition_pending(&self) -> bool {
        self.pending_transition.is_some()
    }

    #[cfg(test)]
    pub(in crate::process) fn observe_transition(
        &mut self,
        memory: &AddressSpace,
        source: &RegionRef,
        target: &RegionRef,
        pc: u32,
        threshold: u32,
    ) -> bool {
        if threshold == 0
            || self.transition_pending()
            || !self.linked_current(memory, source)
            || !self.linked_current(memory, target)
            || !self.indexes(target, pc)
        {
            return false;
        }
        self.record_transition(memory, source, target, pc, threshold)
    }

    pub(in crate::process) fn lookup_transition(
        &mut self,
        memory: &AddressSpace,
        source: &RegionRef,
        pc: u32,
        threshold: u32,
    ) -> TransitionLookup {
        if self.transition_pending() || !source.is_current(memory) {
            return TransitionLookup::Stop;
        }
        let Some((target, slot)) = self.lookup_linked_from(memory, source, pc) else {
            return TransitionLookup::Miss;
        };
        // The lookup just validated the target's source, index and reciprocal link.
        // No guest execution or cache mutation can intervene in this borrow.
        if threshold != 0
            && source.identity() != target.identity()
            && self
                .retained(source)
                .is_some_and(|entry| self.reciprocal_slot(source.key, entry))
            && self.record_transition(memory, source, &target, pc, threshold)
        {
            return TransitionLookup::Stop;
        }
        TransitionLookup::Linked(target, slot)
    }

    fn record_transition(
        &mut self,
        memory: &AddressSpace,
        source: &RegionRef,
        target: &RegionRef,
        pc: u32,
        threshold: u32,
    ) -> bool {
        if source.identity() == target.identity() {
            return false;
        }
        let ineligible = IneligibleTransition {
            target: target.identity(),
            pc,
        };
        if self.regions[source.key.index]
            .as_ref()
            .unwrap()
            .heat
            .ineligible
            == Some(ineligible)
        {
            return false;
        }
        let edges = &self.regions[source.key.index].as_ref().unwrap().heat.edges;
        let identity = target.identity();
        let mut selected = 0;
        let mut matched = false;
        for (index, edge) in edges.iter().enumerate() {
            if edge.target == identity && edge.pc == pc {
                selected = index;
                matched = true;
                break;
            }
            if edge.hits < edges[selected].hits {
                selected = index;
            }
        }
        // a retained code identity fixes its roots and entrypoints for the edge's lifetime.
        if !matched && !formation_candidate(source, target, pc) {
            self.regions[source.key.index]
                .as_mut()
                .unwrap()
                .heat
                .ineligible = Some(ineligible);
            return false;
        }
        if !matched && edges[selected].hits != 0 {
            for (index, edge) in edges.iter().enumerate() {
                if self
                    .region(RegionKey {
                        index: edge.target_index,
                        identity: edge.target,
                    })
                    .is_none()
                {
                    selected = index;
                    break;
                }
            }
        }
        let heat = &mut self.regions[source.key.index].as_mut().unwrap().heat;
        let edge = &mut heat.edges[selected];
        if edge.target != identity || edge.pc != pc {
            *edge = TransitionEdge {
                target: identity,
                target_index: target.key.index,
                pc,
                hits: 0,
            };
            heat.rejected[selected] = None;
        }
        edge.hits = edge.hits.saturating_add(1);
        if edge.hits < threshold {
            return false;
        }
        // success and decline both require another complete heat interval before retry.
        edge.hits = 0;
        self.pending_transition = Some(PendingTransition {
            source: source.key,
            target: target.key,
            pc,
            memory: memory.identity(),
            version: memory.code_source_version(),
        });
        true
    }

    pub(in crate::process) fn prepare_transition(
        &mut self,
        memory: &AddressSpace,
        pc: u32,
        options: CompileOptions,
        gates: &[(GuestAddress, u32)],
    ) -> Option<RegionRef> {
        let pending = self.pending_transition.take()?;
        if pc != pending.pc
            || pending.memory != memory.identity()
            || pending.version != memory.code_source_version()
        {
            return None;
        }
        let source = RegionRef {
            key: pending.source,
            code: Rc::clone(&self.region(pending.source)?.code),
        };
        let target = RegionRef {
            key: pending.target,
            code: Rc::clone(&self.region(pending.target)?.code),
        };
        if !self.linked_current(memory, &source)
            || !self.linked_current(memory, &target)
            || !self.indexes(&target, pc)
        {
            return None;
        }
        let rejected = TransitionRejection {
            version: memory.code_source_version(),
            instructions: options.instructions,
            blocks: options.blocks,
            wasm_bytes: options.wasm_bytes,
            source_entries: self.retained(&source)?.indexed_entries,
            target_entries: self.retained(&target)?.indexed_entries,
        };
        let heat = &mut self.regions[source.key.index].as_mut()?.heat;
        let selected = heat
            .edges
            .iter()
            .position(|edge| edge.target == target.identity() && edge.pc == pc)?;
        if heat.rejected[selected] == Some(rejected) {
            return None;
        }
        // an accepted formation removes this source owner and its edge rows.
        // declined pairs retry when limits, indexed entry counts or source epoch change.
        heat.rejected[selected] = Some(rejected);
        if rejected.source_entries + rejected.target_entries > options.blocks
            || !instruction_union_fits(&source, &target, options.instructions)
        {
            return None;
        }
        let roots = self.transition_roots(memory, &source, &target, pc, options.blocks)?;
        let mut frontier = self.frontier(memory);
        frontier.retain(|entry| {
            self.entries
                .get(&entry.0)
                .is_some_and(|key| *key != pending.source && *key != pending.target)
        });
        let code = CompiledCode::compile_transition(
            memory,
            GuestAddress(pc),
            options,
            gates,
            &frontier,
            &roots,
        )
        .ok()??;
        if code.wasm().len() > source.wasm().len() + target.wasm().len()
            || [&source, &target].into_iter().any(|owner| {
                owner.entrypoints().iter().any(|entry| {
                    self.indexes(owner, entry.0)
                        && code
                            .entrypoints()
                            .binary_search_by_key(&entry.0, |pc| pc.0)
                            .is_err()
                })
            })
        {
            return None;
        }
        Some(self.insert(code))
    }

    fn transition_roots(
        &self,
        memory: &AddressSpace,
        source: &RegionRef,
        target: &RegionRef,
        pc: u32,
        maximum: usize,
    ) -> Option<Vec<GuestAddress>> {
        let mut roots =
            Vec::with_capacity(source.formation_roots().len() + target.formation_roots().len() + 1);
        roots.extend(
            source
                .formation_roots()
                .iter()
                .chain(target.formation_roots())
                .copied(),
        );
        roots.push(GuestAddress(pc));
        roots.sort_unstable_by_key(|root| root.0);
        roots.dedup();
        if roots.len() < 2
            || roots.len() > maximum
            || roots.iter().any(|root| {
                root.0 != pc
                    && self.entries.get(&root.0).is_some_and(|key| {
                        *key != source.key
                            && *key != target.key
                            && self
                                .region(*key)
                                .is_some_and(|entry| entry.code.is_current(memory))
                    })
            })
        {
            return None;
        }
        Some(roots)
    }

    pub(in crate::process) fn clear_transition(&mut self) {
        self.pending_transition = None;
    }

    pub(in crate::process) fn retain_transition_on_import(&mut self, pc: u32) {
        if self
            .pending_transition
            .as_ref()
            .is_some_and(|pending| pending.pc != pc)
        {
            self.clear_transition();
        }
    }

    fn indexes(&self, code: &RegionRef, pc: u32) -> bool {
        self.entries
            .get(&pc)
            .is_some_and(|owner| *owner == code.key)
    }

    fn linked_current(&self, memory: &AddressSpace, code: &RegionRef) -> bool {
        self.retained(code)
            .is_some_and(|row| self.reciprocal_slot(code.key, row) && code.is_current(memory))
    }
}

fn instruction_union_fits(source: &RegionRef, target: &RegionRef, limit: usize) -> bool {
    if source.instruction_count() + target.instruction_count() <= limit {
        return true;
    }
    // final blocks retain unique instruction starts in increasing address order.
    let mut left = source.instruction_pcs().peekable();
    let mut right = target.instruction_pcs().peekable();
    let mut count = 0;
    loop {
        match (left.peek().copied(), right.peek().copied()) {
            (None, None) => return true,
            (Some(a), Some(b)) if a == b => {
                left.next();
                right.next();
            }
            (Some(a), Some(b)) if a < b => {
                left.next();
            }
            (Some(_), None) => {
                left.next();
            }
            _ => {
                right.next();
            }
        }
        count += 1;
        if count > limit {
            return false;
        }
    }
}

fn formation_candidate(source: &RegionRef, target: &RegionRef, pc: u32) -> bool {
    source.identity() != target.identity()
        && !(target
            .formation_roots()
            .binary_search_by_key(&pc, |root| root.0)
            .is_ok()
            && source.formation_roots().iter().all(|root| {
                target
                    .formation_roots()
                    .binary_search_by_key(&root.0, |candidate| candidate.0)
                    .is_ok()
            }))
        && source
            .entrypoints()
            .binary_search_by_key(&pc, |entry| entry.0)
            .is_err()
}

#[cfg(test)]
#[path = "promotion/tests.rs"]
mod tests;
