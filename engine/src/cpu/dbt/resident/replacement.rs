use super::*;

/// fully emitted but unpublished; only the registry can turn this into resident authority.
pub(crate) struct PreparedResidentReplacement {
    id: UnitId,
    region: CompiledRegion,
    coverage: ResidentCohort,
    pages: PreparedEntryPages,
}

impl PreparedResidentReplacement {
    pub(crate) fn coverage(&self) -> ResidentCohort {
        self.coverage
    }
}

impl ResidentRegistry {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn prepare_hinted_replacement(
        &mut self,
        memory: &AddressSpace,
        entry: GuestAddress,
        limits: CompileLimits,
        key: u64,
        cs: CodeFetchContext,
        authority: crate::cpu::x86::decode::DecodeAuthority,
        eligible: &dyn Fn(u64) -> bool,
        is_gate: &dyn Fn(GuestAddress) -> bool,
        hint_target: &dyn Fn(u64) -> Option<GuestAddress>,
    ) -> Result<Option<PreparedResidentReplacement>, RegistryError> {
        super::super::region::validate_limits(limits).map_err(RegistryError::Compile)?;
        let limits = CompileLimits {
            blocks: super::super::region::MAX_COHORT_BLOCKS,
            ..limits
        };
        let Some((coverage, prepared)) = self.prepare_hinted_cohort(
            memory,
            entry,
            limits,
            cs,
            authority,
            eligible,
            is_gate,
            hint_target,
        )?
        else {
            return Ok(None);
        };
        match self.emit_cached_replacement(memory, coverage, prepared, limits, key, cs) {
            Err(error @ RegistryError::Compile(CompileError::Allocation))
            | Err(
                error @ RegistryError::Compile(CompileError::Instruction {
                    cause:
                        super::super::InstructionError::Decode(
                            crate::cpu::x86::decode::DecodeError::Infrastructure(_),
                        ),
                    ..
                }),
            ) => Err(error),
            Err(
                RegistryError::Compile(_)
                | RegistryError::InstructionOverlap { .. }
                | RegistryError::CodeInvalidated,
            ) => Ok(None),
            result => result,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn prepare_cached_replacement(
        &mut self,
        memory: &AddressSpace,
        entry: GuestAddress,
        limits: CompileLimits,
        key: u64,
        cs: CodeFetchContext,
        authority: crate::cpu::x86::decode::DecodeAuthority,
        eligible: &dyn Fn(u64) -> bool,
        is_gate: &dyn Fn(GuestAddress) -> bool,
    ) -> Result<Option<PreparedResidentReplacement>, RegistryError> {
        let Some((coverage, prepared)) =
            self.prepare_cached_cohort(memory, entry, limits, cs, authority, eligible, is_gate)?
        else {
            return Ok(None);
        };
        self.emit_cached_replacement(memory, coverage, prepared, limits, key, cs)
    }

    fn emit_cached_replacement(
        &mut self,
        memory: &AddressSpace,
        coverage: ResidentCohort,
        prepared: PreparedRegion,
        limits: CompileLimits,
        key: u64,
        cs: CodeFetchContext,
    ) -> Result<Option<PreparedResidentReplacement>, RegistryError> {
        let id = allocate_id(&NEXT_UNIT_ID)?;
        let region = match super::super::artifact::emit_entry_prepared_tier(
            memory,
            prepared,
            limits,
            Some(EmbeddedBinding::Resident { key, id: id.get() }),
            Some(cs),
            super::super::CompilationTier32::Optimized,
        ) {
            Ok(region) => region,
            Err(CompileError::WasmLimit) => return Ok(None),
            Err(error) => return Err(RegistryError::Compile(error)),
        };
        if region.tier() != super::super::CompilationTier32::Optimized || !coverage.covers(&region)
        {
            return Ok(None);
        }
        match self.admit_after_retiring(memory, &region, Some(cs), coverage.owners()) {
            Ok(_) => {}
            Err(RegistryError::UnitCapacity | RegistryError::ByteCapacity) => return Ok(None),
            Err(error) => return Err(error),
        }
        let pages = self.entry_index.prepare_replacement(&region)?;
        Ok(Some(PreparedResidentReplacement {
            id,
            region,
            coverage,
            pages,
        }))
    }

    pub(crate) fn validate_cached_replacement(
        &self,
        memory: &AddressSpace,
        cs: CodeFetchContext,
        candidate: &PreparedResidentReplacement,
        is_gate: &dyn Fn(GuestAddress) -> bool,
    ) -> Result<(), RegistryError> {
        self.check_memory(memory)?;
        if self
            .entries
            .last()
            .is_some_and(|unit| unit.id.get() >= candidate.id.get())
        {
            return Err(RegistryError::InvalidUnit);
        }
        if !candidate.region.matches_decode_key(cs.decode_key()) {
            return Err(RegistryError::CodeInvalidated);
        }
        candidate
            .region
            .wasm_bytes(memory)
            .map_err(|ArtifactError::CodeInvalidated| RegistryError::CodeInvalidated)?;
        if let Some(pc) = candidate
            .region
            .executable_addresses()
            .find(|&pc| is_gate(pc))
        {
            return Err(RegistryError::InstructionOverlap { pc });
        }
        Ok(())
    }

    pub(crate) fn publish_cached_replacement(
        &mut self,
        memory: &AddressSpace,
        cs: CodeFetchContext,
        candidate: PreparedResidentReplacement,
        is_gate: &dyn Fn(GuestAddress) -> bool,
    ) -> Result<UnitId, RegistryError> {
        self.validate_cached_replacement(memory, cs, &candidate, is_gate)?;
        let bytes = self.admit(memory, &candidate.region, Some(cs))?;
        let id = candidate.id;
        self.publish(id, candidate.region, bytes, candidate.pages);
        Ok(id)
    }
}

#[cfg(test)]
mod tests;
