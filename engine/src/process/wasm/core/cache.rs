use crate::{
    cpu::core::{CompileError, CompileOptions, CompiledCode},
    memory::{AddressSpace, GuestAddress},
};
use foldhash::fast::RandomState;
use std::{collections::HashMap, ops::Deref, rc::Rc};

const MAX_REGIONS: usize = 4096;
const MAX_CODE_BYTES: usize = 128 << 20;
const MAX_LINK_SLOT: usize = 4096;
const NO_REGION: u16 = u16::MAX;

pub(super) struct CoreCache32 {
    regions: Box<[Option<CachedRegion>]>,
    free: Vec<usize>,
    entries: HashMap<u32, RegionKey, RandomState>,
    trace_entries: HashMap<u32, RegionKey, RandomState>,
    trace_bypass_identity: u64,
    coverage: HashMap<u32, RegionKey, RandomState>,
    entries_epoch: u64,
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

impl RegionRef {
    pub(super) fn transient(code: CompiledCode) -> Self {
        let identity = code.identity();
        Self {
            key: RegionKey {
                index: usize::MAX,
                identity,
            },
            code: Rc::new(code),
        }
    }

    pub(super) fn is_transient(&self) -> bool {
        self.key.index == usize::MAX
    }
}

pub(super) enum TransitionLookup {
    Stop,
    Miss,
    Linked(RegionRef, u32),
}

pub(super) enum OwnedEntryLookup {
    Unowned,
    Ready(RegionRef),
    Declined,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct EntryRejection {
    instructions: usize,
    blocks: usize,
    wasm_bytes: usize,
}

impl From<CompileOptions> for EntryRejection {
    fn from(options: CompileOptions) -> Self {
        Self {
            instructions: options.instructions,
            blocks: options.blocks,
            wasm_bytes: options.wasm_bytes,
        }
    }
}

impl Deref for RegionRef {
    type Target = CompiledCode;

    fn deref(&self) -> &CompiledCode {
        &self.code
    }
}

struct CachedRegion {
    code: Rc<CompiledCode>,
    admitted_entries: Vec<GuestAddress>,
    indexed_entries: usize,
    older: Option<usize>,
    newer: Option<usize>,
    slot: u32,
    linked_entries: LinkedEntries,
    rejected_entries: HashMap<u32, EntryRejection, RandomState>,
    trace_heat: [TraceHeat; 4],
}

#[derive(Clone, Copy, Default)]
struct TraceHeat {
    pc: u32,
    visits: u32,
    interval: u32,
}

impl TraceHeat {
    const INITIAL: u32 = 1024;
    const MAX_INTERVAL: u32 = 1 << 20;

    fn visit(sites: &mut [Self; 4], pc: u32) -> bool {
        let index = sites
            .iter()
            .position(|site| site.interval != 0 && site.pc == pc)
            .or_else(|| sites.iter().position(|site| site.interval == 0))
            .unwrap_or_else(|| {
                sites
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, site)| site.visits)
                    .unwrap()
                    .0
            });
        let site = &mut sites[index];
        if site.interval == 0 || site.pc != pc {
            *site = Self {
                pc,
                visits: 0,
                interval: Self::INITIAL,
            };
        }
        site.visits += 1;
        if site.visits < site.interval {
            return false;
        }
        site.visits = 0;
        site.interval = (site.interval * 4).min(Self::MAX_INTERVAL);
        true
    }
}

#[derive(Default)]
struct LinkedEntries {
    epoch: u64,
    rows: [LinkedEntry; 4],
    next: usize,
}

#[derive(Clone, Copy, Default)]
struct LinkedEntry {
    identity: u64,
    pc: u32,
    index: u16,
}

impl LinkedEntries {
    fn get(&self, epoch: u64, pc: u32) -> Option<RegionKey> {
        if self.epoch != epoch {
            return None;
        }
        self.rows
            .iter()
            .find(|row| row.identity != 0 && row.pc == pc)
            .map(|row| RegionKey {
                index: usize::from(row.index),
                identity: row.identity,
            })
    }

    fn remember(&mut self, epoch: u64, pc: u32, key: RegionKey) {
        if self.epoch != epoch {
            self.rows.fill(LinkedEntry::default());
            self.epoch = epoch;
            self.next = 0;
        }
        self.rows[self.next] = LinkedEntry {
            identity: key.identity,
            pc,
            index: key.index as u16,
        };
        self.next = (self.next + 1) % self.rows.len();
    }
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
            entries: HashMap::with_hasher(RandomState::default()),
            trace_entries: HashMap::with_hasher(RandomState::default()),
            trace_bypass_identity: 0,
            coverage: HashMap::with_hasher(RandomState::default()),
            entries_epoch: 1,
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
        }
    }
}

impl CoreCache32 {
    pub(super) fn observed_owner(&mut self, memory: &AddressSpace, pc: u32) -> Option<RegionRef> {
        let key = *self.trace_entries.get(&pc)?;
        let entry = self.region(key)?;
        if !entry.code.is_observed_family() {
            return None;
        }
        if !entry.code.is_current(memory) {
            self.remove(key);
            self.source_invalidations = self.source_invalidations.saturating_add(1);
            return None;
        }
        let owner = RegionRef {
            key,
            code: Rc::clone(&entry.code),
        };
        self.touch(key.index);
        Some(owner)
    }

    pub(super) fn trace_score(
        &self,
        memory: &AddressSpace,
        root: u32,
    ) -> Option<crate::cpu::core::TraceScore> {
        self.trace_entries
            .get(&root)
            .and_then(|key| self.region(*key))
            .filter(|entry| entry.code.is_current(memory))
            .and_then(|entry| entry.code.trace_score())
    }

    pub(super) fn frontier_contains(&self, memory: &AddressSpace, pc: u32) -> bool {
        self.trace_entries
            .get(&pc)
            .filter(|key| key.identity != self.trace_bypass_identity)
            .and_then(|key| self.region(*key))
            .is_some_and(|entry| entry.code.is_current(memory))
            || self
                .entries
                .get(&pc)
                .and_then(|key| self.region(*key))
                .is_some_and(|entry| entry.code.is_current(memory))
    }

    #[cfg(test)]
    fn frontier(&self, memory: &AddressSpace) -> Vec<GuestAddress> {
        self.entries
            .iter()
            .filter_map(|(&pc, &key)| {
                self.region(key)
                    .filter(|entry| entry.code.is_current(memory))
                    .map(|_| GuestAddress(pc))
            })
            .collect()
    }

    pub(super) fn coverage_contains(&self, memory: &AddressSpace, pc: u32) -> bool {
        self.coverage
            .get(&pc)
            .and_then(|key| self.region(*key))
            .is_some_and(|entry| entry.code.is_current(memory))
    }

    pub(super) fn coverage_owner(&mut self, memory: &AddressSpace, pc: u32) -> Option<RegionRef> {
        let key = *self.coverage.get(&pc)?;
        let entry = self.region(key)?;
        if !entry.code.is_current(memory) {
            self.remove(key);
            self.source_invalidations = self.source_invalidations.saturating_add(1);
            return None;
        }
        Some(RegionRef {
            key,
            code: Rc::clone(&entry.code),
        })
    }

    pub(super) fn prepare_owned_entry(
        &mut self,
        memory: &AddressSpace,
        pc: u32,
        options: CompileOptions,
        _gates: &[(GuestAddress, u32)],
    ) -> Result<OwnedEntryLookup, CompileError> {
        if options.instructions == 0
            || options.instructions > 16384
            || options.blocks == 0
            || options.blocks > 1024
            || options.wasm_bytes < 128
            || options.wasm_bytes > 16 << 20
        {
            return Err(CompileError::InvalidLimits);
        }
        let Some(owner) = self.coverage_owner(memory, pc) else {
            return Ok(OwnedEntryLookup::Unowned);
        };
        if self.entries.get(&pc) == Some(&owner.key) {
            self.touch(owner.key.index);
            return Ok(OwnedEntryLookup::Ready(owner));
        }
        let limits = EntryRejection::from(options);
        if self.region(owner.key).unwrap().rejected_entries.get(&pc) == Some(&limits) {
            return Ok(OwnedEntryLookup::Declined);
        }
        if owner.instruction_count() > options.instructions
            || owner.wasm().len() > options.wasm_bytes
            || !owner.supports_entry(pc)
            || self.region(owner.key).unwrap().indexed_entries >= 1024
        {
            self.regions[owner.key.index]
                .as_mut()
                .unwrap()
                .rejected_entries
                .insert(pc, limits);
            return Ok(OwnedEntryLookup::Declined);
        }
        if let Some(previous) = self.entries.get(&pc).copied() {
            if self
                .region(previous)
                .is_some_and(|entry| entry.code.is_current(memory))
            {
                return Ok(OwnedEntryLookup::Declined);
            }
            self.remove(previous);
            self.source_invalidations = self.source_invalidations.saturating_add(1);
        }
        // The compiler already emitted this precise suffix port. Admitting it
        // changes executable ownership only; its body, identity and roots stay fixed.
        self.advance_entries_epoch();
        self.entries.insert(pc, owner.key);
        let entry = self.regions[owner.key.index].as_mut().unwrap();
        if let Err(index) = entry
            .admitted_entries
            .binary_search_by_key(&pc, |entry| entry.0)
        {
            entry.admitted_entries.insert(index, GuestAddress(pc));
        }
        entry.indexed_entries += 1;
        entry.rejected_entries.remove(&pc);
        self.touch(owner.key.index);
        Ok(OwnedEntryLookup::Ready(owner))
    }

    pub(super) fn admit(
        &mut self,
        memory: &AddressSpace,
        code: CompiledCode,
    ) -> Result<RegionRef, CompileError> {
        if code.is_observed_family() {
            return self.admit_observed_family(memory, code);
        }
        if let Some(root) = code.trace_root() {
            if code.wasm().len() > MAX_CODE_BYTES || code.entrypoints() != [GuestAddress(root)] {
                return Err(CompileError::InvalidLimits);
            }
            if let Some(previous) = self.trace_entries.get(&root).copied() {
                if let Some(entry) = self.region(previous) {
                    if entry.code.is_current(memory) {
                        if entry.code.is_observed_family() {
                            return Err(CompileError::InvalidLimits);
                        }
                        if !code
                            .trace_score()
                            .zip(entry.code.trace_score())
                            .is_some_and(|(next, current)| next.strictly_grows(current))
                        {
                            return Err(CompileError::InvalidLimits);
                        }
                    } else {
                        self.source_invalidations = self.source_invalidations.saturating_add(1);
                    }
                }
                // All fallible validation is complete. Retire only the prior
                // executor at this root, releasing its count/byte/slot budget
                // before infallible insertion; baseline coverage is untouched.
                self.remove(previous);
            }
            return Ok(self.insert_owned(code));
        }
        // Formation borrows the coverage directory as its frontier. A current
        // collision here indicates a violated ownership contract, never a suffix
        // that may silently replace only some entries of an existing owner.
        for pc in code.instruction_pcs() {
            if let Some(key) = self.coverage.get(&pc).copied() {
                if self
                    .region(key)
                    .is_some_and(|entry| entry.code.is_current(memory))
                {
                    return Err(CompileError::InvalidLimits);
                }
            }
        }
        for pc in code.instruction_pcs() {
            if let Some(key) = self.coverage.get(&pc).copied() {
                self.remove(key);
                self.source_invalidations = self.source_invalidations.saturating_add(1);
            }
        }
        Ok(self.insert_owned(code))
    }

    fn admit_observed_family(
        &mut self,
        memory: &AddressSpace,
        code: CompiledCode,
    ) -> Result<RegionRef, CompileError> {
        let Some(anchor) = code.trace_root() else {
            return Err(CompileError::InvalidLimits);
        };
        if !code.is_current(memory)
            || code.wasm().len() > MAX_CODE_BYTES
            || !code.supports_entry(anchor)
            || !code
                .entrypoints()
                .iter()
                .map(|entry| entry.0)
                .eq(code.instruction_pcs())
        {
            return Err(CompileError::InvalidLimits);
        }
        let mut stale = Vec::new();
        let mut current = None;
        for pc in code.instruction_pcs() {
            let Some(key) = self.trace_entries.get(&pc).copied() else {
                continue;
            };
            let Some(entry) = self.region(key) else {
                continue;
            };
            if !entry.code.is_current(memory) {
                if !stale.contains(&key) {
                    stale.push(key);
                }
                continue;
            }
            if current.is_some_and(|previous| previous != key)
                || !entry.code.is_observed_family()
                || entry.code.trace_root() != Some(anchor)
                || !code.observed_grows(&entry.code)
            {
                return Err(CompileError::InvalidLimits);
            }
            current = Some(key);
        }
        // all aliases are validated before retiring any retained identity.
        for key in stale {
            self.remove(key);
            self.source_invalidations = self.source_invalidations.saturating_add(1);
        }
        if let Some(key) = current {
            self.remove(key);
        }
        Ok(self.insert_owned(code))
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
        if let Some(key) = self
            .trace_entries
            .get(&pc)
            .copied()
            .filter(|key| key.identity != self.trace_bypass_identity)
        {
            if let Some(entry) = self.region(key) {
                if entry.code.is_current(memory) {
                    let code = RegionRef {
                        key,
                        code: Rc::clone(&entry.code),
                    };
                    self.touch(key.index);
                    return Some(code);
                }
            }
            self.remove(key);
            self.source_invalidations = self.source_invalidations.saturating_add(1);
        }
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
        if let Some(key) = self
            .trace_entries
            .get(&pc)
            .copied()
            .filter(|key| key.identity != self.trace_bypass_identity)
        {
            if let Some(linked) = self.lookup_linked_key(memory, key) {
                return Some(linked);
            }
        }
        let key = *self.entries.get(&pc)?;
        self.lookup_linked_key(memory, key)
    }

    pub(in crate::process) fn lookup_transition(
        &mut self,
        memory: &AddressSpace,
        source: &RegionRef,
        pc: u32,
    ) -> TransitionLookup {
        if !source.is_current(memory) {
            return TransitionLookup::Stop;
        }
        match self.lookup_linked_from(memory, source, pc) {
            Some((target, slot)) => TransitionLookup::Linked(target, slot),
            None => TransitionLookup::Miss,
        }
    }

    fn lookup_linked_from(
        &mut self,
        memory: &AddressSpace,
        source: &RegionRef,
        pc: u32,
    ) -> Option<(RegionRef, u32)> {
        let cached = self
            .retained(source)
            .and_then(|entry| entry.linked_entries.get(self.entries_epoch, pc));
        if let Some(key) = cached.filter(|key| key.identity != self.trace_bypass_identity) {
            if let Some(linked) = self.lookup_linked_key(memory, key) {
                return Some(linked);
            }
        }
        let linked = self.lookup_linked(memory, pc)?;
        if self.retained(source).is_some() {
            self.regions[source.key.index]
                .as_mut()
                .unwrap()
                .linked_entries
                .remember(self.entries_epoch, pc, linked.0.key);
        }
        Some(linked)
    }

    fn lookup_linked_key(
        &mut self,
        memory: &AddressSpace,
        key: RegionKey,
    ) -> Option<(RegionRef, u32)> {
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
        // An overlay can have been retained but unavailable while an SDK slot
        // was unlinked. Cached baseline edges must reconsider trace priority
        // when its executable table binding becomes available again.
        if code.trace_root().is_some() {
            self.advance_entries_epoch();
        }
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

    #[cfg(test)]
    pub(super) fn insert(&mut self, code: CompiledCode) -> RegionRef {
        // Legacy overlap controls exercise conditional index cleanup. Production
        // admission goes through the coverage checks or whole-owner replacement.
        self.insert_owned(code)
    }

    fn insert_owned(&mut self, code: CompiledCode) -> RegionRef {
        let entries = code.entrypoints().to_vec();
        self.insert_with_entries(code, entries)
    }

    fn insert_with_entries(&mut self, code: CompiledCode, entries: Vec<GuestAddress>) -> RegionRef {
        self.advance_entries_epoch();
        let trace = code.trace_root().is_some();
        let code = Rc::new(code);
        for pc in entries.iter().filter(|_| !trace) {
            if let Some(key) = self.entries.remove(&pc.0) {
                let entry = self.regions[key.index].as_mut().unwrap();
                entry.indexed_entries -= 1;
                if entry.indexed_entries == 0 {
                    self.remove(key);
                }
            }
        }
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
        for entry in &entries {
            if trace {
                self.trace_entries.insert(entry.0, key);
            } else {
                self.entries.insert(entry.0, key);
            }
        }
        for pc in code.instruction_pcs().filter(|_| !trace) {
            self.coverage.insert(pc, key);
        }
        self.bytes += code.wasm().len();
        self.regions[index] = Some(CachedRegion {
            indexed_entries: entries.len(),
            admitted_entries: entries,
            code: Rc::clone(&code),
            older: self.newest,
            newer: None,
            slot: 0,
            linked_entries: LinkedEntries::default(),
            rejected_entries: HashMap::with_hasher(RandomState::default()),
            trace_heat: [TraceHeat::default(); 4],
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
        self.advance_entries_epoch();
        let entry = self.regions[key.index].take().unwrap();
        self.detach(entry.older, entry.newer);
        self.bytes -= entry.code.wasm().len();
        if self.reciprocal_slot(key, &entry) {
            self.link_owners[entry.slot as usize] = 0;
            self.slot_regions[entry.slot as usize] = NO_REGION;
        }
        for pc in &entry.admitted_entries {
            if self.trace_entries.get(&pc.0) == Some(&key) {
                self.trace_entries.remove(&pc.0);
            }
            if self
                .entries
                .get(&pc.0)
                .is_some_and(|indexed| *indexed == key)
            {
                self.entries.remove(&pc.0);
            }
        }
        for pc in entry.code.instruction_pcs() {
            if self.coverage.get(&pc) == Some(&key) {
                self.coverage.remove(&pc);
            }
        }
        self.free.push(key.index);
    }

    fn advance_entries_epoch(&mut self) {
        self.entries_epoch = match self.entries_epoch.checked_add(1) {
            Some(epoch) => epoch,
            None => {
                for entry in self.regions.iter_mut().flatten() {
                    entry.linked_entries.epoch = 0;
                }
                1
            }
        };
    }

    pub(super) fn bypass_trace(&mut self, identity: u64) {
        if self.trace_bypass_identity != identity {
            self.trace_bypass_identity = identity;
            self.advance_entries_epoch();
        }
    }

    // Heat belongs to bounded retained owners, not an immortal root history.
    // Distinct hot ports and already observed owners can form a later capture.
    // Failed/equal captures back off without permanently consuming the site.
    pub(super) fn trace_candidate(&mut self, code: &RegionRef, pc: u32) -> bool {
        if self.retained(code).is_none() || !code.supports_entry(pc) {
            return false;
        }
        let entry = self.regions[code.key.index].as_mut().unwrap();
        TraceHeat::visit(&mut entry.trace_heat, pc)
    }
}

#[cfg(test)]
#[path = "cache/tests.rs"]
mod tests;

#[cfg(test)]
#[path = "cache/ownership_tests.rs"]
mod ownership_tests;

#[cfg(test)]
#[path = "cache/graph_tests.rs"]
mod graph_tests;

#[cfg(test)]
#[path = "cache/trace_tests.rs"]
mod trace_tests;

#[cfg(test)]
#[path = "cache/family_tests.rs"]
mod family_tests;
