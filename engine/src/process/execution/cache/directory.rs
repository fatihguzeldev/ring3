use super::{context::*, types::*};
use crate::{
    cpu::dbt::{CompilationTier32, RegistryError},
    process::{EngineInstance, HostError},
};
use std::sync::atomic::{AtomicU64, Ordering};

impl ExecutionCache32 {
    pub(super) fn installed(
        &mut self,
        slot: u32,
        id: u64,
        entry: u32,
        bytes: usize,
        tier: CompilationTier32,
    ) {
        let row = &mut self.entries[slot as usize];
        if row.as_ref().is_some_and(|row| row.id == id) {
            return;
        }
        let last = increment(&self.clock);
        *row = Some(CacheEntry32 {
            id,
            entry,
            bytes,
            tier,
            hits: AtomicU64::new(0),
            last: AtomicU64::new(last),
            need_code_hint: Default::default(),
            promotion_retry: Default::default(),
        });
        self.free_slots.occupy(slot);
        self.victims.install(slot, id, last);
        self.revision = self.revision.saturating_add(1);
    }
    pub(super) fn removed(&mut self, id: u64, slot: Option<u32>) {
        if let Some(slot) = slot {
            let row = &mut self.entries[slot as usize];
            if row.as_ref().is_some_and(|row| row.id == id) {
                *row = None;
                self.free_slots.release(slot);
                self.victims.remove(slot);
            }
        }
        self.revision = self.revision.saturating_add(1);
    }
    pub(in crate::process::execution) fn touch(&self, slot: u32, id: u64) {
        if let Some(row) = self
            .entries
            .get(slot as usize)
            .and_then(Option::as_ref)
            .filter(|row| row.id == id)
        {
            increment(&row.hits);
            increment(&self.hits);
            row.last.store(increment(&self.clock), Ordering::Relaxed);
        }
    }
    pub(super) fn can_request(&self) -> Result<(), HostError> {
        if self.request.is_some() {
            return Err(HostError::Call(crate::process::CallError::Busy));
        }
        self.serial
            .checked_add(1)
            .ok_or(HostError::Resident(RegistryError::IdentityExhausted))?;
        self.revision
            .checked_add(1)
            .ok_or(HostError::Resident(RegistryError::IdentityExhausted))?;
        Ok(())
    }
}
impl EngineInstance {
    pub fn cache_usage(&self) -> Result<CacheUsage32, HostError> {
        self.memory()?;
        let cache = self
            .execution_cache
            .as_ref()
            .ok_or(HostError::InvalidRequest)?;
        let usage = self.resident.as_ref().unwrap().usage();
        Ok(CacheUsage32 {
            units: usage.units as u32,
            wasm_bytes: usage.wasm_bytes as u32,
            installed: cache.victims.len() as u32,
            hits: cache.hits.load(Ordering::Relaxed),
            misses: cache.misses,
            evictions: cache.evictions,
            promotions: cache.promotions,
            pending_serial: cache.request.map_or(0, |pending| pending.request.serial),
        })
    }
    pub(in crate::process::execution) fn cache_track_installation(&mut self, slot: u32, id: u64) {
        if self.execution_cache.is_none() {
            return;
        }
        let region = self
            .resident
            .as_ref()
            .unwrap()
            .get_retained_raw(self.memory.as_ref().unwrap(), id)
            .unwrap();
        let entry = region.first_entry().unwrap_or(0);
        let bytes = region.retained_wasm_byte_len();
        let tier = region.tier();
        self.execution_cache
            .as_mut()
            .unwrap()
            .installed(slot, id, entry, bytes, tier);
    }
    pub(in crate::process::execution) fn cache_track_removal(
        &mut self,
        id: u64,
        slot: Option<u32>,
    ) {
        if let Some(cache) = self.execution_cache.as_mut() {
            cache.removed(id, slot);
        }
    }
}
