use super::{context::PendingCache32, types::*};
use crate::{
    cpu::{
        dbt::{CompilationTier32, CompileLimits, RegistryError},
        x86::decode::DecodeAuthority,
    },
    memory::GuestAddress,
    process::{CallError, EngineInstance, HostError},
};
use std::sync::atomic::Ordering;

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
        self.guard_cache_control(key)?;
        let cache = self
            .execution_cache
            .as_ref()
            .ok_or(HostError::InvalidRequest)?;
        cache.can_request()?;
        let cs = self.code_fetch_context()?;
        let memory = self.memory.as_ref().unwrap();
        let tier = cache
            .promotion
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
            && let Some(installed) = self
                .resident_installations
                .iter()
                .flatten()
                .find(|installed| installed.unit_id == id)
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
        let free = self.resident_installations.iter().position(Option::is_none);
        let usage = registry.usage();
        let quota = existing.is_none()
            && (usage.units >= cache.config.units as usize
                || usage
                    .wasm_bytes
                    .checked_add(CompileLimits::default().wasm_bytes)
                    .is_none_or(|bytes| bytes > cache.config.wasm_bytes as usize));
        if free.is_none() || quota {
            let pins = self.cache_pins()?;
            let victim = cache
                .entries
                .iter()
                .enumerate()
                .filter_map(|(slot, row)| row.as_ref().map(|row| (slot, row)))
                .filter(|(_, row)| !pins.contains(&row.id))
                .min_by_key(|(_, row)| {
                    (
                        registry.get_raw(memory, row.id).is_ok(),
                        row.last.load(Ordering::Relaxed),
                    )
                });
            let Some((slot, victim)) = victim else {
                return Err(
                    if cache
                        .entries
                        .iter()
                        .flatten()
                        .any(|row| pins.contains(&row.id))
                    {
                        HostError::Call(CallError::Busy)
                    } else {
                        HostError::Resident(if usage.units >= cache.config.units as usize {
                            RegistryError::UnitCapacity
                        } else {
                            RegistryError::ByteCapacity
                        })
                    },
                );
            };
            let request = CacheRequest32 {
                serial: cache.serial,
                kind: CacheRequestKind32::Evict,
                pc,
                unit_id: existing.unwrap_or(0),
                victim_id: victim.id,
                slot: free.unwrap_or(slot) as u32,
                victim_slot: slot as u32,
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
            let gate = self.cache_gate_at(pc);
            let registry = self.resident.as_mut().unwrap();
            let memory = self.memory.as_ref().unwrap();
            registry
                .compile_cached_bound(
                    memory,
                    GuestAddress(pc),
                    gate,
                    CompileLimits::default(),
                    key,
                    cs,
                    DecodeAuthority::new(self.system_register_bank),
                    tier,
                )
                .map_err(HostError::Resident)?
                .get()
        };
        let tier = self
            .resident
            .as_ref()
            .unwrap()
            .get_raw(self.memory.as_ref().unwrap(), id)
            .map_err(HostError::Resident)?
            .tier();
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
    fn cache_gate_at(&self, pc: u32) -> Option<crate::cpu::dbt::GateSpec> {
        if let Some(gate) = self.module_gate_at(pc) {
            return Some(gate);
        }
        let mut callback = self.callback.as_ref();
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
}
