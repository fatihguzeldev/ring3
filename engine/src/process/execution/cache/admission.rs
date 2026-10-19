use super::{context::PendingCache32, types::*};
use crate::{
    cpu::{
        dbt::{CompilationTier32, RegistryError},
        x86::decode::DecodeAuthority,
    },
    memory::GuestAddress,
    process::{CallError, EngineInstance, HostError},
};

impl EngineInstance {
    pub fn prepare_cache_entry(&mut self, key: u64, pc: u32) -> Result<CacheRequest32, HostError> {
        #[cfg(feature = "observability")]
        let _capture = self.observer.enter();
        #[cfg(feature = "observability")]
        let _scope = ring3_observability::current().scope(
            ring3_observability::Level::Detailed,
            ring3_observability::categories::ENGINE_CPU_CACHE,
            "prepare_entry",
            &[ring3_observability::Field::unsigned("pc", u64::from(pc))],
        );
        if let Err(error) = self.guard_cache_control(key) {
            if key == self.key
                && error == HostError::Call(CallError::Cancelled)
                && self
                    .execution_cache
                    .as_ref()
                    .is_some_and(|cache| cache.request.is_none() && cache.cohort.is_some())
            {
                self.abandon_promotion_cohort();
            }
            return Err(error);
        }
        if let Some(request) = self.continue_promotion_cohort(pc)? {
            return Ok(request);
        }
        let cache = self
            .execution_cache
            .as_ref()
            .ok_or(HostError::InvalidRequest)?;
        cache.can_request()?;
        let cs = self.code_fetch_context()?;
        let memory = self.memory.as_ref().unwrap();
        let tier = cache
            .promotion
            .as_ref()
            .filter(|target| {
                target.pc == pc
                    && target.version == memory.code_validation_version()
                    && target.decode_key == cs.decode_key()
            })
            .map_or(CompilationTier32::Baseline, |_| {
                CompilationTier32::Optimized
            });
        let registry = self.resident.as_ref().unwrap();
        let existing = match registry.lookup_bound(memory, GuestAddress(pc), cs) {
            Ok(id) => Some(id.get()),
            Err(RegistryError::NotFound { .. } | RegistryError::CodeInvalidated) => None,
            Err(error) => return Err(HostError::Resident(error)),
        };
        if let Some(id) = existing
            && let Some(installed) = self.resident_installation_by_id(id)
        {
            cache.touch(installed.slot, id);
            return Ok(CacheRequest32 {
                serial: 0,
                kind: CacheRequestKind32::Hit,
                pc,
                unit_id: id,
                victim_id: 0,
                slot: installed.slot,
                victim_slot: u32::MAX,
                tier: cache.entries[installed.slot as usize]
                    .as_ref()
                    .map_or(CompilationTier32::Baseline, |row| row.tier),
            });
        }
        let free = cache.free_slots.first().map(|slot| slot as usize);
        let usage = registry.usage();
        let quota = existing.is_none()
            && (usage.units >= cache.config.units as usize
                || usage
                    .wasm_bytes
                    .checked_add(cache.compile_limits.wasm_bytes)
                    .is_none_or(|bytes| bytes > cache.config.wasm_bytes as usize));
        if free.is_none() || quota {
            let pins = self.cache_pins()?;
            let cache = self.execution_cache.as_mut().unwrap();
            let selected = cache.victims.select(
                &cache.entries,
                memory.code_validation_version(),
                |id| registry.get_raw(memory, id).is_ok(),
                &pins,
            );
            let Some(slot) = selected else {
                return Err(if cache.victims.len() != 0 {
                    HostError::Call(CallError::Busy)
                } else {
                    HostError::Resident(if usage.units >= cache.config.units as usize {
                        RegistryError::UnitCapacity
                    } else {
                        RegistryError::ByteCapacity
                    })
                });
            };
            let victim = cache.entries[slot as usize].as_ref().unwrap();
            let request = CacheRequest32 {
                serial: cache.serial,
                kind: CacheRequestKind32::Evict,
                pc,
                unit_id: existing.unwrap_or(0),
                victim_id: victim.id,
                slot: free.unwrap_or(slot as usize) as u32,
                victim_slot: slot,
                tier: CompilationTier32::Baseline,
            };
            #[cfg(feature = "observability")]
            ring3_observability::current().instant(
                ring3_observability::Level::Detailed,
                ring3_observability::categories::ENGINE_CPU_CACHE,
                "eviction.requested",
                &[
                    ring3_observability::Field::unsigned("unit_id", victim.id),
                    ring3_observability::Field::unsigned("pc", u64::from(pc)),
                    ring3_observability::Field::boolean(
                        "unit_capacity",
                        usage.units >= cache.config.units as usize,
                    ),
                    ring3_observability::Field::boolean(
                        "byte_capacity",
                        quota && usage.units < cache.config.units as usize,
                    ),
                    ring3_observability::Field::boolean("slot_capacity", free.is_none()),
                ],
            );
            self.publish_cache_request(request, false)?;
            return Ok(request);
        }
        let slot = free.unwrap() as u32;
        let id = if let Some(id) = existing {
            id
        } else {
            let modules = &self.modules;
            let callback = self.callback.as_ref();
            let gate = cache_gate_at(modules, callback, pc);
            let registry = self.resident.as_mut().unwrap();
            let memory = self.memory.as_ref().unwrap();
            registry
                .compile_cached_bound(
                    memory,
                    GuestAddress(pc),
                    gate,
                    cache.compile_limits,
                    key,
                    cs,
                    DecodeAuthority::new(self.system_register_bank),
                    tier,
                    &|address| cache_gate_at(modules, callback, address.0).is_some(),
                )
                .map_err(HostError::Resident)?
                .get()
        };
        let region = self
            .resident
            .as_ref()
            .unwrap()
            .get_raw(self.memory.as_ref().unwrap(), id)
            .map_err(HostError::Resident)?;
        let tier = region.tier();
        if self
            .execution_cache
            .as_ref()
            .unwrap()
            .promotion
            .as_ref()
            .filter(|target| {
                target.pc == pc
                    && target.version == self.memory.as_ref().unwrap().code_validation_version()
                    && target.decode_key == cs.decode_key()
            })
            .and_then(|target| target.coverage.as_ref())
            .is_some_and(|coverage| {
                tier != CompilationTier32::Optimized || !coverage.covers(region)
            })
        {
            // an emitted byte-limit fallback cannot claim to replace a whole cohort.
            // already acknowledged owners remain absent; the normal cold path rebuilds them.
            if existing.is_none() {
                self.resident
                    .as_mut()
                    .unwrap()
                    .discard_quiescent_raw(self.memory.as_ref().unwrap(), id)
                    .map_err(HostError::Resident)?;
            }
            self.execution_cache.as_mut().unwrap().promotion = None;
            return self.prepare_cache_entry(key, pc);
        }
        #[cfg(feature = "observability")]
        let metadata = region.metadata();
        #[cfg(feature = "observability")]
        let code_bytes = region.retained_wasm_byte_len();
        let request = CacheRequest32 {
            serial: self.execution_cache.as_ref().unwrap().serial,
            kind: CacheRequestKind32::Install,
            pc,
            unit_id: id,
            victim_id: 0,
            slot,
            victim_slot: u32::MAX,
            tier,
        };
        self.publish_cache_request(request, existing.is_none())?;
        #[cfg(feature = "observability")]
        ring3_observability::current().instant(
            ring3_observability::Level::Detailed,
            ring3_observability::categories::ENGINE_CPU_CACHE,
            "installation.requested",
            &[
                ring3_observability::Field::unsigned("unit_id", id),
                ring3_observability::Field::unsigned("pc", u64::from(pc)),
                ring3_observability::Field::unsigned("tier", tier as u64),
                ring3_observability::Field::unsigned("blocks", metadata.blocks as u64),
                ring3_observability::Field::unsigned("instructions", metadata.instructions as u64),
                ring3_observability::Field::unsigned("wasm_bytes", code_bytes as u64),
                ring3_observability::Field::boolean("compiled", existing.is_none()),
            ],
        );
        Ok(request)
    }
    pub(super) fn publish_cache_request(
        &mut self,
        request: CacheRequest32,
        owned_unit: bool,
    ) -> Result<(), HostError> {
        let decode_key = self.code_fetch_context()?.decode_key();
        let version = self.memory.as_ref().unwrap().code_validation_version();
        let cache = self.execution_cache.as_mut().unwrap();
        cache.request = Some(PendingCache32 {
            request,
            version,
            decode_key,
            revision: cache.revision,
            owned_unit,
        });
        cache.serial += 1;
        cache.misses = cache.misses.saturating_add(1);
        Ok(())
    }
}

pub(super) fn cache_gate_at(
    modules: &crate::process::loading::modules::ModuleProcess32,
    mut callback: Option<&crate::process::calls::callback::SuspendedCallback>,
    pc: u32,
) -> Option<crate::cpu::dbt::GateSpec> {
    if let Some(gate) = modules.gate_at(pc) {
        return Some(gate);
    }
    while let Some(current) = callback {
        let (return_pc, return_id) = match current.record {
            crate::process::calls::callback::SuspendedRecord::Replacement(record) => {
                (record.return_pc, record.return_id)
            }
            crate::process::calls::callback::SuspendedRecord::Resident { record, .. } => {
                (record.return_pc, record.return_id)
            }
        };
        if return_pc == pc {
            return Some(crate::cpu::dbt::GateSpec {
                entry: GuestAddress(pc),
                id: return_id,
            });
        }
        callback = current.parent.as_ref().map(|parent| &parent[0]);
    }
    None
}
