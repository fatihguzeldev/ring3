use crate::{
    cpu::core::CompiledCode,
    memory::{AddressSpace, GuestAddress},
};
use std::{collections::HashMap, ops::Deref, rc::Rc};

#[path = "promotion.rs"]
mod promotion;
use promotion::{PendingTransition, TransitionHeat};

pub(super) const TRANSITION_HITS: u32 = 4096;

const MAX_REGIONS: usize = 4096;
const MAX_CODE_BYTES: usize = 128 << 20;
const MAX_LINK_SLOT: usize = 4096;
const NO_REGION: u16 = u16::MAX;

pub(super) struct CoreCache32 {
    regions: Box<[Option<CachedRegion>]>,
    free: Vec<usize>,
    entries: HashMap<u32, RegionKey>,
    bytes: usize,
    oldest: Option<usize>,
    newest: Option<usize>,
    link_owners: Box<[u64]>,
    slot_regions: Box<[u16]>,
    compiled: u64,
    admitted_encoded_bytes: u64,
    count_evictions: u64,
    byte_evictions: u64,
    source_invalidations: u64,
    pending_transition: Option<PendingTransition>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RegionKey {
    index: usize,
    identity: u64,
}

#[derive(Clone)]
pub(super) struct RegionRef {
    key: RegionKey,
    code: Rc<CompiledCode>,
}

pub(super) enum TransitionLookup {
    Stop,
    Miss,
    Linked(RegionRef, u32),
}

impl Deref for RegionRef {
    type Target = CompiledCode;

    fn deref(&self) -> &CompiledCode {
        &self.code
    }
}

struct CachedRegion {
    code: Rc<CompiledCode>,
    older: Option<usize>,
    newer: Option<usize>,
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
            regions: (0..MAX_REGIONS).map(|_| None).collect(),
            free: (0..MAX_REGIONS).rev().collect(),
            entries: HashMap::new(),
            bytes: 0,
            oldest: None,
            newest: None,
            link_owners: vec![0; MAX_LINK_SLOT + 1].into_boxed_slice(),
            slot_regions: vec![NO_REGION; MAX_LINK_SLOT + 1].into_boxed_slice(),
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
        self.entries
            .get(&pc)
            .and_then(|key| self.region(*key))
            .is_some_and(|entry| entry.code.is_current(memory))
    }

    pub(super) fn frontier(&self, memory: &AddressSpace) -> Vec<GuestAddress> {
        let mut frontier = Vec::new();
        for (index, entry) in self.regions.iter().enumerate() {
            let Some(entry) = entry else { continue };
            if !entry.code.is_current(memory) {
                continue;
            }
            let key = RegionKey {
                index,
                identity: entry.code.identity(),
            };
            for pc in entry.code.entrypoints() {
                if self
                    .entries
                    .get(&pc.0)
                    .is_some_and(|indexed| *indexed == key)
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
        let mut current = self.oldest;
        while let Some(index) = current {
            let entry = self.regions[index].as_ref().unwrap();
            let key = RegionKey {
                index,
                identity: entry.code.identity(),
            };
            if self.reciprocal_slot(key, entry) {
                return entry.slot;
            }
            current = entry.newer;
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
            (MAX_REGIONS - self.free.len()) as u64,
            self.bytes as u64,
            MAX_REGIONS as u64,
        ]
    }

    pub(super) fn lookup(&mut self, memory: &AddressSpace, pc: u32) -> Option<RegionRef> {
        let key = *self.entries.get(&pc)?;
        let entry = self.region(key)?;
        if !entry.code.is_current(memory) {
            self.remove(key);
            self.source_invalidations = self.source_invalidations.saturating_add(1);
            return None;
        }
        let code = RegionRef {
            key,
            code: Rc::clone(&entry.code),
        };
        self.touch(key.index);
        Some(code)
    }

    pub(super) fn lookup_linked(
        &mut self,
        memory: &AddressSpace,
        pc: u32,
    ) -> Option<(RegionRef, u32)> {
        let key = *self.entries.get(&pc)?;
        let entry = self.region(key)?;
        if !entry.code.is_current(memory) {
            self.remove(key);
            self.source_invalidations = self.source_invalidations.saturating_add(1);
            return None;
        }
        let linked = self.reciprocal_slot(key, entry).then(|| {
            (
                RegionRef {
                    key,
                    code: Rc::clone(&entry.code),
                },
                entry.slot,
            )
        });
        self.touch(key.index);
        linked
    }

    pub(super) fn install_link(&mut self, code: &RegionRef, slot: u32) -> Result<(), LinkError> {
        let owner = *self
            .link_owners
            .get(slot as usize)
            .filter(|_| slot != 0)
            .ok_or(LinkError::InvalidSlot)?;
        let identity = code.identity();
        let retained = self.retained(code).ok_or(LinkError::Unretained)?;
        if retained.slot != 0 && retained.slot != slot {
            return Err(LinkError::AlreadyLinked);
        }
        if owner != 0 && owner != identity {
            return Err(LinkError::Occupied);
        }
        if retained.slot == slot {
            return if owner == identity
                && usize::from(self.slot_regions[slot as usize]) == code.key.index
            {
                Ok(())
            } else {
                Err(LinkError::Inconsistent)
            };
        }
        if owner != 0 || self.slot_regions[slot as usize] != NO_REGION {
            return Err(LinkError::Inconsistent);
        }
        self.regions[code.key.index].as_mut().unwrap().slot = slot;
        self.link_owners[slot as usize] = identity;
        self.slot_regions[slot as usize] = code.key.index as u16;
        Ok(())
    }

    pub(super) fn unlink(&mut self, slot: u32, expected: u64) -> Result<(), LinkError> {
        let owner = *self
            .link_owners
            .get(slot as usize)
            .filter(|_| slot != 0)
            .ok_or(LinkError::InvalidSlot)?;
        if owner == 0 {
            return if self.slot_regions[slot as usize] == NO_REGION {
                Ok(())
            } else {
                Err(LinkError::Inconsistent)
            };
        }
        if owner != expected {
            return Err(LinkError::WrongOwner);
        }
        let key = RegionKey {
            index: usize::from(self.slot_regions[slot as usize]),
            identity: owner,
        };
        if !self.region(key).is_some_and(|entry| entry.slot == slot) {
            return Err(LinkError::Inconsistent);
        }
        self.regions[key.index].as_mut().unwrap().slot = 0;
        self.link_owners[slot as usize] = 0;
        self.slot_regions[slot as usize] = NO_REGION;
        Ok(())
    }

    pub(super) fn insert(&mut self, code: CompiledCode) -> RegionRef {
        let code = Rc::new(code);
        while self.free.is_empty() || self.bytes + code.wasm().len() > MAX_CODE_BYTES {
            let Some(index) = self.oldest else { break };
            let byte_pressure = self.bytes + code.wasm().len() > MAX_CODE_BYTES;
            let identity = self.regions[index].as_ref().unwrap().code.identity();
            self.remove(RegionKey { index, identity });
            if byte_pressure {
                self.byte_evictions = self.byte_evictions.saturating_add(1);
            } else {
                self.count_evictions = self.count_evictions.saturating_add(1);
            }
        }
        let index = self.free.pop().unwrap();
        let identity = code.identity();
        let key = RegionKey { index, identity };
        for entry in code.entrypoints() {
            self.entries.insert(entry.0, key);
        }
        self.bytes += code.wasm().len();
        self.regions[index] = Some(CachedRegion {
            code: Rc::clone(&code),
            older: self.newest,
            newer: None,
            slot: 0,
            heat: TransitionHeat::default(),
        });
        if let Some(newest) = self.newest {
            self.regions[newest].as_mut().unwrap().newer = Some(index);
        } else {
            self.oldest = Some(index);
        }
        self.newest = Some(index);
        self.compiled = self.compiled.saturating_add(1);
        self.admitted_encoded_bytes = self
            .admitted_encoded_bytes
            .saturating_add(code.wasm().len() as u64);
        RegionRef { key, code }
    }

    fn region(&self, key: RegionKey) -> Option<&CachedRegion> {
        self.regions
            .get(key.index)?
            .as_ref()
            .filter(|entry| entry.code.identity() == key.identity)
    }

    fn retained(&self, code: &RegionRef) -> Option<&CachedRegion> {
        self.region(code.key)
            .filter(|entry| Rc::ptr_eq(&entry.code, &code.code))
    }

    fn reciprocal_slot(&self, key: RegionKey, entry: &CachedRegion) -> bool {
        let slot = entry.slot as usize;
        slot != 0
            && self.link_owners.get(slot) == Some(&key.identity)
            && self
                .slot_regions
                .get(slot)
                .is_some_and(|index| usize::from(*index) == key.index)
    }

    fn touch(&mut self, index: usize) {
        if self.newest == Some(index) {
            return;
        }
        let entry = self.regions[index].as_ref().unwrap();
        let (older, newer) = (entry.older, entry.newer);
        self.detach(older, newer);
        let newest = self.newest.unwrap();
        self.regions[newest].as_mut().unwrap().newer = Some(index);
        let entry = self.regions[index].as_mut().unwrap();
        entry.older = self.newest;
        entry.newer = None;
        self.newest = Some(index);
    }

    fn detach(&mut self, older: Option<usize>, newer: Option<usize>) {
        if let Some(older) = older {
            self.regions[older].as_mut().unwrap().newer = newer;
        } else {
            self.oldest = newer;
        }
        if let Some(newer) = newer {
            self.regions[newer].as_mut().unwrap().older = older;
        } else {
            self.newest = older;
        }
    }

    fn remove(&mut self, key: RegionKey) {
        if self.region(key).is_none() {
            return;
        }
        let entry = self.regions[key.index].take().unwrap();
        self.detach(entry.older, entry.newer);
        self.bytes -= entry.code.wasm().len();
        if self.reciprocal_slot(key, &entry) {
            self.link_owners[entry.slot as usize] = 0;
            self.slot_regions[entry.slot as usize] = NO_REGION;
        }
        for pc in entry.code.entrypoints() {
            if self
                .entries
                .get(&pc.0)
                .is_some_and(|indexed| *indexed == key)
            {
                self.entries.remove(&pc.0);
            }
        }
        self.free.push(key.index);
    }
}

#[cfg(test)]
#[path = "cache/tests.rs"]
mod tests;
