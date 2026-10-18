use super::{context::PromotionTarget32, types::*};
use crate::{
    cpu::{
        dbt::{COHORT_OWNERS, CompilationTier32, ResidentCohort},
        x86::decode::CodeDecodeKey,
    },
    process::{CallError, EngineInstance, HostError, ResidentInstallation},
};

#[derive(Clone, Copy)]
pub(super) struct PromotionCohort32 {
    owners: [ResidentInstallation; COHORT_OWNERS],
    coverage: ResidentCohort,
    next: usize,
    pc: u32,
    version: u64,
    decode_key: CodeDecodeKey,
    revision: u64,
    last_serial: u64,
}

impl PromotionCohort32 {
    pub(super) fn new(
        engine: &EngineInstance,
        pc: u32,
        coverage: ResidentCohort,
    ) -> Result<Self, HostError> {
        let mut owners = [ResidentInstallation {
            unit_id: 0,
            slot: u32::MAX,
        }; COHORT_OWNERS];
        for (row, &id) in owners.iter_mut().zip(coverage.owners()) {
            *row = engine
                .resident_installation_by_id(id)
                .ok_or(HostError::InvalidArtifact)?;
        }
        Ok(Self {
            owners,
            coverage,
            next: 0,
            pc,
            version: engine.memory()?.code_validation_version(),
            decode_key: engine.code_fetch_context()?.decode_key(),
            revision: engine.execution_cache.as_ref().unwrap().revision,
            last_serial: 0,
        })
    }

    pub(super) fn request(&self, serial: u64) -> CacheRequest32 {
        let victim = self.owners[self.next];
        CacheRequest32 {
            serial,
            kind: CacheRequestKind32::Evict,
            pc: self.pc,
            unit_id: 0,
            victim_id: victim.unit_id,
            slot: self.owners[0].slot,
            victim_slot: victim.slot,
            tier: CompilationTier32::Optimized,
        }
    }

    fn validate(&self, engine: &EngineInstance) -> Result<(), HostError> {
        let cache = engine.execution_cache.as_ref().unwrap();
        let memory = engine.memory()?;
        let cs = engine.code_fetch_context()?;
        if self.version != memory.code_validation_version()
            || self.decode_key != cs.decode_key()
            || self.revision != cache.revision
        {
            return Err(HostError::Call(CallError::StateChanged));
        }
        let pins = engine.cache_pins()?;
        for owner in &self.owners[self.next..self.coverage.owner_count] {
            if engine.resident_installation_by_id(owner.unit_id) != Some(*owner) {
                return Err(HostError::InvalidArtifact);
            }
            if pins.contains(&owner.unit_id) {
                return Err(HostError::Call(CallError::Busy));
            }
            let region = engine
                .resident
                .as_ref()
                .unwrap()
                .get_raw(memory, owner.unit_id)
                .map_err(HostError::Resident)?;
            if !region.matches_decode_key(self.decode_key) {
                return Err(HostError::Call(CallError::StateChanged));
            }
        }
        Ok(())
    }
}

impl EngineInstance {
    pub(super) fn continue_promotion_cohort(
        &mut self,
        pc: u32,
    ) -> Result<Option<CacheRequest32>, HostError> {
        let Some(plan) = self
            .execution_cache
            .as_ref()
            .and_then(|cache| cache.cohort.as_ref())
        else {
            return Ok(None);
        };
        self.execution_cache.as_ref().unwrap().can_request()?;
        if pc != plan.pc {
            self.abandon_promotion_cohort();
            return Ok(None);
        }
        if let Err(error) = plan.validate(self) {
            self.abandon_promotion_cohort();
            return Err(error);
        }
        let request = plan.request(self.execution_cache.as_ref().unwrap().serial);
        self.publish_cache_request(request, false)?;
        Ok(Some(request))
    }

    pub(super) fn validate_cohort_ack(&self, request: CacheRequest32) -> Result<(), HostError> {
        if let Some(plan) = self
            .execution_cache
            .as_ref()
            .and_then(|cache| cache.cohort.as_ref())
        {
            plan.validate(self)?;
            if plan.request(request.serial) != request {
                return Err(HostError::InvalidRequest);
            }
        }
        Ok(())
    }

    pub(super) fn abandon_promotion_cohort(&mut self) {
        let cache = self.execution_cache.as_mut().unwrap();
        cache.cohort = None;
        cache.promotion = None;
    }

    pub(super) fn cancel_cohort_after_ack(&mut self, serial: u64) -> bool {
        let matches = self.execution_cache.as_ref().is_some_and(|cache| {
            cache.request.is_none()
                && cache
                    .cohort
                    .as_ref()
                    .is_some_and(|plan| serial != 0 && plan.last_serial == serial)
        });
        if matches {
            self.abandon_promotion_cohort();
        }
        matches
    }

    /// acknowledged owners are already destroyed; cancellation intentionally rebuilds them cold.
    pub(super) fn advance_promotion_cohort(&mut self, serial: u64) -> bool {
        let cache = self.execution_cache.as_mut().unwrap();
        let Some(mut plan) = cache.cohort else {
            return false;
        };
        plan.next += 1;
        plan.last_serial = serial;
        plan.revision = cache.revision;
        if plan.next == plan.coverage.owner_count {
            cache.promotion = Some(PromotionTarget32 {
                pc: plan.pc,
                version: plan.version,
                decode_key: plan.decode_key,
                coverage: Some(plan.coverage),
            });
            cache.cohort = None;
        } else {
            cache.cohort = Some(plan);
        }
        true
    }
}
