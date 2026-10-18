use super::{
    context::{PendingCache32, PromotionTarget32},
    types::*,
};
use crate::process::{CallError, EngineInstance, HostError, ResidentInstallation};

impl EngineInstance {
    pub fn pending_cache_request(&self) -> Result<Option<CacheRequest32>, HostError> {
        self.memory()?;
        Ok(self
            .execution_cache
            .as_ref()
            .and_then(|cache| cache.request.map(|pending| pending.request)))
    }
    fn checked_cache_request(
        &self,
        key: u64,
        serial: u64,
        kind: CacheRequestKind32,
    ) -> Result<PendingCache32, HostError> {
        self.guard_cache_control(key)?;
        let cache = self
            .execution_cache
            .as_ref()
            .ok_or(HostError::InvalidRequest)?;
        let pending = cache
            .request
            .filter(|pending| {
                serial != 0 && pending.request.serial == serial && pending.request.kind == kind
            })
            .ok_or(HostError::Call(CallError::InvalidToken))?;
        if pending.version != self.memory.as_ref().unwrap().code_validation_version()
            || pending.decode_key != self.code_fetch_context()?.decode_key()
            || pending.revision != cache.revision
        {
            return Err(HostError::Call(CallError::StateChanged));
        }
        Ok(pending)
    }
    /// acknowledges a cleared host table slot after all generated execution has returned.
    pub fn acknowledge_cache_eviction(
        &mut self,
        key: u64,
        serial: u64,
        victim: u64,
        slot: u32,
    ) -> Result<(), HostError> {
        let pending = self.checked_cache_request(key, serial, CacheRequestKind32::Evict)?;
        if pending.request.victim_id != victim
            || pending.request.victim_slot != slot
            || self
                .resident_installations
                .get(slot as usize)
                .copied()
                .flatten()
                != Some(ResidentInstallation {
                    unit_id: victim,
                    slot,
                })
        {
            return Err(HostError::InvalidRequest);
        }
        if self.cache_pins()?.contains(&victim) {
            return Err(HostError::Call(CallError::Busy));
        }
        self.resident
            .as_mut()
            .unwrap()
            .discard_quiescent_raw(self.memory.as_ref().unwrap(), victim)
            .map_err(HostError::Resident)?;
        self.remove_resident_installation(victim);
        self.cache_track_removal(victim);
        let cache = self.execution_cache.as_mut().unwrap();
        cache.evictions = cache.evictions.saturating_add(1);
        if pending.request.tier == crate::cpu::dbt::CompilationTier32::Optimized {
            cache.promotion = Some(PromotionTarget32 {
                pc: pending.request.pc,
                version: pending.version,
                decode_key: pending.decode_key,
            });
        }
        cache.request = None;
        Ok(())
    }
    /// acknowledges the compiled unit installed in its reserved host table slot.
    pub fn install_cached_entry(
        &mut self,
        key: u64,
        serial: u64,
        unit: u64,
        slot: u32,
    ) -> Result<ResidentInstallation, HostError> {
        let pending = self.checked_cache_request(key, serial, CacheRequestKind32::Install)?;
        if pending.request.unit_id != unit || pending.request.slot != slot {
            return Err(HostError::InvalidRequest);
        }
        self.guard_resident_unit(key, unit)?;
        let installed = self.acknowledge_resident_slot(unit, slot)?;
        let cache = self.execution_cache.as_mut().unwrap();
        if pending.request.tier == crate::cpu::dbt::CompilationTier32::Optimized {
            cache.promotions = cache.promotions.saturating_add(1);
        }
        if cache
            .promotion
            .is_some_and(|target| target.pc == pending.request.pc)
        {
            cache.promotion = None;
        }
        cache.request = None;
        Ok(installed)
    }
    pub fn cancel_cache_request(&mut self, key: u64, serial: u64) -> Result<(), HostError> {
        self.memory()?;
        if key != self.key {
            return Err(HostError::InvalidArtifact);
        }
        self.guard_detection_material_change()?;
        let pending = self
            .execution_cache
            .as_ref()
            .and_then(|cache| cache.request)
            .filter(|pending| serial != 0 && pending.request.serial == serial)
            .ok_or(HostError::Call(CallError::InvalidToken))?;
        if pending.owned_unit {
            if self.cache_pins()?.contains(&pending.request.unit_id) {
                return Err(HostError::Call(CallError::Busy));
            }
            if self
                .resident_installations
                .iter()
                .flatten()
                .any(|installed| installed.unit_id == pending.request.unit_id)
            {
                return Err(HostError::Call(CallError::Busy));
            }
            self.resident
                .as_mut()
                .unwrap()
                .discard_quiescent_raw(self.memory.as_ref().unwrap(), pending.request.unit_id)
                .map_err(HostError::Resident)?;
            self.cache_track_removal(pending.request.unit_id);
        }
        self.execution_cache.as_mut().unwrap().request = None;
        Ok(())
    }
}
