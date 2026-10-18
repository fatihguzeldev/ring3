use super::{CacheRequest32, CacheRequestKind32};
use crate::{
    cpu::dbt::CompilationTier32,
    process::{EngineInstance, HostError},
};
use std::sync::atomic::Ordering;

impl EngineInstance {
    pub fn prepare_cache_promotion(
        &mut self,
        key: u64,
    ) -> Result<Option<CacheRequest32>, HostError> {
        self.guard_cache_control(key)?;
        let cache = self
            .execution_cache
            .as_ref()
            .ok_or(HostError::InvalidRequest)?;
        cache.can_request()?;
        if cache.cohort.is_some() {
            return Err(HostError::Call(crate::process::CallError::Busy));
        }
        let pins = self.cache_pins()?;
        let memory = self.memory.as_ref().unwrap();
        let cs = self.code_fetch_context()?;
        let registry = self.resident.as_ref().unwrap();
        let candidate = cache
            .entries
            .iter()
            .enumerate()
            .filter_map(|(slot, row)| row.as_ref().map(|row| (slot, row)))
            .filter(|(_, row)| {
                row.tier == CompilationTier32::Baseline
                    && row.hits.load(Ordering::Relaxed) >= u64::from(cache.config.promotion_hits)
                    && !pins.contains(&row.id)
            })
            .filter(|(_, row)| {
                registry.get_raw(memory, row.id).is_ok_and(|region| {
                    region.supports_indexed_entry()
                        && region.matches_decode_key(cs.decode_key())
                        && region.contains_instruction(row.entry)
                })
            })
            .max_by_key(|(_, row)| {
                (
                    row.hits.load(Ordering::Relaxed),
                    std::cmp::Reverse(row.bytes),
                )
            });
        let Some((slot, row)) = candidate else {
            return Ok(None);
        };
        let request = CacheRequest32 {
            serial: cache.serial,
            kind: CacheRequestKind32::Evict,
            pc: row.entry,
            unit_id: 0,
            victim_id: row.id,
            slot: slot as u32,
            victim_slot: slot as u32,
            tier: CompilationTier32::Optimized,
        };
        let coverage = registry
            .plan_cached_cohort(
                memory,
                crate::memory::GuestAddress(row.entry),
                crate::cpu::dbt::CompileLimits::default(),
                cs,
                crate::cpu::x86::decode::DecodeAuthority::new(self.system_register_bank),
                &|id| {
                    !pins.contains(&id)
                        && self
                            .resident_installation_by_id(id)
                            .is_some_and(|installed| {
                                cache.entries[installed.slot as usize]
                                    .as_ref()
                                    .is_some_and(|row| row.id == id)
                            })
                },
                &|pc| {
                    super::admission::cache_gate_at(&self.modules, self.callback.as_ref(), pc.0)
                        .is_some()
                },
            )
            .map_err(HostError::Resident)?;
        let plan = coverage
            .map(|coverage| super::cohort::PromotionCohort32::new(self, row.entry, coverage))
            .transpose()?;
        let request = plan
            .as_ref()
            .map_or(request, |plan| plan.request(cache.serial));
        self.publish_cache_request(request, false)?;
        self.execution_cache.as_mut().unwrap().cohort = plan;
        Ok(Some(request))
    }
}
