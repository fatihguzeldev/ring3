use super::*;
use crate::cpu::core::CompileOptions;

#[derive(Default)]
pub(super) struct TransitionHeat {
    target: u64,
    pc: u32,
    hits: u32,
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
            || source.identity() == target.identity()
            || (target.formation_roots().contains(&GuestAddress(pc))
                && source
                    .formation_roots()
                    .iter()
                    .all(|root| target.formation_roots().contains(root)))
            || source
                .entrypoints()
                .binary_search_by_key(&pc, |entry| entry.0)
                .is_ok()
            || !self.linked_current(memory, source)
            || !self.linked_current(memory, target)
            || !self.indexes(target, pc)
        {
            return false;
        }
        let heat = &mut self.regions[source.key.index].as_mut().unwrap().heat;
        if heat.target == target.identity() && heat.pc == pc {
            heat.hits = heat.hits.saturating_add(1);
        } else if heat.hits != 0 {
            heat.hits -= 1;
            return false;
        } else {
            heat.target = target.identity();
            heat.pc = pc;
            heat.hits = 1;
        }
        if heat.hits < threshold {
            return false;
        }
        // success and decline both require another complete heat interval before retry.
        heat.hits = 0;
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
        let mut roots = [GuestAddress(0); 16];
        let source_roots = source.formation_roots();
        let target_roots = target.formation_roots();
        roots[..source_roots.len()].copy_from_slice(source_roots);
        roots[source_roots.len()..source_roots.len() + target_roots.len()]
            .copy_from_slice(target_roots);
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
            &roots[..source_roots.len() + target_roots.len()],
        )
        .ok()??;
        Some(self.insert(code))
    }

    pub(in crate::process) fn clear_transition(&mut self) {
        self.pending_transition = None;
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

#[cfg(test)]
#[path = "promotion/tests.rs"]
mod tests;
