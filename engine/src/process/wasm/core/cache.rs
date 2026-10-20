use crate::{
    cpu::core::CompiledCode,
    memory::{AddressSpace, GuestAddress},
};
use std::{
    collections::HashMap,
    rc::{Rc, Weak},
};

#[path = "promotion.rs"]
mod promotion;
use promotion::{PendingTransition, TransitionHeat};

pub(super) const TRANSITION_HITS: u32 = 4096;

const MAX_REGIONS: usize = 1024;
const MAX_CODE_BYTES: usize = 32 << 20;
const MAX_LINK_SLOT: usize = 4096;

pub(super) struct CoreCache32 {
    regions: HashMap<u64, CachedRegion>,
    entries: HashMap<u32, Weak<CompiledCode>>,
    bytes: usize,
    oldest: u64,
    newest: u64,
    link_owners: Box<[u64]>,
    compiled: u64,
    admitted_encoded_bytes: u64,
    count_evictions: u64,
    byte_evictions: u64,
    source_invalidations: u64,
    pending_transition: Option<PendingTransition>,
}

struct CachedRegion {
    code: Rc<CompiledCode>,
    older: u64,
    newer: u64,
    slot: u32,
    heat: TransitionHeat,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum LinkError {
    InvalidSlot,
    Unretained,
    Occupied,
    AlreadyLinked,
    WrongOwner,
    Inconsistent,
}

impl Default for CoreCache32 {
    fn default() -> Self {
        Self {
            regions: HashMap::new(),
            entries: HashMap::new(),
            bytes: 0,
            oldest: 0,
            newest: 0,
            link_owners: vec![0; MAX_LINK_SLOT + 1].into_boxed_slice(),
            compiled: 0,
            admitted_encoded_bytes: 0,
            count_evictions: 0,
            byte_evictions: 0,
            source_invalidations: 0,
            pending_transition: None,
        }
    }
}

impl CoreCache32 {
    pub(super) fn frontier_contains(&self, memory: &AddressSpace, pc: u32) -> bool {
        let Some(code) = self.entries.get(&pc).and_then(Weak::upgrade) else {
            return false;
        };
        self.regions
            .get(&code.identity())
            .is_some_and(|entry| Rc::ptr_eq(&entry.code, &code) && code.is_current(memory))
    }

    pub(super) fn frontier(&self, memory: &AddressSpace) -> Vec<GuestAddress> {
        let mut frontier = Vec::new();
        for entry in self.regions.values() {
            if !entry.code.is_current(memory) {
                continue;
            }
            let owner = Rc::downgrade(&entry.code);
            for pc in entry.code.entrypoints() {
                if self
                    .entries
                    .get(&pc.0)
                    .is_some_and(|indexed| indexed.ptr_eq(&owner))
                {
                    frontier.push(*pc);
                }
            }
        }
        frontier
    }

    pub(super) fn link_owners_ptr(&self) -> *const u64 {
        self.link_owners.as_ptr()
    }

    pub(super) fn oldest_linked_slot(&self) -> u32 {
        let mut identity = self.oldest;
        while identity != 0 {
            let entry = &self.regions[&identity];
            if entry.slot != 0 && self.link_owners[entry.slot as usize] == identity {
                return entry.slot;
            }
            identity = entry.newer;
        }
        0
    }

    pub(super) fn snapshot(&self) -> [u64; 8] {
        [
            self.compiled,
            self.admitted_encoded_bytes,
            self.count_evictions,
            self.byte_evictions,
            self.source_invalidations,
            self.regions.len() as u64,
            self.bytes as u64,
            MAX_REGIONS as u64,
        ]
    }

    pub(super) fn lookup(&mut self, memory: &AddressSpace, pc: u32) -> Option<Rc<CompiledCode>> {
        let code = self.entries.get(&pc)?.upgrade()?;
        let retained = self.regions.get(&code.identity())?;
        if !Rc::ptr_eq(&retained.code, &code) {
            return None;
        }
        if !code.is_current(memory) {
            self.remove(code.identity());
            self.source_invalidations = self.source_invalidations.saturating_add(1);
            return None;
        }
        self.touch(code.identity());
        Some(code)
    }

    pub(super) fn lookup_linked(
        &mut self,
        memory: &AddressSpace,
        pc: u32,
    ) -> Option<(Rc<CompiledCode>, u32)> {
        let code = self.lookup(memory, pc)?;
        let slot = self.regions.get(&code.identity())?.slot;
        if slot == 0 || self.link_owners[slot as usize] != code.identity() {
            return None;
        }
        Some((code, slot))
    }

    pub(super) fn install_link(
        &mut self,
        code: &Rc<CompiledCode>,
        slot: u32,
    ) -> Result<(), LinkError> {
        let owner = *self
            .link_owners
            .get(slot as usize)
            .filter(|_| slot != 0)
            .ok_or(LinkError::InvalidSlot)?;
        let identity = code.identity();
        let retained = self
            .regions
            .get(&identity)
            .filter(|entry| Rc::ptr_eq(&entry.code, code))
            .ok_or(LinkError::Unretained)?;
        if retained.slot != 0 && retained.slot != slot {
            return Err(LinkError::AlreadyLinked);
        }
        if owner != 0 && owner != identity {
            return Err(LinkError::Occupied);
        }
        if retained.slot == slot {
            return if owner == identity {
                Ok(())
            } else {
                Err(LinkError::Inconsistent)
            };
        }
        if owner != 0 {
            return Err(LinkError::Inconsistent);
        }
        self.regions.get_mut(&identity).unwrap().slot = slot;
        self.link_owners[slot as usize] = identity;
        Ok(())
    }

    pub(super) fn unlink(&mut self, slot: u32, expected: u64) -> Result<(), LinkError> {
        let owner = *self
            .link_owners
            .get(slot as usize)
            .filter(|_| slot != 0)
            .ok_or(LinkError::InvalidSlot)?;
        if owner == 0 {
            return Ok(());
        }
        if owner != expected {
            return Err(LinkError::WrongOwner);
        }
        let retained = self
            .regions
            .get_mut(&owner)
            .filter(|entry| entry.slot == slot)
            .ok_or(LinkError::Inconsistent)?;
        retained.slot = 0;
        self.link_owners[slot as usize] = 0;
        Ok(())
    }

    pub(super) fn insert(&mut self, code: CompiledCode) -> Rc<CompiledCode> {
        let code = Rc::new(code);
        while self.regions.len() >= MAX_REGIONS || self.bytes + code.wasm().len() > MAX_CODE_BYTES {
            if self.oldest == 0 {
                break;
            }
            let byte_pressure = self.bytes + code.wasm().len() > MAX_CODE_BYTES;
            self.remove(self.oldest);
            if byte_pressure {
                self.byte_evictions = self.byte_evictions.saturating_add(1);
            } else {
                self.count_evictions = self.count_evictions.saturating_add(1);
            }
        }
        for entry in code.entrypoints() {
            self.entries.insert(entry.0, Rc::downgrade(&code));
        }
        self.bytes += code.wasm().len();
        let identity = code.identity();
        self.regions.insert(
            identity,
            CachedRegion {
                code: Rc::clone(&code),
                older: self.newest,
                newer: 0,
                slot: 0,
                heat: TransitionHeat::default(),
            },
        );
        if self.newest == 0 {
            self.oldest = identity;
        } else {
            self.regions.get_mut(&self.newest).unwrap().newer = identity;
        }
        self.newest = identity;
        self.compiled = self.compiled.saturating_add(1);
        self.admitted_encoded_bytes = self
            .admitted_encoded_bytes
            .saturating_add(code.wasm().len() as u64);
        code
    }

    fn touch(&mut self, identity: u64) {
        if self.newest == identity {
            return;
        }
        let entry = self.regions.get(&identity).unwrap();
        let (older, newer) = (entry.older, entry.newer);
        self.detach(older, newer);
        self.regions.get_mut(&self.newest).unwrap().newer = identity;
        let entry = self.regions.get_mut(&identity).unwrap();
        entry.older = self.newest;
        entry.newer = 0;
        self.newest = identity;
    }

    fn detach(&mut self, older: u64, newer: u64) {
        if older == 0 {
            self.oldest = newer;
        } else {
            self.regions.get_mut(&older).unwrap().newer = newer;
        }
        if newer == 0 {
            self.newest = older;
        } else {
            self.regions.get_mut(&newer).unwrap().older = older;
        }
    }

    fn remove(&mut self, identity: u64) {
        let Some(entry) = self.regions.remove(&identity) else {
            return;
        };
        self.detach(entry.older, entry.newer);
        self.bytes -= entry.code.wasm().len();
        if entry.slot != 0 && self.link_owners[entry.slot as usize] == identity {
            self.link_owners[entry.slot as usize] = 0;
        }
        for pc in entry.code.entrypoints() {
            if self
                .entries
                .get(&pc.0)
                .is_some_and(|weak| weak.ptr_eq(&Rc::downgrade(&entry.code)))
            {
                self.entries.remove(&pc.0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        cpu::core::{CompileOptions, compile},
        memory::{GuestAddress, PageRange, Permissions},
    };

    fn memory() -> AddressSpace {
        let filler_bytes = (MAX_REGIONS + 64) * 8;
        let filler_pages = filler_bytes.div_ceil(4096);
        let mut memory = AddressSpace::new((2 + filler_pages) as u32).unwrap();
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
        let mut code = [0; 4096];
        code[..15].copy_from_slice(&[
            0x83, 0xc0, 1, 0x75, 5, 0x83, 0xc0, 2, 0xeb, 3, 0x83, 0xc0, 3, 0x0f, 0x0b,
        ]);
        memory.write(GuestAddress(0x1000), &code).unwrap();
        memory
            .map_zeroed(
                PageRange::new(GuestAddress(0x4000), filler_pages as u32).unwrap(),
                Permissions::ALL,
            )
            .unwrap();
        let mut fillers = vec![0; filler_pages * 4096];
        for index in 0..MAX_REGIONS + 64 {
            fillers[index * 8..index * 8 + 3].copy_from_slice(&[0x90, 0x0f, 0x0b]);
        }
        memory.write(GuestAddress(0x4000), &fillers).unwrap();
        memory
    }

    fn compiled(memory: &AddressSpace, pc: u32) -> CompiledCode {
        compile(
            memory,
            GuestAddress(pc),
            CompileOptions {
                instructions: 32,
                blocks: 8,
                ..CompileOptions::default()
            },
        )
        .unwrap()
    }

    fn filler(index: usize) -> u32 {
        0x4000 + index as u32 * 8
    }

    fn bounds(cache: &CoreCache32) {
        assert!(cache.regions.len() <= MAX_REGIONS);
        assert!(cache.bytes <= MAX_CODE_BYTES);
        assert_eq!(
            cache.bytes,
            cache
                .regions
                .values()
                .map(|entry| entry.code.wasm().len())
                .sum::<usize>()
        );
        assert!(
            cache
                .entries
                .values()
                .all(|entry| entry.upgrade().is_some())
        );
        let mut seen = std::collections::HashSet::new();
        let (mut previous, mut current) = (0, cache.oldest);
        while current != 0 {
            assert!(seen.insert(current), "LRU has a cycle");
            let entry = cache.regions.get(&current).unwrap();
            assert_eq!(entry.older, previous);
            previous = current;
            current = entry.newer;
        }
        assert_eq!(previous, cache.newest);
        assert_eq!(seen.len(), cache.regions.len());
        assert_eq!(cache.link_owners[0], 0);
        for (&identity, entry) in &cache.regions {
            assert_eq!(entry.code.identity(), identity);
            if entry.slot != 0 {
                assert_eq!(cache.link_owners[entry.slot as usize], identity);
            }
        }
        for (slot, &identity) in cache.link_owners.iter().enumerate().skip(1) {
            if identity != 0 {
                assert_eq!(cache.regions.get(&identity).unwrap().slot as usize, slot);
            }
        }
    }

    #[test]
    fn source_changes_and_identical_remapping_invalidate_cached_entries() {
        let mut memory = memory();
        let mut cache = CoreCache32::default();
        let first = cache.insert(compiled(&memory, 0x1000));
        memory.write(GuestAddress(0x3000), &[7]).unwrap();
        assert!(Rc::ptr_eq(&cache.lookup(&memory, 0x1000).unwrap(), &first));
        memory.write(GuestAddress(0x1002), &[4]).unwrap();
        assert!(cache.lookup(&memory, 0x1000).is_none());
        assert!(cache.regions.is_empty());
        for entry in first.entrypoints() {
            assert!(!cache.entries.contains_key(&entry.0));
        }

        let second = cache.insert(compiled(&memory, 0x1000));
        let mut code = [0; 4096];
        memory.read(GuestAddress(0x1000), &mut code).unwrap();
        let range = PageRange::new(GuestAddress(0x1000), 1).unwrap();
        memory.unmap(range).unwrap();
        memory.map_zeroed(range, Permissions::ALL).unwrap();
        memory.write(GuestAddress(0x1000), &code).unwrap();
        assert!(!second.is_current(&memory));
        assert!(cache.lookup(&memory, 0x1005).is_none());
        assert!(cache.regions.is_empty());
        bounds(&cache);
    }

    #[test]
    fn actual_compiled_side_entries_reuse_the_same_retained_region() {
        let memory = memory();
        let mut cache = CoreCache32::default();
        let code = cache.insert(compiled(&memory, 0x1000));
        assert_eq!(
            code.entrypoints(),
            &[
                GuestAddress(0x1000),
                GuestAddress(0x1005),
                GuestAddress(0x100a),
                GuestAddress(0x100d)
            ]
        );
        for pc in [0x1000, 0x1005, 0x100a, 0x100d] {
            assert!(Rc::ptr_eq(&cache.lookup(&memory, pc).unwrap(), &code));
        }
        assert!(cache.lookup(&memory, 0x1001).is_none());
        assert_eq!(cache.regions.len(), 1);
        bounds(&cache);
    }

    #[test]
    fn lru_touch_preserves_the_touched_region_and_eviction_stays_bounded() {
        let memory = memory();
        let mut cache = CoreCache32::default();
        let first = cache.insert(compiled(&memory, filler(0)));
        for index in 1..MAX_REGIONS {
            cache.insert(compiled(&memory, filler(index)));
        }
        assert_eq!(cache.regions.len(), MAX_REGIONS);
        assert!(Rc::ptr_eq(
            &cache.lookup(&memory, filler(0)).unwrap(),
            &first
        ));
        cache.insert(compiled(&memory, filler(MAX_REGIONS)));
        assert!(cache.lookup(&memory, filler(1)).is_none());
        assert!(Rc::ptr_eq(
            &cache.lookup(&memory, filler(0)).unwrap(),
            &first
        ));
        for index in MAX_REGIONS + 1..MAX_REGIONS + 16 {
            cache.insert(compiled(&memory, filler(index)));
            bounds(&cache);
        }
        assert_eq!(cache.regions.len(), MAX_REGIONS);
    }

    #[test]
    fn newer_side_entry_alias_survives_eviction_of_the_previous_region() {
        let memory = memory();
        let mut cache = CoreCache32::default();
        let old = cache.insert(compiled(&memory, 0x1000));
        let newer = cache.insert(compiled(&memory, 0x1005));
        assert_ne!(old.identity(), newer.identity());
        assert_eq!(
            newer.entrypoints(),
            &[GuestAddress(0x1005), GuestAddress(0x100d)]
        );
        for index in 0..MAX_REGIONS - 1 {
            cache.insert(compiled(&memory, filler(index)));
        }
        assert!(
            cache
                .regions
                .values()
                .all(|entry| entry.code.identity() != old.identity())
        );
        assert!(cache.lookup(&memory, 0x1000).is_none());
        assert!(cache.lookup(&memory, 0x100a).is_none());
        for pc in [0x1005, 0x100d] {
            assert!(Rc::ptr_eq(&cache.lookup(&memory, pc).unwrap(), &newer));
        }
        bounds(&cache);
    }

    #[test]
    fn links_require_a_retained_owner_and_accept_the_last_driver_slot() {
        let memory = memory();
        let mut cache = CoreCache32::default();
        let code = cache.insert(compiled(&memory, 0x1000));
        let outside = Rc::new(compiled(&memory, filler(0)));
        assert!(cache.install_link(&code, 0).is_err());
        assert!(cache.install_link(&code, 4097).is_err());
        assert!(cache.install_link(&outside, 1).is_err());
        assert!(cache.lookup_linked(&memory, 0x1000).is_none());
        cache.install_link(&code, 4096).unwrap();
        cache.install_link(&code, 4096).unwrap();
        assert!(cache.install_link(&code, 1).is_err());
        let (selected, slot) = cache.lookup_linked(&memory, 0x1005).unwrap();
        assert!(Rc::ptr_eq(&selected, &code));
        assert_eq!(slot, 4096);
        bounds(&cache);
    }

    #[test]
    fn occupied_and_wrong_owner_link_operations_leave_both_owners_unchanged() {
        let memory = memory();
        let mut cache = CoreCache32::default();
        let first = cache.insert(compiled(&memory, filler(0)));
        let second = cache.insert(compiled(&memory, filler(1)));
        cache.install_link(&first, 1).unwrap();
        assert!(cache.install_link(&second, 1).is_err());
        cache.install_link(&second, 2).unwrap();
        assert!(cache.unlink(1, second.identity()).is_err());
        assert!(cache.unlink(0, first.identity()).is_err());
        assert!(cache.unlink(4097, first.identity()).is_err());
        for (pc, code, expected) in [(filler(0), &first, 1), (filler(1), &second, 2)] {
            let (selected, slot) = cache.lookup_linked(&memory, pc).unwrap();
            assert!(Rc::ptr_eq(&selected, code));
            assert_eq!(slot, expected);
        }
        bounds(&cache);
    }

    #[test]
    fn unlink_is_idempotent_for_an_empty_slot_and_preserves_cached_code() {
        let memory = memory();
        let mut cache = CoreCache32::default();
        let code = cache.insert(compiled(&memory, filler(0)));
        cache.install_link(&code, 1).unwrap();
        cache.unlink(1, code.identity()).unwrap();
        cache.unlink(1, code.identity()).unwrap();
        assert!(cache.lookup_linked(&memory, filler(0)).is_none());
        assert!(Rc::ptr_eq(
            &cache.lookup(&memory, filler(0)).unwrap(),
            &code
        ));
        cache.install_link(&code, 2).unwrap();
        assert_eq!(cache.lookup_linked(&memory, filler(0)).unwrap().1, 2);
        bounds(&cache);
    }

    #[test]
    fn eviction_releases_the_link_but_a_held_current_region_cannot_reinstall_it() {
        let memory = memory();
        let mut cache = CoreCache32::default();
        let old = cache.insert(compiled(&memory, filler(0)));
        cache.install_link(&old, 1).unwrap();
        let mut newest = Rc::clone(&old);
        for index in 1..=MAX_REGIONS {
            newest = cache.insert(compiled(&memory, filler(index)));
        }
        assert_eq!(cache.snapshot()[0], (MAX_REGIONS + 1) as u64);
        assert_eq!(&cache.snapshot()[2..5], &[1, 0, 0]);
        assert!(old.is_current(&memory));
        assert!(!old.wasm().is_empty());
        assert!(cache.lookup_linked(&memory, filler(0)).is_none());
        assert!(cache.install_link(&old, 1).is_err());
        cache.install_link(&newest, 1).unwrap();
        assert!(cache.unlink(1, old.identity()).is_err());
        let (selected, slot) = cache.lookup_linked(&memory, filler(MAX_REGIONS)).unwrap();
        assert!(Rc::ptr_eq(&selected, &newest));
        assert_eq!(slot, 1);
        bounds(&cache);
    }

    #[test]
    fn a_linked_source_change_rejects_and_releases_the_slot_for_new_code() {
        let mut memory = memory();
        let mut cache = CoreCache32::default();
        let old = cache.insert(compiled(&memory, 0x1000));
        cache.install_link(&old, 1).unwrap();
        memory.write(GuestAddress(0x1002), &[4]).unwrap();
        assert!(cache.lookup_linked(&memory, 0x1000).is_none());
        assert!(cache.lookup_linked(&memory, 0x1005).is_none());
        let fresh = cache.insert(compiled(&memory, 0x1000));
        cache.install_link(&fresh, 1).unwrap();
        assert!(cache.unlink(1, old.identity()).is_err());
        let (selected, slot) = cache.lookup_linked(&memory, 0x1000).unwrap();
        assert!(Rc::ptr_eq(&selected, &fresh));
        assert_eq!(slot, 1);
        bounds(&cache);
    }

    #[test]
    fn linked_lookup_is_source_checked_and_touches_the_same_lru() {
        let memory = memory();
        let mut cache = CoreCache32::default();
        let first = cache.insert(compiled(&memory, filler(0)));
        let second = cache.insert(compiled(&memory, filler(1)));
        cache.install_link(&first, 1).unwrap();
        cache.install_link(&second, 2).unwrap();
        for index in 2..MAX_REGIONS {
            cache.insert(compiled(&memory, filler(index)));
        }
        assert!(cache.lookup_linked(&memory, 0xdead_0000).is_none());
        assert!(Rc::ptr_eq(
            &cache.lookup_linked(&memory, filler(0)).unwrap().0,
            &first
        ));
        let newest = cache.insert(compiled(&memory, filler(MAX_REGIONS)));
        assert!(cache.lookup_linked(&memory, filler(1)).is_none());
        assert!(cache.install_link(&second, 2).is_err());
        cache.install_link(&newest, 2).unwrap();
        assert_eq!(
            cache.lookup_linked(&memory, filler(MAX_REGIONS)).unwrap().1,
            2
        );
        assert_eq!(cache.lookup_linked(&memory, filler(0)).unwrap().1, 1);
        bounds(&cache);
    }

    #[test]
    fn more_than_256_small_regions_remain_cached_below_the_byte_limit() {
        let memory = memory();
        let mut cache = CoreCache32::default();
        let first = cache.insert(compiled(&memory, filler(0)));
        cache.install_link(&first, 1).unwrap();
        for index in 1..300 {
            cache.insert(compiled(&memory, filler(index)));
        }
        assert_eq!(cache.regions.len(), 300);
        assert!(cache.bytes < MAX_CODE_BYTES);
        assert_eq!(
            cache.snapshot(),
            [
                300,
                cache.bytes as u64,
                0,
                0,
                0,
                300,
                cache.bytes as u64,
                MAX_REGIONS as u64
            ]
        );
        let (selected, slot) = cache.lookup_linked(&memory, filler(0)).unwrap();
        assert!(Rc::ptr_eq(&selected, &first));
        assert_eq!(slot, 1);
        bounds(&cache);
    }

    #[test]
    fn real_byte_pressure_evicts_and_releases_links_before_the_count_limit() {
        let mut memory = AddressSpace::new(4).unwrap();
        memory
            .map_zeroed(
                PageRange::new(GuestAddress(0x1000), 4).unwrap(),
                Permissions::ALL,
            )
            .unwrap();
        memory
            .write(GuestAddress(0x1000), &[0x83, 0x03, 1].repeat(4096))
            .unwrap();
        let limits = CompileOptions {
            instructions: 4096,
            blocks: 1,
            wasm_bytes: 16 << 20,
        };
        let mut cache = CoreCache32::default();
        let first = cache.insert(compile(&memory, GuestAddress(0x1000), limits).unwrap());
        let bytes = first.wasm().len();
        let count = MAX_CODE_BYTES / bytes + 1;
        assert!(
            count < MAX_REGIONS,
            "byte pressure must precede count pressure"
        );
        cache.install_link(&first, 1).unwrap();
        let mut newest = Rc::clone(&first);
        for index in 1..count {
            newest = cache.insert(compile(&memory, GuestAddress(0x1000), limits).unwrap());
            cache.install_link(&newest, index as u32 + 1).unwrap();
            bounds(&cache);
        }
        assert!(cache.regions.len() < count);
        assert_eq!(
            cache.snapshot(),
            [
                count as u64,
                (count * bytes) as u64,
                0,
                (count - cache.regions.len()) as u64,
                0,
                cache.regions.len() as u64,
                cache.bytes as u64,
                MAX_REGIONS as u64
            ]
        );
        assert!(!cache.regions.contains_key(&first.identity()));
        assert!(cache.install_link(&first, 1).is_err());
        cache.unlink(count as u32, newest.identity()).unwrap();
        cache.install_link(&newest, 1).unwrap();
        assert!(cache.unlink(1, first.identity()).is_err());
        assert_eq!(cache.lookup_linked(&memory, 0x1000).unwrap().1, 1);
        bounds(&cache);
    }

    #[test]
    fn oldest_linked_slot_follows_native_hits_and_skips_unlinked_owners() {
        let memory = memory();
        let mut cache = CoreCache32::default();
        assert_eq!(cache.oldest_linked_slot(), 0);
        let first = cache.insert(compiled(&memory, filler(0)));
        let second = cache.insert(compiled(&memory, filler(1)));
        cache.insert(compiled(&memory, filler(2)));
        assert_eq!(cache.oldest_linked_slot(), 0);
        cache.install_link(&first, 11).unwrap();
        cache.install_link(&second, 29).unwrap();
        assert_eq!(cache.oldest_linked_slot(), 11);

        cache.lookup_linked(&memory, filler(0)).unwrap();
        assert_eq!(cache.oldest_linked_slot(), 29);
        cache.lookup_linked(&memory, filler(1)).unwrap();
        assert_eq!(cache.oldest_linked_slot(), 11);
        cache.unlink(11, first.identity()).unwrap();
        assert_eq!(cache.oldest_linked_slot(), 29);
        cache.unlink(29, second.identity()).unwrap();
        assert_eq!(cache.oldest_linked_slot(), 0);
        bounds(&cache);
    }

    #[test]
    fn cold_snapshot_counts_compilation_and_lazy_source_removal_without_counting_hits() {
        let mut memory = memory();
        let mut cache = CoreCache32::default();
        assert_eq!(cache.snapshot(), [0, 0, 0, 0, 0, 0, 0, 1024]);
        let first = cache.insert(compiled(&memory, 0x1000));
        let second = cache.insert(compiled(&memory, filler(0)));
        let first_bytes = first.wasm().len() as u64;
        let second_bytes = second.wasm().len() as u64;
        let initial = [
            2,
            first_bytes + second_bytes,
            0,
            0,
            0,
            2,
            first_bytes + second_bytes,
            1024,
        ];
        assert_eq!(cache.snapshot(), initial);
        cache.install_link(&first, 1).unwrap();
        for _ in 0..3 {
            cache.lookup(&memory, 0x1000).unwrap();
            cache.lookup_linked(&memory, 0x1005).unwrap();
            assert_eq!(cache.snapshot(), initial);
        }
        cache.unlink(1, first.identity()).unwrap();
        assert_eq!(cache.snapshot(), initial);

        memory.write(GuestAddress(0x1002), &[4]).unwrap();
        assert!(cache.lookup(&memory, 0x1000).is_none());
        assert!(cache.lookup(&memory, 0x1005).is_none());
        let stale = [
            2,
            first_bytes + second_bytes,
            0,
            0,
            1,
            1,
            second_bytes,
            1024,
        ];
        assert_eq!(cache.snapshot(), stale);
        let replacement = cache.insert(compiled(&memory, 0x1000));
        let replacement_bytes = replacement.wasm().len() as u64;
        assert_eq!(
            cache.snapshot(),
            [
                3,
                first_bytes + second_bytes + replacement_bytes,
                0,
                0,
                1,
                2,
                second_bytes + replacement_bytes,
                1024
            ]
        );
        bounds(&cache);
    }

    #[test]
    fn owner_table_pointer_survives_cache_moves_unlink_and_source_removal() {
        let mut memory = memory();
        let mut cache = CoreCache32::default();
        let pointer = cache.link_owners_ptr();
        assert_eq!(pointer, cache.link_owners.as_ptr());
        assert_eq!(cache.link_owners.len(), 4097);
        assert!(cache.link_owners.iter().all(|owner| *owner == 0));
        let old = cache.insert(compiled(&memory, 0x1000));
        cache.install_link(&old, 4096).unwrap();
        assert_eq!(cache.link_owners[4096], old.identity());
        let mut moved = cache;
        assert_eq!(moved.link_owners_ptr(), pointer);
        moved.unlink(4096, old.identity()).unwrap();
        assert_eq!(moved.link_owners[4096], 0);
        moved.install_link(&old, 4096).unwrap();
        memory.write(GuestAddress(0x1002), &[4]).unwrap();
        assert!(moved.lookup_linked(&memory, 0x1000).is_none());
        assert_eq!(moved.link_owners_ptr(), pointer);
        assert_eq!(moved.link_owners[0], 0);
        assert_eq!(moved.link_owners[4096], 0);
        bounds(&moved);
    }

    #[test]
    fn frontier_contains_current_indexed_entries_even_without_installed_links() {
        let memory = memory();
        let mut cache = CoreCache32::default();
        assert!(cache.frontier(&memory).is_empty());
        let first = cache.insert(compiled(&memory, 0x1000));
        let second = cache.insert(compiled(&memory, filler(0)));
        cache.install_link(&second, 7).unwrap();
        let snapshot = cache.snapshot();
        let mut frontier = cache.frontier(&memory);
        frontier.sort_unstable_by_key(|pc| pc.0);
        assert_eq!(
            frontier,
            [0x1000, 0x1005, 0x100a, 0x100d, filler(0)]
                .map(GuestAddress)
                .to_vec()
        );
        assert_eq!(cache.regions[&first.identity()].slot, 0);
        assert_eq!(cache.oldest, first.identity());
        assert_eq!(cache.newest, second.identity());
        assert_eq!(cache.oldest_linked_slot(), 7);
        assert_eq!(cache.snapshot(), snapshot);
        assert_eq!(cache.frontier(&AddressSpace::new(1).unwrap()), []);
        assert_eq!(cache.snapshot(), snapshot);
        bounds(&cache);
    }

    #[test]
    fn frontier_omits_stale_and_identically_remapped_sources_without_removing_owners() {
        for remap in [false, true] {
            let mut memory = memory();
            let mut cache = CoreCache32::default();
            let old = cache.insert(compiled(&memory, 0x1000));
            let current = cache.insert(compiled(&memory, filler(0)));
            cache.install_link(&old, 1).unwrap();
            cache.install_link(&current, 2).unwrap();
            let snapshot = cache.snapshot();
            if remap {
                let mut page = [0; 4096];
                memory.read(GuestAddress(0x1000), &mut page).unwrap();
                let range = PageRange::new(GuestAddress(0x1000), 1).unwrap();
                memory.unmap(range).unwrap();
                memory.map_zeroed(range, Permissions::ALL).unwrap();
                memory.write(GuestAddress(0x1000), &page).unwrap();
            } else {
                memory.write(GuestAddress(0x1002), &[4]).unwrap();
            }
            assert!(!old.is_current(&memory));
            assert_eq!(cache.frontier(&memory), vec![GuestAddress(filler(0))]);
            assert_eq!(cache.snapshot(), snapshot);
            assert!(cache.regions.contains_key(&old.identity()));
            assert_eq!(cache.link_owners[1], old.identity());
            assert_eq!(cache.oldest, old.identity());
            assert_eq!(cache.newest, current.identity());
            bounds(&cache);
        }
    }

    #[test]
    fn frontier_excludes_shadowed_side_entries_after_the_indexed_owner_is_removed() {
        let memory = memory();
        let mut cache = CoreCache32::default();
        let old = cache.insert(compiled(&memory, 0x1000));
        let newer = cache.insert(compiled(&memory, 0x1005));
        assert!(old.entrypoints().contains(&GuestAddress(0x1005)));
        assert!(newer.entrypoints().contains(&GuestAddress(0x100d)));
        cache.remove(newer.identity());
        assert!(newer.is_current(&memory));
        assert!(cache.regions.contains_key(&old.identity()));
        assert!(!cache.regions.contains_key(&newer.identity()));
        let snapshot = cache.snapshot();
        let mut frontier = cache.frontier(&memory);
        frontier.sort_unstable_by_key(|pc| pc.0);
        assert_eq!(frontier, vec![GuestAddress(0x1000), GuestAddress(0x100a)]);
        assert_eq!(cache.snapshot(), snapshot);
        bounds(&cache);
    }

    #[test]
    fn frontier_membership_accepts_linked_and_unlinked_entries_without_touching_lru() {
        let memory = memory();
        let mut cache = CoreCache32::default();
        let first = cache.insert(compiled(&memory, 0x1000));
        let second = cache.insert(compiled(&memory, filler(0)));
        cache.install_link(&first, 1).unwrap();
        let snapshot = cache.snapshot();
        let links = cache.link_owners.to_vec();
        for pc in [0x1000, 0x1005, 0x100a, 0x100d, filler(0)] {
            assert!(cache.frontier_contains(&memory, pc));
        }
        assert!(!cache.frontier_contains(&memory, 0x1001));
        assert!(!cache.frontier_contains(&memory, 0xdead_0000));
        assert_eq!(cache.regions[&second.identity()].slot, 0);
        assert_eq!(cache.oldest, first.identity());
        assert_eq!(cache.newest, second.identity());
        assert_eq!(cache.oldest_linked_slot(), 1);
        assert_eq!(cache.snapshot(), snapshot);
        assert_eq!(cache.link_owners.as_ref(), links);
        bounds(&cache);
    }

    #[test]
    fn frontier_membership_rejects_foreign_stale_and_remapped_sources_without_removal() {
        for remap in [false, true] {
            let mut memory = memory();
            let mut cache = CoreCache32::default();
            let first = cache.insert(compiled(&memory, 0x1000));
            let second = cache.insert(compiled(&memory, filler(0)));
            cache.install_link(&first, 1).unwrap();
            let snapshot = cache.snapshot();
            assert!(cache.frontier_contains(&memory, 0x1000));
            assert!(!cache.frontier_contains(&AddressSpace::new(1).unwrap(), 0x1000));
            if remap {
                let mut page = [0; 4096];
                memory.read(GuestAddress(0x1000), &mut page).unwrap();
                let range = PageRange::new(GuestAddress(0x1000), 1).unwrap();
                memory.unmap(range).unwrap();
                memory.map_zeroed(range, Permissions::ALL).unwrap();
                memory.write(GuestAddress(0x1000), &page).unwrap();
            } else {
                memory.write(GuestAddress(0x1002), &[4]).unwrap();
            }
            assert!(!cache.frontier_contains(&memory, 0x1000));
            assert!(cache.frontier_contains(&memory, filler(0)));
            assert_eq!(cache.snapshot(), snapshot);
            assert_eq!(cache.regions.len(), 2);
            assert_eq!(cache.link_owners[1], first.identity());
            assert_eq!(cache.oldest, first.identity());
            assert_eq!(cache.newest, second.identity());
            bounds(&cache);
        }
    }

    #[test]
    fn frontier_membership_does_not_resurrect_a_removed_shadowing_owner() {
        let memory = memory();
        let mut cache = CoreCache32::default();
        let old = cache.insert(compiled(&memory, 0x1000));
        let newer = cache.insert(compiled(&memory, 0x1005));
        assert!(cache.frontier_contains(&memory, 0x1005));
        cache.remove(newer.identity());
        assert!(newer.is_current(&memory));
        assert!(cache.regions.contains_key(&old.identity()));
        assert!(!cache.frontier_contains(&memory, 0x1005));
        assert!(!cache.frontier_contains(&memory, 0x100d));
        assert!(cache.frontier_contains(&memory, 0x1000));
        assert!(cache.frontier_contains(&memory, 0x100a));
        bounds(&cache);
    }
}
