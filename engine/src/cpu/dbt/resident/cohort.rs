use super::*;
use std::cell::RefCell;

pub(crate) const COHORT_OWNERS: usize = 8;
pub(crate) const COHORT_INSTRUCTIONS: usize = 64;

/// exact instruction starts of whole owners, captured before any destructive acknowledgement.
#[derive(Clone, Copy)]
pub(crate) struct ResidentCohort {
    pub(crate) owners: [u64; COHORT_OWNERS],
    pub(crate) owner_count: usize,
    addresses: [u32; COHORT_INSTRUCTIONS],
    address_count: usize,
    blocks: usize,
}

impl ResidentCohort {
    fn empty() -> Self {
        Self {
            owners: [0; COHORT_OWNERS],
            owner_count: 0,
            addresses: [0; COHORT_INSTRUCTIONS],
            address_count: 0,
            blocks: 0,
        }
    }

    pub(crate) fn owners(&self) -> &[u64] {
        &self.owners[..self.owner_count]
    }

    pub(crate) fn covers(&self, region: &CompiledRegion) -> bool {
        self.addresses[..self.address_count]
            .iter()
            .all(|pc| region.contains_instruction(*pc))
    }

    fn covers_prepared(&self, prepared: &PreparedRegion) -> bool {
        self.addresses[..self.address_count].iter().all(|pc| {
            prepared.blocks.iter().any(|block| {
                block
                    .instructions
                    .iter()
                    .any(|instruction| instruction.pc().0 == *pc)
            })
        })
    }

    fn include(
        &mut self,
        id: u64,
        region: &CompiledRegion,
        pc: GuestAddress,
        limits: CompileLimits,
        is_gate: &dyn Fn(GuestAddress) -> bool,
    ) -> bool {
        if self.owners().contains(&id) {
            return true;
        }
        let metadata = region.metadata();
        if self.owner_count >= COHORT_OWNERS
            || region.first_entry() != Some(pc.0)
            || self.blocks + metadata.blocks > limits.blocks.min(COHORT_OWNERS)
            || self.address_count + metadata.instructions
                > limits.instructions.min(COHORT_INSTRUCTIONS)
            || region
                .executable_addresses()
                .any(|entry| !region.contains_instruction(entry.0) || is_gate(entry))
        {
            return false;
        }
        for entry in region.executable_addresses() {
            self.addresses[self.address_count] = entry.0;
            self.address_count += 1;
        }
        self.owners[self.owner_count] = id;
        self.owner_count += 1;
        self.blocks += metadata.blocks;
        true
    }
}

impl ResidentRegistry {
    /// the live registry remains unchanged. Failed speculative coverage never authorizes retirement.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn plan_cached_cohort(
        &self,
        memory: &AddressSpace,
        entry: GuestAddress,
        limits: CompileLimits,
        cs: CodeFetchContext,
        authority: crate::cpu::x86::decode::DecodeAuthority,
        eligible: &dyn Fn(u64) -> bool,
        is_gate: &dyn Fn(GuestAddress) -> bool,
    ) -> Result<Option<ResidentCohort>, RegistryError> {
        self.check_memory(memory)?;
        let root = self.lookup_bound(memory, entry, cs)?;
        if !eligible(root.get()) {
            return Ok(None);
        }
        let root_region = self.get(memory, root)?;
        let mut plan = ResidentCohort::empty();
        if !plan.include(root.get(), root_region, entry, limits, is_gate) {
            return Ok(None);
        }
        let plan = RefCell::new(plan);
        let prepared = super::super::cold::prepare_runtime_region_until(
            memory,
            entry,
            limits,
            Some(cs),
            authority,
            &|pc| {
                if is_gate(pc) {
                    return true;
                }
                let Ok(id) = self.lookup_bound(memory, pc, cs) else {
                    return false;
                };
                if plan.borrow().owners().contains(&id.get()) {
                    return false;
                }
                if !eligible(id.get()) {
                    return true;
                }
                let Ok(region) = self.get(memory, id) else {
                    return true;
                };
                !plan
                    .borrow_mut()
                    .include(id.get(), region, pc, limits, is_gate)
            },
        )
        .map_err(RegistryError::Compile)?;
        let plan = plan.into_inner();
        Ok((plan.owner_count > 1 && plan.covers_prepared(&prepared)).then_some(plan))
    }
}

#[cfg(test)]
mod tests;
