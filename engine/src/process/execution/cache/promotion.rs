use super::CacheRequest32;
use crate::{
    cpu::dbt::CompilationTier32,
    process::{EngineInstance, HostError},
};
use std::sync::atomic::Ordering;

mod retry;
pub(super) use retry::PromotionRetry32;

impl EngineInstance {
    pub fn prepare_cache_promotion(
        &mut self,
        key: u64,
    ) -> Result<Option<CacheRequest32>, HostError> {
        #[cfg(feature = "observability")]
        let _capture = self.observer.enter();
        self.guard_cache_control(key)?;
        let cache = self
            .execution_cache
            .as_ref()
            .ok_or(HostError::InvalidRequest)?;
        cache.can_request()?;
        if cache.cohort.is_some() || cache.replacement.is_some() {
            return Err(HostError::Call(crate::process::CallError::Busy));
        }
        let round = self
            .execution_cache
            .as_mut()
            .unwrap()
            .next_promotion_round();
        let cache = self.execution_cache.as_ref().unwrap();
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
                row.promotion_retry.eligible(round)
                    && (row.tier == CompilationTier32::Baseline
                        || row
                            .need_code_hint
                            .target(cache.config.promotion_hits)
                            .is_some())
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
                    row.tier == CompilationTier32::Baseline,
                    row.hits.load(Ordering::Relaxed),
                    std::cmp::Reverse(row.bytes),
                )
            });
        let Some((slot, row)) = candidate else {
            return Ok(None);
        };
        let pc = row.entry;
        let tier = row.tier;
        #[cfg(feature = "observability")]
        let source_id = row.id;
        #[cfg(test)]
        cache.promotion_attempts.fetch_add(1, Ordering::Relaxed);
        let installations = &self.resident_installations;
        let by_id = &self.resident_installations_by_id;
        let modules = &self.modules;
        let callback = self.callback.as_ref();
        let current_row = |id| {
            let installed = by_id
                .binary_search_by_key(&id, |installed| installed.unit_id)
                .ok()
                .map(|index| by_id[index])?;
            if installations
                .get(installed.slot as usize)
                .copied()
                .flatten()
                != Some(installed)
            {
                return None;
            }
            cache
                .entries
                .get(installed.slot as usize)?
                .as_ref()
                .filter(|row| row.id == id)
        };
        let eligible = |id| !pins.contains(&id) && current_row(id).is_some();
        let is_gate = |pc: crate::memory::GuestAddress| {
            super::admission::cache_gate_at(modules, callback, pc.0).is_some()
        };
        let registry = self.resident.as_mut().unwrap();
        let authority = crate::cpu::x86::decode::DecodeAuthority::new(self.system_register_bank);
        let replacement = if tier == CompilationTier32::Optimized {
            registry.prepare_hinted_replacement(
                memory,
                crate::memory::GuestAddress(pc),
                cache.compile_limits,
                key,
                cs,
                authority,
                &eligible,
                &is_gate,
                &|id| {
                    current_row(id)?
                        .need_code_hint
                        .target(cache.config.promotion_hits)
                        .map(crate::memory::GuestAddress)
                },
            )
        } else {
            registry.prepare_cached_replacement(
                memory,
                crate::memory::GuestAddress(pc),
                cache.compile_limits,
                key,
                cs,
                authority,
                &eligible,
                &is_gate,
            )
        }
        .map_err(HostError::Resident)?;
        let Some(replacement) = replacement else {
            let row = self.execution_cache.as_mut().unwrap().entries[slot]
                .as_mut()
                .unwrap();
            row.promotion_retry.declined(round);
            #[cfg(feature = "observability")]
            if self.observer.enabled(
                ring3_observability::Level::Basic,
                ring3_observability::categories::ENGINE_CPU_CACHE,
            ) {
                let next = row.promotion_retry.next_round();
                self.observer.instant(
                    ring3_observability::Level::Basic,
                    ring3_observability::categories::ENGINE_CPU_CACHE,
                    "promotion.declined",
                    &[
                        ring3_observability::Field::unsigned("unit_id", row.id),
                        ring3_observability::Field::unsigned("pc", u64::from(pc)),
                        ring3_observability::Field::unsigned(
                            "retry_rounds",
                            next.saturating_sub(round).saturating_sub(1),
                        ),
                        ring3_observability::Field::unsigned("next_promotion_round", next),
                    ],
                );
            }
            return Ok(None);
        };
        #[cfg(feature = "observability")]
        if tier == CompilationTier32::Optimized {
            self.observer.instant(
                ring3_observability::Level::Basic,
                ring3_observability::categories::ENGINE_CPU_CACHE,
                "promotion.hinted.prepared",
                &[
                    ring3_observability::Field::unsigned("unit_id", source_id),
                    ring3_observability::Field::unsigned("pc", u64::from(pc)),
                    ring3_observability::Field::unsigned(
                        "owners",
                        replacement.coverage().owners().len() as u64,
                    ),
                ],
            );
        }
        let plan = super::cohort::PromotionCohort32::new(self, pc, replacement.coverage())?;
        let request = plan.request(self.execution_cache.as_ref().unwrap().serial);
        self.publish_cache_request(request, false)?;
        let cache = self.execution_cache.as_mut().unwrap();
        cache.cohort = Some(plan);
        cache.replacement = Some(replacement);
        Ok(Some(request))
    }
}
