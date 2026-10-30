use crate::{
    cpu::runtime::{CompiledUnit, emit_dispatch},
    memory::{AddressSpace, CodeSnapshot, PageVersion},
    process::HostError,
};
use std::mem::size_of;

const BYTE_LIMIT: usize = 128 * 1024 * 1024;
const MIN_DIRECTORY: usize = 16;

pub(super) struct CodeCache {
    units: Vec<Option<RetainedUnit>>,
    entries: Vec<[u32; 2]>,
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
        if code.entries.is_empty() || code.sources.is_empty() || !code.is_current(memory) {
            return Err(HostError::InvalidArtifact);
        }
        let incoming = code_bytes(&code);
        if incoming
            .saturating_add(self.dispatcher.capacity())
            .saturating_add(MIN_DIRECTORY * size_of::<[u32; 2]>())
            > self.limit
        {
            return Err(HostError::Infrastructure);
        }
        self.refresh(memory)?;
        let count = self
            .units
            .len()
            .checked_add(1)
            .ok_or(HostError::Infrastructure)?;
        let slot = u32::try_from(count).map_err(|_| HostError::Infrastructure)?;
        let mut compacted = false;
        loop {
            let entries = self
                .entries
                .len()
                .checked_add(code.entries.len())
                .ok_or(HostError::Infrastructure)?;
            let directory = directory_size(entries)?;
            let extra = incoming
                .saturating_add(growth(&self.units, count))
                .saturating_add(growth(&self.entries, entries))
                .saturating_add(growth(&self.directory, directory))
                .saturating_add(growth(&self.victims, count));
            if self.charged_bytes().saturating_add(extra) <= self.limit {
                reserve(&mut self.units, count)?;
                reserve(&mut self.entries, entries)?;
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
                self.directory.shrink_to_fit();
                self.victims.shrink_to_fit();
                compacted = true;
            }
        }
        self.entries
            .extend(code.entries.iter().copied().map(|pc| [pc, slot]));
        self.entries.sort_unstable();
        self.entries.dedup_by_key(|entry| entry[0]);
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
}
