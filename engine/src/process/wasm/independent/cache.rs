use crate::{
    cpu::runtime::{CompiledUnit, ServiceSite, emit_dispatch_with_service as emit_dispatch},
    memory::{AddressSpace, CodeSnapshot, PageVersion},
    process::HostError,
};
use std::mem::size_of;

const BYTE_LIMIT: usize = 128 * 1024 * 1024;
const MIN_DIRECTORY: usize = 16;

pub(super) struct CodeCache {
    units: Vec<Option<RetainedUnit>>,
    entries: Vec<[u32; 2]>,
    services: Vec<[u32; 3]>,
    directory: Vec<[u32; 2]>,
    dispatcher: Vec<u8>,
    victims: Vec<u32>,
    validated: Option<(u64, u64)>,
    oldest: usize,
    limit: usize,
    retained_code_bytes: usize,
    #[cfg(test)]
    source_checks: usize,
}

struct RetainedUnit {
    code: CompiledUnit,
    charge: usize,
}

fn code_bytes(code: &CompiledUnit) -> usize {
    code.wasm
        .capacity()
        .saturating_add(code.entries.capacity().saturating_mul(size_of::<u32>()))
        .saturating_add(
            code.services
                .capacity()
                .saturating_mul(size_of::<ServiceSite>()),
        )
        .saturating_add(
            code.sources
                .capacity()
                .saturating_mul(size_of::<CodeSnapshot>()),
        )
        .saturating_add(
            code.sources
                .iter()
                .map(|source| {
                    source
                        .versions
                        .len()
                        .saturating_mul(size_of::<PageVersion>())
                })
                .fold(0usize, usize::saturating_add),
        )
}

fn directory_size(entries: usize) -> Result<usize, HostError> {
    entries
        .checked_mul(2)
        .and_then(|length| length.max(MIN_DIRECTORY).checked_next_power_of_two())
        .ok_or(HostError::Infrastructure)
}

fn growth<T>(values: &Vec<T>, required: usize) -> usize {
    required
        .saturating_sub(values.capacity())
        .saturating_mul(size_of::<T>())
}

fn reserve<T>(values: &mut Vec<T>, required: usize) -> Result<(), HostError> {
    if required > values.capacity() {
        values
            .try_reserve_exact(required.saturating_sub(values.len()))
            .map_err(|_| HostError::Infrastructure)?;
    }
    Ok(())
}

impl CodeCache {
    pub(super) fn new() -> Result<Self, HostError> {
        let mut directory = Vec::new();
        reserve(&mut directory, MIN_DIRECTORY)?;
        directory.resize(MIN_DIRECTORY, [0; 2]);
        let cache = Self {
            units: Vec::new(),
            entries: Vec::new(),
            services: Vec::new(),
            directory,
            dispatcher: emit_dispatch(),
            victims: Vec::new(),
            validated: None,
            oldest: 0,
            limit: BYTE_LIMIT,
            retained_code_bytes: 0,
            #[cfg(test)]
            source_checks: 0,
        };
        if cache.charged_bytes() > cache.limit {
            return Err(HostError::Infrastructure);
        }
        Ok(cache)
    }

    pub(super) fn charged_bytes(&self) -> usize {
        self.units
            .capacity()
            .saturating_mul(size_of::<Option<RetainedUnit>>())
            .saturating_add(
                self.entries
                    .capacity()
                    .saturating_mul(size_of::<[u32; 2]>()),
            )
            .saturating_add(
                self.services
                    .capacity()
                    .saturating_mul(size_of::<[u32; 3]>()),
            )
            .saturating_add(
                self.directory
                    .capacity()
                    .saturating_mul(size_of::<[u32; 2]>()),
            )
            .saturating_add(self.victims.capacity().saturating_mul(size_of::<u32>()))
            .saturating_add(self.dispatcher.capacity())
            .saturating_add(self.retained_code_bytes)
    }

    pub(super) fn directory_ptr(&self) -> u32 {
        self.directory.as_ptr() as usize as u32
    }
    pub(super) fn directory_mask(&self) -> u32 {
        self.directory.len() as u32 - 1
    }
    pub(super) fn dispatcher_ptr(&self) -> u32 {
        self.dispatcher.as_ptr() as usize as u32
    }
    pub(super) fn dispatcher_len(&self) -> u32 {
        self.dispatcher.len() as u32
    }

    pub(super) fn lookup(&self, pc: u32) -> Option<u32> {
        self.entries
            .binary_search_by_key(&pc, |entry| entry[0])
            .ok()
            .map(|index| self.entries[index][1])
    }

    pub(super) fn service(
        &mut self,
        memory: &AddressSpace,
        pc: u32,
    ) -> Result<Option<ServiceSite>, HostError> {
        self.refresh(memory)?;
        let Ok(index) = self.services.binary_search_by_key(&pc, |entry| entry[0]) else {
            return Ok(None);
        };
        let [_, slot, site] = self.services[index];
        let site = self
            .selected(slot)
            .and_then(|code| code.services.get(site as usize))
            .copied()
            .ok_or(HostError::InvalidArtifact)?;
        if site.pc != pc {
            return Err(HostError::InvalidArtifact);
        }
        Ok(Some(site))
    }

    pub(super) fn selected(&self, slot: u32) -> Option<&CompiledUnit> {
        slot.checked_sub(1)
            .and_then(|index| self.units.get(index as usize))
            .and_then(Option::as_ref)
            .map(|unit| &unit.code)
    }

    pub(super) fn take_victim(&mut self) -> u32 {
        self.victims.pop().unwrap_or(0)
    }

    pub(super) fn refresh(&mut self, memory: &AddressSpace) -> Result<(), HostError> {
        let current = (memory.identity(), memory.code_validation_version());
        if self.validated == Some(current) {
            return Ok(());
        }
        if self
            .validated
            .is_some_and(|previous| previous.0 != current.0)
        {
            return Err(HostError::InvalidArtifact);
        }
        let mut revoked = false;
        for index in 0..self.units.len() {
            let Some(unit) = self.units[index].as_ref() else {
                continue;
            };
            #[cfg(test)]
            {
                self.source_checks += 1;
            }
            if !unit.code.is_current(memory) {
                if !revoked {
                    self.directory.fill([0; 2]);
                    revoked = true;
                }
                self.remove(index);
            }
        }
        if revoked {
            self.rebuild_directory()?;
        }
        self.validated = Some(current);
        Ok(())
    }

    pub(super) fn install(
        &mut self,
        code: CompiledUnit,
        memory: &AddressSpace,
    ) -> Result<u32, HostError> {
        if code.entries.is_empty()
            || code.sources.is_empty()
            || !code.is_current(memory)
            || code.services.len() > u32::MAX as usize
        {
            return Err(HostError::InvalidArtifact);
        }
        let incoming = code_bytes(&code);
        let count = self
            .units
            .len()
            .checked_add(1)
            .ok_or(HostError::Infrastructure)?;
        let slot = u32::try_from(count).map_err(|_| HostError::Infrastructure)?;
        // slot identities never move or disappear; even an otherwise empty
        // cache must retain every historical header and reserve its victim row.
        let slot_bytes = count
            .checked_mul(size_of::<Option<RetainedUnit>>())
            .and_then(|headers| {
                count
                    .checked_mul(size_of::<u32>())
                    .and_then(|victims| headers.checked_add(victims))
            })
            .ok_or(HostError::Infrastructure)?;
        if incoming
            .saturating_add(self.dispatcher.capacity())
            .saturating_add(slot_bytes)
            .saturating_add(code.entries.len().saturating_mul(size_of::<[u32; 2]>()))
            .saturating_add(code.services.len().saturating_mul(size_of::<[u32; 3]>()))
            .saturating_add(
                directory_size(code.entries.len())?.saturating_mul(size_of::<[u32; 2]>()),
            )
            > self.limit
        {
            return Err(HostError::Infrastructure);
        }
        self.refresh(memory)?;
        let mut compacted = false;
        loop {
            let entries = self
                .entries
                .len()
                .checked_add(code.entries.len())
                .ok_or(HostError::Infrastructure)?;
            let directory = directory_size(entries)?;
            let services = self
                .services
                .len()
                .checked_add(code.services.len())
                .ok_or(HostError::Infrastructure)?;
            let extra = incoming
                .saturating_add(growth(&self.units, count))
                .saturating_add(growth(&self.entries, entries))
                .saturating_add(growth(&self.services, services))
                .saturating_add(growth(&self.directory, directory))
                .saturating_add(growth(&self.victims, count));
            if self.charged_bytes().saturating_add(extra) <= self.limit {
                reserve(&mut self.units, count)?;
                reserve(&mut self.entries, entries)?;
                reserve(&mut self.services, services)?;
                reserve(&mut self.directory, directory)?;
                reserve(&mut self.victims, count)?;
                if self.charged_bytes().saturating_add(incoming) <= self.limit {
                    break;
                }
            }
            if !self.evict_oldest()? {
                if compacted {
                    return Err(HostError::Infrastructure);
                }
                self.units.shrink_to_fit();
                self.entries.shrink_to_fit();
                self.services.shrink_to_fit();
                self.directory.shrink_to_fit();
                self.victims.shrink_to_fit();
                compacted = true;
            }
        }
        self.entries
            .extend(code.entries.iter().copied().map(|pc| [pc, slot]));
        self.entries.sort_unstable();
        self.entries.dedup_by_key(|entry| entry[0]);
        self.services.extend(
            code.services
                .iter()
                .enumerate()
                .map(|(index, site)| [site.pc, slot, index as u32]),
        );
        self.services.sort_unstable();
        self.units.push(Some(RetainedUnit {
            code,
            charge: incoming,
        }));
        self.retained_code_bytes += incoming;
        self.rebuild_directory()?;
        Ok(slot)
    }

    fn remove(&mut self, index: usize) {
        let slot = index as u32 + 1;
        self.entries.retain(|entry| entry[1] != slot);
        self.services.retain(|entry| entry[1] != slot);
        self.victims.push(slot);
        self.retained_code_bytes -= self.units[index].as_ref().unwrap().charge;
        self.units[index] = None;
        for (index, unit) in self
            .units
            .iter()
            .enumerate()
            .filter_map(|(index, code)| code.as_ref().map(|code| (index, code)))
        {
            for &pc in &unit.code.entries {
                if let Err(at) = self.entries.binary_search_by_key(&pc, |entry| entry[0]) {
                    self.entries.insert(at, [pc, index as u32 + 1]);
                }
            }
        }
    }

    fn evict_oldest(&mut self) -> Result<bool, HostError> {
        while self.oldest < self.units.len() {
            let index = self.oldest;
            self.oldest += 1;
            if self.units[index].is_some() {
                self.directory.fill([0; 2]);
                self.remove(index);
                self.rebuild_directory()?;
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn rebuild_directory(&mut self) -> Result<(), HostError> {
        let length = directory_size(self.entries.len())?;
        if length > self.directory.capacity() {
            return Err(HostError::Infrastructure);
        }
        self.directory.resize(length, [0; 2]);
        self.directory.fill([0; 2]);
        for &[pc, slot] in &self.entries {
            let mut index = ((pc >> 1) ^ (pc >> 12)) as usize & (length - 1);
            while self.directory[index][1] != 0 {
                index = (index + 1) & (length - 1);
            }
            self.directory[index] = [pc, slot];
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        cpu::runtime::compile,
        memory::{GuestAddress, PageRange, Permissions},
    };

    fn fixture() -> AddressSpace {
        let mut memory = AddressSpace::new(2).unwrap();
        memory
            .map_zeroed(
                PageRange::new(GuestAddress(0x1000), 1).unwrap(),
                Permissions::ALL,
            )
            .unwrap();
        memory
            .map_zeroed(
                PageRange::new(GuestAddress(0x3000), 1).unwrap(),
                Permissions::READ_WRITE,
            )
            .unwrap();
        memory.write(GuestAddress(0x1000), &[0x90; 16]).unwrap();
        memory
    }

    fn unit(memory: &AddressSpace, pc: u32) -> CompiledUnit {
        compile(memory, GuestAddress(pc), 1).unwrap()
    }

    fn recomputed_bytes(cache: &CodeCache) -> usize {
        let code = cache
            .units
            .iter()
            .flatten()
            .map(|unit| {
                let code = &unit.code;
                code.wasm.capacity()
                    + code.entries.capacity() * size_of::<u32>()
                    + code.services.capacity() * size_of::<ServiceSite>()
                    + code.sources.capacity() * size_of::<CodeSnapshot>()
                    + code
                        .sources
                        .iter()
                        .map(|source| source.versions.len() * size_of::<PageVersion>())
                        .sum::<usize>()
            })
            .sum::<usize>();
        code + cache.units.capacity() * size_of::<Option<RetainedUnit>>()
            + cache.entries.capacity() * size_of::<[u32; 2]>()
            + cache.services.capacity() * size_of::<[u32; 3]>()
            + cache.directory.capacity() * size_of::<[u32; 2]>()
            + cache.victims.capacity() * size_of::<u32>()
            + cache.dispatcher.capacity()
    }

    #[test]
    fn retained_charge_matches_allocations_after_admission_removal_and_compaction() {
        let mut memory = fixture();
        let mut cache = CodeCache::new().unwrap();
        assert_eq!(cache.charged_bytes(), recomputed_bytes(&cache));
        let mut first = unit(&memory, 0x1000);
        first.wasm.reserve_exact(73);
        first.entries.reserve_exact(11);
        first.sources.reserve_exact(9);
        let first = cache.install(first, &memory).unwrap();
        assert_eq!(cache.charged_bytes(), recomputed_bytes(&cache));
        let second = cache.install(unit(&memory, 0x1001), &memory).unwrap();
        assert_eq!(cache.charged_bytes(), recomputed_bytes(&cache));
        assert!(cache.evict_oldest().unwrap());
        assert!(cache.selected(first).is_none());
        assert!(cache.selected(second).is_some());
        assert_eq!(cache.charged_bytes(), recomputed_bytes(&cache));
        assert_eq!(cache.take_victim(), first);
        assert_eq!(cache.charged_bytes(), recomputed_bytes(&cache));
        memory.write(GuestAddress(0x1001), &[0xc3]).unwrap();
        cache.refresh(&memory).unwrap();
        assert!(cache.selected(second).is_none());
        assert_eq!(cache.charged_bytes(), recomputed_bytes(&cache));
        assert_eq!(cache.take_victim(), second);
        assert_eq!(cache.take_victim(), 0);
        cache.entries.reserve_exact(512);
        cache.directory.reserve_exact(512);
        cache.victims.reserve_exact(64);
        assert_eq!(cache.charged_bytes(), recomputed_bytes(&cache));
        let spare = cache.entries.capacity() + cache.directory.capacity();
        cache.limit = cache.charged_bytes();
        let third = cache.install(unit(&memory, 0x1002), &memory).unwrap();
        assert!(third > second);
        assert!(cache.entries.capacity() + cache.directory.capacity() < spare);
        assert_eq!(cache.charged_bytes(), recomputed_bytes(&cache));
        assert!(cache.charged_bytes() <= cache.limit);
    }

    #[test]
    fn unchanged_code_epoch_preserves_directory_and_skips_source_checks() {
        let mut memory = fixture();
        let mut cache = CodeCache::new().unwrap();
        let slot = cache.install(unit(&memory, 0x1000), &memory).unwrap();
        let before = (
            cache.directory_ptr(),
            cache.dispatcher_ptr(),
            cache.charged_bytes(),
            cache.source_checks,
        );
        for _ in 0..3 {
            cache.refresh(&memory).unwrap();
        }
        memory.write(GuestAddress(0x3000), &[1]).unwrap();
        cache.refresh(&memory).unwrap();
        assert_eq!(
            (
                cache.directory_ptr(),
                cache.dispatcher_ptr(),
                cache.charged_bytes(),
                cache.source_checks
            ),
            before
        );
        assert_eq!(cache.lookup(0x1000), Some(slot));
    }

    #[test]
    fn source_invalidation_revokes_entries_drops_bytes_and_queues_one_victim() {
        let mut memory = fixture();
        let mut cache = CodeCache::new().unwrap();
        let slot = cache.install(unit(&memory, 0x1000), &memory).unwrap();
        let before = cache.charged_bytes();
        memory.write(GuestAddress(0x1000), &[0xc3]).unwrap();
        cache.refresh(&memory).unwrap();
        assert_eq!(cache.lookup(0x1000), None);
        assert!(cache.selected(slot).is_none());
        assert!(cache.directory.iter().all(|entry| entry[1] != slot));
        assert!(cache.charged_bytes() < before);
        assert_eq!(cache.take_victim(), slot);
        cache.refresh(&memory).unwrap();
        assert_eq!(cache.take_victim(), 0);
    }

    #[test]
    fn pressure_evicts_oldest_code_without_reusing_its_slot() {
        let memory = fixture();
        let mut cache = CodeCache::new().unwrap();
        let first = cache.install(unit(&memory, 0x1000), &memory).unwrap();
        let second = unit(&memory, 0x1001);
        cache.limit = cache.charged_bytes() + code_bytes(&second);
        let second = cache.install(second, &memory).unwrap();
        assert!(second > first);
        assert_eq!(cache.lookup(0x1000), None);
        assert_eq!(cache.lookup(0x1001), Some(second));
        assert!(cache.selected(first).is_none());
        assert!(cache.charged_bytes() <= cache.limit);
        assert_eq!(cache.take_victim(), first);
        assert_eq!(cache.take_victim(), 0);
    }

    #[test]
    fn oversized_candidate_and_foreign_memory_do_not_evict_current_code() {
        let memory = fixture();
        let mut cache = CodeCache::new().unwrap();
        let slot = cache.install(unit(&memory, 0x1000), &memory).unwrap();
        cache.limit = cache.charged_bytes();
        let mut oversized = unit(&memory, 0x1001);
        oversized.wasm.resize(cache.limit + 1, 0);
        let before = (cache.charged_bytes(), cache.directory_ptr());
        assert!(cache.install(oversized, &memory).is_err());
        assert!(cache.refresh(&fixture()).is_err());
        assert_eq!((cache.charged_bytes(), cache.directory_ptr()), before);
        assert_eq!(cache.lookup(0x1000), Some(slot));
        assert!(cache.selected(slot).is_some());
        assert_eq!(cache.take_victim(), 0);
    }

    #[test]
    fn charge_includes_spare_capacities_snapshot_boxes_and_dispatcher() {
        let memory = fixture();
        let mut code = unit(&memory, 0x1000);
        code.wasm.reserve_exact(73);
        code.sources.reserve_exact(9);
        code.entries.reserve_exact(11);
        let retained = code_bytes(&code);
        let mut cache = CodeCache::new().unwrap();
        cache.install(code, &memory).unwrap();
        assert_eq!(
            cache.charged_bytes(),
            retained
                + cache.dispatcher.capacity()
                + cache.units.capacity() * size_of::<Option<RetainedUnit>>()
                + cache.entries.capacity() * size_of::<[u32; 2]>()
                + cache.services.capacity() * size_of::<[u32; 3]>()
                + cache.directory.capacity() * size_of::<[u32; 2]>()
                + cache.victims.capacity() * size_of::<u32>()
        );
    }

    #[test]
    fn eviction_rebinds_an_overlapping_entry_to_its_retained_owner() {
        let mut memory = fixture();
        memory.write(GuestAddress(0x1000), &[0xeb, 0]).unwrap();
        let mut cache = CodeCache::new().unwrap();
        let first = cache.install(unit(&memory, 0x1002), &memory).unwrap();
        let second = cache
            .install(compile(&memory, GuestAddress(0x1000), 2).unwrap(), &memory)
            .unwrap();
        assert_eq!(cache.lookup(0x1002), Some(first));
        cache.evict_oldest().unwrap();
        assert_eq!(cache.lookup(0x1002), Some(second));
        assert!(cache.selected(second).unwrap().entries.contains(&0x1002));
        assert_eq!(cache.take_victim(), first);
        assert_eq!(cache.take_victim(), 0);
    }

    #[test]
    fn admission_retries_after_reclaiming_spare_capacity_without_live_victims() {
        let memory = fixture();
        let mut cache = CodeCache::new().unwrap();
        let first = cache.install(unit(&memory, 0x1000), &memory).unwrap();
        assert!(cache.evict_oldest().unwrap());
        assert_eq!(cache.take_victim(), first);
        cache.entries.reserve_exact(512);
        cache.directory.reserve_exact(512);
        let spare = cache.entries.capacity() + cache.directory.capacity();
        cache.limit = cache.charged_bytes();
        let second = cache
            .install(unit(&memory, 0x1001), &memory)
            .expect("spare capacity must be reclaimed before rejecting a candidate that fits");
        assert!(second > first);
        assert_eq!(cache.lookup(0x1001), Some(second));
        assert!(cache.entries.capacity() + cache.directory.capacity() < spare);
        assert!(cache.charged_bytes() <= cache.limit);
    }

    fn service_unit(memory: &AddressSpace, entry: u32, count: usize) -> CompiledUnit {
        compile(memory, GuestAddress(entry), count).unwrap()
    }

    #[test]
    fn service_lookup_finds_a_current_inline_interior_without_selected_slot() {
        let mut memory = fixture();
        memory
            .write(GuestAddress(0x1000), &[0x90, 0xd9, 0xe8, 0x90])
            .unwrap();
        let code = service_unit(&memory, 0x1000, 3);
        let expected = code.services[0];
        let mut cache = CodeCache::new().unwrap();
        let slot = cache.install(code, &memory).unwrap();
        assert_eq!(cache.lookup(expected.pc), None);
        assert_eq!(cache.services, [[expected.pc, slot, 0]]);
        assert_eq!(cache.charged_bytes(), recomputed_bytes(&cache));
        let checks = cache.source_checks;
        assert_eq!(cache.service(&memory, expected.pc).unwrap(), Some(expected));
        assert_eq!(cache.service(&memory, expected.pc).unwrap(), Some(expected));
        memory.write(GuestAddress(0x3000), &[1]).unwrap();
        assert_eq!(cache.service(&memory, expected.pc).unwrap(), Some(expected));
        assert_eq!(cache.source_checks, checks);
        assert_eq!(cache.service(&memory, 0x1000).unwrap(), None);
    }

    #[test]
    fn duplicate_service_owners_survive_eviction_without_reusing_slots() {
        let mut memory = fixture();
        memory
            .write(GuestAddress(0x1000), &[0x90, 0xd9, 0xe8, 0x90])
            .unwrap();
        let mut cache = CodeCache::new().unwrap();
        let code = service_unit(&memory, 0x1001, 1);
        let expected = code.services[0];
        let first = cache.install(code, &memory).unwrap();
        let second = cache
            .install(service_unit(&memory, 0x1000, 3), &memory)
            .unwrap();
        assert_eq!(cache.services.len(), 2);
        assert!(cache.evict_oldest().unwrap());
        assert_eq!(cache.services, [[expected.pc, second, 0]]);
        assert_eq!(cache.service(&memory, expected.pc).unwrap(), Some(expected));
        assert!(cache.selected(first).is_none());
        assert_eq!(cache.take_victim(), first);
        assert_eq!(cache.take_victim(), 0);
        assert!(cache.evict_oldest().unwrap());
        assert_eq!(cache.service(&memory, expected.pc).unwrap(), None);
        assert_eq!(cache.take_victim(), second);
        assert_eq!(cache.take_victim(), 0);
        assert_eq!(cache.charged_bytes(), recomputed_bytes(&cache));
    }

    #[test]
    fn executable_alias_store_and_mapping_changes_revoke_service_descriptors() {
        let mut memory = AddressSpace::new(2).unwrap();
        let shared = memory.allocate_shared_pages(1, &[0]).unwrap();
        for pc in [0x1000, 0x5000] {
            memory
                .map_shared_pages(
                    PageRange::new(GuestAddress(pc), 1).unwrap(),
                    &shared,
                    0,
                    Permissions::ALL,
                    false,
                )
                .unwrap();
        }
        memory.write(GuestAddress(0x1000), &[0xd9, 0xe8]).unwrap();
        let mut cache = CodeCache::new().unwrap();
        let first = cache
            .install(service_unit(&memory, 0x1000, 1), &memory)
            .unwrap();
        memory.write(GuestAddress(0x5001), &[0xee]).unwrap();
        assert_eq!(cache.service(&memory, 0x1000).unwrap(), None);
        assert_eq!(cache.take_victim(), first);
        assert!(cache.services.is_empty());
        let replacement = service_unit(&memory, 0x1000, 1);
        let expected = replacement.services[0];
        assert!(matches!(
            expected.operation,
            crate::cpu::x86::ir::Operation::X87 {
                operation: crate::cpu::x86::X87Operation::LoadConstant { index: 6 },
                ..
            }
        ));
        let second = cache.install(replacement, &memory).unwrap();
        assert_eq!(cache.service(&memory, 0x1000).unwrap(), Some(expected));
        memory
            .protect(
                PageRange::new(GuestAddress(0x1000), 1).unwrap(),
                Permissions::READ_WRITE,
            )
            .unwrap();
        assert_eq!(cache.service(&memory, 0x1000).unwrap(), None);
        assert_eq!(cache.take_victim(), second);
        memory
            .protect(
                PageRange::new(GuestAddress(0x1000), 1).unwrap(),
                Permissions::ALL,
            )
            .unwrap();
        let third = cache
            .install(service_unit(&memory, 0x1000, 1), &memory)
            .unwrap();
        memory
            .unmap(PageRange::new(GuestAddress(0x1000), 1).unwrap())
            .unwrap();
        memory
            .map_zeroed(
                PageRange::new(GuestAddress(0x1000), 1).unwrap(),
                Permissions::ALL,
            )
            .unwrap();
        memory.write(GuestAddress(0x1000), &[0xd9, 0xe8]).unwrap();
        assert_eq!(cache.service(&memory, 0x1000).unwrap(), None);
        assert_eq!(cache.take_victim(), third);
        assert!(cache.services.is_empty());
        assert!(cache.service(&fixture(), 0x1000).is_err());
    }

    #[test]
    fn service_metadata_capacities_are_charged_before_admission_or_eviction() {
        let mut memory = fixture();
        memory
            .write(GuestAddress(0x1000), &[0xd9, 0xe8, 0xd9, 0xee])
            .unwrap();
        let mut cache = CodeCache::new().unwrap();
        let mut code = service_unit(&memory, 0x1000, 1);
        code.services.reserve_exact(13);
        let first = cache.install(code, &memory).unwrap();
        assert_eq!(cache.charged_bytes(), recomputed_bytes(&cache));
        let before = (
            cache.charged_bytes(),
            cache.directory_ptr(),
            cache.services.clone(),
        );
        cache.limit = cache.charged_bytes();
        let mut oversized = service_unit(&memory, 0x1002, 1);
        oversized
            .services
            .reserve_exact(cache.limit / size_of::<ServiceSite>() + 1);
        assert!(cache.install(oversized, &memory).is_err());
        assert_eq!(
            (
                cache.charged_bytes(),
                cache.directory_ptr(),
                cache.services.clone()
            ),
            before
        );
        assert!(cache.selected(first).is_some());
        assert_eq!(cache.take_victim(), 0);
        let second = cache
            .install(service_unit(&memory, 0x1002, 1), &memory)
            .unwrap();
        assert!(second > first);
        assert!(cache.charged_bytes() <= cache.limit);
        assert_eq!(cache.charged_bytes(), recomputed_bytes(&cache));
        assert_eq!(cache.service(&memory, 0x1002).unwrap().unwrap().pc, 0x1002);
    }

    #[test]
    fn impossible_monotonic_slot_growth_rejects_before_evicting_current_code() {
        let mut memory = fixture();
        memory.write(GuestAddress(0x1000), &[0xd9, 0xe8]).unwrap();
        let mut cache = CodeCache::new().unwrap();
        for _ in 0..16 {
            let slot = cache
                .install(service_unit(&memory, 0x1000, 1), &memory)
                .unwrap();
            assert!(cache.evict_oldest().unwrap());
            assert_eq!(cache.take_victim(), slot);
            assert_eq!(cache.take_victim(), 0);
        }
        let current = cache
            .install(service_unit(&memory, 0x1000, 1), &memory)
            .unwrap();
        let before = (
            cache.directory.clone(),
            cache.entries.clone(),
            cache.services.clone(),
            cache.charged_bytes(),
        );
        let mut candidate = service_unit(&memory, 0x1000, 1);
        candidate.wasm.reserve_exact(cache.charged_bytes());
        let incoming = code_bytes(&candidate);
        let common = incoming
            + cache.dispatcher.capacity()
            + candidate.entries.len() * size_of::<[u32; 2]>()
            + candidate.services.len() * size_of::<[u32; 3]>()
            + directory_size(candidate.entries.len()).unwrap() * size_of::<[u32; 2]>();
        let count = cache.units.len() + 1;
        // Eviction can release payloads, but monotonic slot headers remain and
        // admission reserves a victim entry for every slot in that history.
        let irreducible =
            common + count * size_of::<Option<RetainedUnit>>() + count * size_of::<u32>();
        cache.limit = irreducible - 1;
        assert!(before.3 <= cache.limit, "current cache must already fit");
        assert!(
            common + size_of::<Option<RetainedUnit>>() + size_of::<u32>() <= cache.limit,
            "candidate must pass the former fresh-cache estimate"
        );
        assert!(cache.install(candidate, &memory).is_err());
        assert!(
            cache.selected(current).is_some(),
            "an impossible candidate must not evict current code"
        );
        assert_eq!(
            (
                cache.directory.clone(),
                cache.entries.clone(),
                cache.services.clone(),
                cache.charged_bytes()
            ),
            before
        );
        assert_eq!(cache.take_victim(), 0);
    }
}
