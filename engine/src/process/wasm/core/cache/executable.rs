use super::*;
use crate::cpu::core::HotSpan;

pub(in crate::process) struct FragmentAdmission {
    ownership_epoch: u64,
    fingerprint: ObservedPlanFingerprint,
    ports: Vec<HotSpan>,
}

impl CoreCache32 {
    fn hot_keys(&self, pc: u32) -> [Option<RegionKey>; 4] {
        [
            self.window_entries.get(&pc).copied(),
            self.trace_entries.get(&pc).copied(),
            self.executable_entries.get(&pc).copied(),
            self.entries.get(&pc).copied(),
        ]
    }

    pub(in crate::process) fn current_hot_port(
        &self,
        memory: &AddressSpace,
        pc: u32,
    ) -> Option<(RegionRef, HotSpan)> {
        self.hot_port_with_bypass(memory, pc, 0)
    }

    fn hot_port_with_bypass(
        &self,
        memory: &AddressSpace,
        pc: u32,
        bypass: u64,
    ) -> Option<(RegionRef, HotSpan)> {
        for key in self
            .hot_keys(pc)
            .into_iter()
            .flatten()
            .filter(|key| key.identity != bypass)
        {
            let Some(entry) = self.region(key) else {
                continue;
            };
            if !entry.code.is_current(memory)
                || entry.slot != 0 && !self.reciprocal_slot(key, entry)
            {
                continue;
            }
            let Some(span) = entry.code.hot_span(pc) else {
                continue;
            };
            return Some((
                RegionRef {
                    key,
                    code: Rc::clone(&entry.code),
                },
                span,
            ));
        }
        None
    }

    pub(in crate::process) fn executable_frontier_contains(
        &self,
        memory: &AddressSpace,
        pc: u32,
        remaining: u32,
        bypass: u64,
    ) -> bool {
        self.hot_port_with_bypass(memory, pc, bypass)
            .is_some_and(|(_, span)| span.instructions <= remaining)
    }

    fn clean_stale_hot_entries(&mut self, memory: &AddressSpace, pc: u32) {
        for key in self.hot_keys(pc).into_iter().flatten() {
            if self
                .region(key)
                .is_some_and(|entry| !entry.code.is_current(memory))
            {
                self.remove(key);
                self.source_invalidations = self.source_invalidations.saturating_add(1);
            }
        }
    }

    pub(in crate::process) fn lookup_executable(
        &mut self,
        memory: &AddressSpace,
        pc: u32,
    ) -> Option<RegionRef> {
        self.clean_stale_hot_entries(memory, pc);
        let (code, _) = self.hot_port_with_bypass(memory, pc, self.trace_bypass_identity)?;
        self.touch(code.key.index);
        Some(code)
    }

    pub(in crate::process) fn lookup_executable_linked(
        &mut self,
        memory: &AddressSpace,
        pc: u32,
        remaining: u32,
    ) -> Option<(RegionRef, u32)> {
        self.clean_stale_hot_entries(memory, pc);
        let (code, span) = self.hot_port_with_bypass(memory, pc, self.trace_bypass_identity)?;
        if span.instructions > remaining {
            return None;
        }
        self.lookup_linked_key(memory, code.key)
    }

    pub(in crate::process) fn preflight_fragment(
        &mut self,
        memory: &AddressSpace,
        plan: &ObservedPlan,
    ) -> Option<FragmentAdmission> {
        if !plan.is_current(memory)
            || !plan.parent_identities().is_empty()
            || self.observed_refusals.iter().any(|old| {
                old.ownership_epoch == self.ownership_epoch
                    && &old.fingerprint == plan.fingerprint()
            })
        {
            return None;
        }
        let ports = plan.hot_ports();
        if ports.is_empty()
            || !ports.iter().any(|port| port.entry == plan.anchor())
            || ports
                .iter()
                .any(|port| self.current_hot_port(memory, port.entry).is_some())
        {
            return None;
        }
        Some(FragmentAdmission {
            ownership_epoch: self.ownership_epoch,
            fingerprint: plan.fingerprint().clone(),
            ports,
        })
    }

    pub(in crate::process) fn commit_fragment(
        &mut self,
        memory: &AddressSpace,
        admission: FragmentAdmission,
        code: CompiledCode,
    ) -> Result<RegionRef, CompileError> {
        if admission.ownership_epoch != self.ownership_epoch
            || !code.is_current(memory)
            || code.observed_fingerprint() != Some(&admission.fingerprint)
            || !code.is_observed_family()
            || code.is_execution_window()
            || !code.hot_spans().eq(admission.ports.iter().copied())
            || admission
                .ports
                .iter()
                .any(|port| self.current_hot_port(memory, port.entry).is_some())
        {
            return Err(CompileError::InvalidLimits);
        }
        let entries = admission
            .ports
            .iter()
            .map(|port| GuestAddress(port.entry))
            .collect();
        self.insert_with_kind(code, entries, DirectoryKind::Fragment)
    }

    pub(in crate::process) fn reject_fragment(&mut self, admission: FragmentAdmission) {
        if admission.ownership_epoch == self.ownership_epoch {
            self.remember_observed_refusal(admission.fingerprint);
        }
    }
}
