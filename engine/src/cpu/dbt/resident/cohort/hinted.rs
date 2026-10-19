use super::*;
use crate::cpu::{
    dbt::region::{MAX_COHORT_BLOCKS, prepare_resident_cohort_region},
    x86::decode::DecodeAuthority,
};

impl ResidentRegistry {
    #[allow(clippy::too_many_arguments)]
    pub(in crate::cpu::dbt::resident) fn prepare_hinted_cohort(
        &self,
        memory: &AddressSpace,
        entry: GuestAddress,
        limits: CompileLimits,
        cs: CodeFetchContext,
        authority: DecodeAuthority,
        eligible: &dyn Fn(u64) -> bool,
        is_gate: &dyn Fn(GuestAddress) -> bool,
        hint_target: &dyn Fn(u64) -> Option<GuestAddress>,
    ) -> Result<Option<(ResidentCohort, PreparedRegion)>, RegistryError> {
        self.check_memory(memory)?;
        let root = match self.lookup_bound(memory, entry, cs) {
            Ok(id) => id,
            Err(RegistryError::NotFound { .. } | RegistryError::CodeInvalidated) => {
                return Ok(None);
            }
            Err(error) => return Err(error),
        };
        if !eligible(root.get()) {
            return Ok(None);
        }
        let region = self.get(memory, root)?;
        if region.tier() != super::super::super::CompilationTier32::Optimized
            || region.first_entry() != Some(entry.0)
        {
            return Ok(None);
        }
        let mut coverage = ResidentCohort::empty();
        let mut specs = Vec::new();
        specs
            .try_reserve_exact(MAX_COHORT_BLOCKS)
            .map_err(|_| RegistryError::Allocation)?;
        let mut id = root;
        loop {
            let region = self.get(memory, id)?;
            let Some(blocks) = region
                .material_block_specs()
                .map_err(RegistryError::Compile)?
            else {
                break;
            };
            if !coverage.include_material(id.get(), region, limits, MAX_COHORT_BLOCKS, is_gate) {
                break;
            }
            specs.extend(blocks);
            if coverage.owner_count == COHORT_OWNERS {
                break;
            }
            let Some(target) = hint_target(id.get()) else {
                break;
            };
            if is_gate(target) {
                break;
            }
            let next = match self.lookup_bound(memory, target, cs) {
                Ok(id) => id,
                Err(RegistryError::NotFound { .. } | RegistryError::CodeInvalidated) => break,
                Err(error) => return Err(error),
            };
            if coverage.owners().contains(&next.get()) || !eligible(next.get()) {
                break;
            }
            if !self.get(memory, next)?.contains_instruction(target.0) {
                break;
            }
            id = next;
        }
        if coverage.owner_count < 2 {
            return Ok(None);
        }
        let prepared = match prepare_resident_cohort_region(
            memory,
            &specs,
            limits,
            &[],
            Some(cs),
            authority,
        ) {
            Ok(prepared) => prepared,
            Err(error @ CompileError::Allocation)
            | Err(
                error @ CompileError::Instruction {
                    cause:
                        super::super::super::InstructionError::Decode(
                            crate::cpu::x86::decode::DecodeError::Infrastructure(_),
                        ),
                    ..
                },
            ) => return Err(RegistryError::Compile(error)),
            Err(_) => return Ok(None),
        };
        Ok(coverage
            .covers_prepared(&prepared)
            .then_some((coverage, prepared)))
    }
}
