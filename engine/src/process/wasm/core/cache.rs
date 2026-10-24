use crate::cpu::core::continuation as route;
use crate::{
    cpu::core::{
        CompileError, CompileOptions, CompiledCode, ObservedPlan, ObservedPlanFingerprint,
        WindowFingerprint, WindowPlan,
    },
    memory::{AddressSpace, GuestAddress},
};
use foldhash::fast::RandomState;
use std::{
    collections::{HashMap, HashSet},
    mem::size_of,
    ops::Deref,
    rc::Rc,
};

#[path = "cache/context.rs"]
mod context;
#[path = "cache/continuation.rs"]
mod continuation;
#[path = "cache/executable.rs"]
mod executable;
#[path = "cache/policy.rs"]
mod policy;
use context::ContextBook;
pub(in crate::process) use context::PreferredKey;
use policy::{AutoPermit, PolicyBook};

// Initial directory extents, not independent residency limits.
const MAX_REGIONS: usize = 4096;
const MAX_CODE_BYTES: usize = 128 << 20;
const MAX_LINK_SLOT: usize = 4096;
const NO_REGION: u32 = u32::MAX;
const MAX_OBSERVED_REFUSALS: usize = 4;
const MAX_REGION_EXTENT: usize = crate::cpu::core::COLD_MARK as usize - 1;

pub(super) struct CoreCache32 {
    regions: Vec<Option<CachedRegion>>,
    free: Vec<usize>,
    entries: HashMap<u32, RegionKey, RandomState>,
    // Effective HashMap capacity can fall on tombstone removal. These bounds
    // describe the retained allocation and decrease only after a fresh rebuild.
    entries_capacity: usize,
    trace_entries: HashMap<u32, RegionKey, RandomState>,
    trace_entries_capacity: usize,
    executable_entries: HashMap<u32, RegionKey, RandomState>,
    executable_entries_capacity: usize,
    window_entries: HashMap<u32, RegionKey, RandomState>,
    window_entries_capacity: usize,
    trace_bypass_identity: u64,
    coverage: HashMap<u32, RegionKey, RandomState>,
    coverage_capacity: usize,
    entries_epoch: u64,
    ownership_epoch: u64,
    observed_refusals: Vec<ObservedRefusal>,
    window_refusals: Vec<WindowRefusal>,
    bytes: usize,
    owner_charge: usize,
    shared_charge: usize,
    policy: PolicyBook,
    contexts: ContextBook,
    protected_charge: usize,
    protected_count: usize,
    binding_epoch: u64,
    route_header: Box<route::Header>,
    route_owners: Vec<route::Owner>,
    route_rows: Vec<route::Row>,
    route_count: usize,
    link_owners: Vec<u64>,
    slot_regions: Vec<u32>,
    slot_generations: Vec<u64>,
    compiled: u64,
    admitted_encoded_bytes: u64,
    count_evictions: u64,
    byte_evictions: u64,
    source_invalidations: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct RegionKey {
    index: usize,
    identity: u64,
}

pub(super) struct ObservedAdmission {
    ownership_epoch: u64,
    anchor: u32,
    fingerprint: ObservedPlanFingerprint,
    parents: Vec<RegionKey>,
    stale: Vec<RegionKey>,
}

struct ObservedRefusal {
    ownership_epoch: u64,
    fingerprint: ObservedPlanFingerprint,
    charge: usize,
}

pub(super) struct WindowAdmission {
    ownership_epoch: u64,
    anchor: u32,
    ports: Vec<crate::cpu::core::HotSpan>,
    fingerprint: WindowFingerprint,
    replace: Vec<RegionKey>,
    stale: Vec<RegionKey>,
    permit: AutoPermit,
}

struct WindowRefusal {
    fingerprint: WindowFingerprint,
    charge: usize,
}

#[derive(Clone, Copy)]
enum DirectoryKind {
    Baseline,
    Family,
    Window,
    Fragment,
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
    slot: u32,
    linked_entries: LinkedEntries,
    rejected_entries: HashMap<u32, EntryRejection, RandomState>,
    rejected_capacity: usize,
    trace_heat: [TraceHeat; 4],
    charge: usize,
    automatic: bool,
    policy_baseline: u64,
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
        Self::visit_weight(sites, pc, 1)
    }

    fn visit_weight(sites: &mut [Self; 4], pc: u32, weight: u32) -> bool {
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
        site.visits = site.visits.saturating_add(weight);
        if site.visits < site.interval {
            return false;
        }
        site.visits -= site.interval;
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
    index: u32,
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
                index: row.index as usize,
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
            index: key.index as u32,
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
            entries_capacity: 0,
            trace_entries: HashMap::with_hasher(RandomState::default()),
            trace_entries_capacity: 0,
            executable_entries: HashMap::with_hasher(RandomState::default()),
            executable_entries_capacity: 0,
            window_entries: HashMap::with_hasher(RandomState::default()),
            window_entries_capacity: 0,
            trace_bypass_identity: 0,
            coverage: HashMap::with_hasher(RandomState::default()),
            coverage_capacity: 0,
            entries_epoch: 1,
            ownership_epoch: 1,
            observed_refusals: Vec::new(),
            window_refusals: Vec::new(),
            bytes: 0,
            owner_charge: 0,
            shared_charge: 0,
            policy: PolicyBook::default(),
            contexts: ContextBook::default(),
            protected_charge: 0,
            protected_count: 0,
            binding_epoch: 1,
            route_header: Box::default(),
            route_owners: vec![route::Owner::default(); MAX_REGIONS],
            route_rows: Vec::new(),
            route_count: 0,
            link_owners: vec![0; MAX_LINK_SLOT + 1],
            slot_regions: vec![NO_REGION; MAX_LINK_SLOT + 1],
            slot_generations: vec![0; MAX_LINK_SLOT + 1],
            compiled: 0,
            admitted_encoded_bytes: 0,
            count_evictions: 0,
            byte_evictions: 0,
            source_invalidations: 0,
        }
    }
}

impl CoreCache32 {
    fn hash_charge<K, V>(capacity: usize) -> usize {
        if capacity == 0 {
            return 0;
        }
        // capacity excludes the table's spare buckets and control bytes.
        capacity
            .saturating_mul((size_of::<(K, V)>() + 1).saturating_mul(2))
            .saturating_add(64)
    }

    fn reserved_capacity<K, V>(
        map: &HashMap<K, V, RandomState>,
        additional: usize,
        allocation_capacity: usize,
    ) -> usize {
        let required = map.len().saturating_add(additional);
        let reserved = if required <= map.capacity() {
            allocation_capacity
        } else {
            // reserve may choose a larger bucket array than the requested entries.
            required.max(4).next_power_of_two().saturating_mul(2)
        };
        reserved.max(allocation_capacity)
    }

    fn rebuild_map<K: Eq + std::hash::Hash, V>(map: &mut HashMap<K, V, RandomState>) -> usize {
        let mut rebuilt = HashMap::with_capacity_and_hasher(map.len(), RandomState::default());
        rebuilt.extend(map.drain());
        let capacity = rebuilt.capacity();
        *map = rebuilt;
        capacity
    }

    fn memo_charge(&self) -> usize {
        self.observed_refusals.capacity() * size_of::<ObservedRefusal>()
            + self
                .observed_refusals
                .iter()
                .map(|refusal| refusal.charge)
                .sum::<usize>()
            + self.window_refusals.capacity() * size_of::<WindowRefusal>()
            + self
                .window_refusals
                .iter()
                .map(|refusal| refusal.charge)
                .sum::<usize>()
    }

    fn directory_charge(&self) -> usize {
        size_of::<Self>()
            + self.shared_charge
            + self.regions.capacity() * size_of::<Option<CachedRegion>>()
            + self.free.capacity() * size_of::<usize>()
            + self.link_owners.capacity() * size_of::<u64>()
            + self.slot_regions.capacity() * size_of::<u32>()
            + size_of::<route::Header>()
            + self.route_owners.capacity() * size_of::<route::Owner>()
            + self.route_rows.capacity() * size_of::<route::Row>()
            + self.slot_generations.capacity() * size_of::<u64>()
            + Self::hash_charge::<u32, RegionKey>(self.entries_capacity)
            + Self::hash_charge::<u32, RegionKey>(self.trace_entries_capacity)
            + Self::hash_charge::<u32, RegionKey>(self.window_entries_capacity)
            + Self::hash_charge::<u32, RegionKey>(self.coverage_capacity)
            + Self::hash_charge::<u32, RegionKey>(self.executable_entries_capacity)
            + self.memo_charge()
            + self.policy.charge()
            + self.contexts.charge()
    }

    fn resident_charge(&self) -> usize {
        self.owner_charge.saturating_add(self.directory_charge())
    }

    pub(super) fn reserve_shared_resources(&mut self, charge: usize) -> Result<(), CompileError> {
        if charge == 0
            || self.route_header.active != 0
            || self.owner_charge != 0
            || self.shared_charge != 0
        {
            return Err(CompileError::WasmLimit);
        }
        if self.resident_charge().saturating_add(charge) > MAX_CODE_BYTES {
            return Err(CompileError::WasmLimit);
        }
        self.shared_charge = charge;
        Ok(())
    }

    fn code_charge(code: &CompiledCode, entries_capacity: usize) -> usize {
        code.resident_charge()
            .saturating_add(2 * size_of::<usize>()) // Rc allocation control words
            .saturating_add(entries_capacity.saturating_mul(size_of::<GuestAddress>()))
    }

    fn validate_candidate(
        &self,
        code: &CompiledCode,
        entries_capacity: usize,
    ) -> Result<(), CompileError> {
        let kind = if code.is_execution_window() {
            DirectoryKind::Window
        } else if code.trace_root().is_some() {
            DirectoryKind::Family
        } else {
            DirectoryKind::Baseline
        };
        self.validate_candidate_kind(code, entries_capacity, kind)
    }

    fn validate_candidate_kind(
        &self,
        code: &CompiledCode,
        entries_capacity: usize,
        kind: DirectoryKind,
    ) -> Result<(), CompileError> {
        self.validate_candidate_routes(code, entries_capacity, kind, 0)
    }

    fn validate_candidate_routes(
        &self,
        code: &CompiledCode,
        entries_capacity: usize,
        kind: DirectoryKind,
        additional_routes: usize,
    ) -> Result<(), CompileError> {
        let trace = code.trace_root().is_some();
        let entries = code.entrypoints().len();
        let table_capacity = |count: usize| {
            if count == 0 {
                0
            } else {
                count.max(4).next_power_of_two().saturating_mul(2)
            }
        };
        let minimum_directory = size_of::<Self>()
            + self.shared_charge
            + MAX_REGIONS
                * (size_of::<Option<CachedRegion>>()
                    + size_of::<usize>()
                    + size_of::<route::Owner>())
            + (MAX_LINK_SLOT + 1) * (size_of::<u64>() * 2 + size_of::<u32>())
            + size_of::<route::Header>()
            + self.policy.charge()
            + self.contexts.charge()
            + Self::route_capacity(
                code.hot_spans()
                    .count()
                    .saturating_add(usize::from(matches!(kind, DirectoryKind::Window)))
                    .saturating_add(additional_routes),
            ) * size_of::<route::Row>()
            + Self::hash_charge::<u32, RegionKey>(table_capacity(entries))
            + if trace && !matches!(kind, DirectoryKind::Fragment) {
                0
            } else {
                Self::hash_charge::<u32, RegionKey>(table_capacity(code.instruction_count()))
            };
        if Self::code_charge(code, entries_capacity).saturating_add(minimum_directory)
            > MAX_CODE_BYTES
        {
            return Err(CompileError::WasmLimit);
        }
        Ok(())
    }

    #[cfg(test)]
    fn projected_directory(&self, entries: usize, instructions: usize, trace: bool) -> usize {
        self.projected_directory_kind(
            entries,
            instructions,
            if trace {
                DirectoryKind::Family
            } else {
                DirectoryKind::Baseline
            },
        )
    }

    fn projected_directory_kind(
        &self,
        entries: usize,
        instructions: usize,
        kind: DirectoryKind,
    ) -> usize {
        self.projected_directory_routes(entries, instructions, kind, instructions)
    }

    fn projected_directory_routes(
        &self,
        entries: usize,
        instructions: usize,
        kind: DirectoryKind,
        routes: usize,
    ) -> usize {
        let extent = if self.free.is_empty() {
            self.regions.len().saturating_mul(2).min(MAX_REGION_EXTENT)
        } else {
            self.regions.len()
        };
        let (entry_capacity, trace_capacity, coverage_capacity, window_capacity) = match kind {
            DirectoryKind::Fragment => (
                self.entries_capacity,
                self.trace_entries_capacity,
                Self::reserved_capacity(&self.coverage, instructions, self.coverage_capacity),
                self.window_entries_capacity,
            ),
            DirectoryKind::Family => (
                self.entries_capacity,
                Self::reserved_capacity(&self.trace_entries, entries, self.trace_entries_capacity),
                self.coverage_capacity,
                self.window_entries_capacity,
            ),
            DirectoryKind::Window => (
                self.entries_capacity,
                self.trace_entries_capacity,
                self.coverage_capacity,
                Self::reserved_capacity(
                    &self.window_entries,
                    entries,
                    self.window_entries_capacity,
                ),
            ),
            DirectoryKind::Baseline => (
                Self::reserved_capacity(&self.entries, entries, self.entries_capacity),
                self.trace_entries_capacity,
                Self::reserved_capacity(&self.coverage, instructions, self.coverage_capacity),
                self.window_entries_capacity,
            ),
        };
        size_of::<Self>()
            + self.shared_charge
            + self.regions.capacity().max(extent) * size_of::<Option<CachedRegion>>()
            + self.free.capacity().max(extent) * size_of::<usize>()
            + self.link_owners.capacity().max(extent + 1) * size_of::<u64>()
            + self.slot_regions.capacity().max(extent + 1) * size_of::<u32>()
            + self.slot_generations.capacity().max(extent + 1) * size_of::<u64>()
            + size_of::<route::Header>()
            + self.route_owners.capacity().max(extent) * size_of::<route::Owner>()
            + self.route_rows.capacity().max(Self::route_capacity(
                self.route_count.saturating_add(routes),
            )) * size_of::<route::Row>()
            + Self::hash_charge::<u32, RegionKey>(entry_capacity)
            + Self::hash_charge::<u32, RegionKey>(trace_capacity)
            + Self::hash_charge::<u32, RegionKey>(coverage_capacity)
            + Self::hash_charge::<u32, RegionKey>(window_capacity)
            + Self::hash_charge::<u32, RegionKey>(if matches!(kind, DirectoryKind::Fragment) {
                Self::reserved_capacity(
                    &self.executable_entries,
                    entries,
                    self.executable_entries_capacity,
                )
            } else {
                self.executable_entries_capacity
            })
            + self.memo_charge()
            + self.policy.charge()
            + self.contexts.charge()
    }

    fn grow_directories(&mut self) {
        let old = self.regions.len();
        let extent = old.saturating_mul(2).min(MAX_REGION_EXTENT);
        assert!(extent > old);
        let mut regions = Vec::with_capacity(extent);
        regions.append(&mut self.regions);
        regions.resize_with(extent, || None);
        self.regions = regions;
        let mut free = Vec::with_capacity(extent);
        free.append(&mut self.free);
        free.extend((old..extent).rev());
        self.free = free;
        let mut owners = Vec::with_capacity(extent + 1);
        owners.append(&mut self.link_owners);
        owners.resize(extent + 1, 0);
        self.link_owners = owners;
        let mut slots = Vec::with_capacity(extent + 1);
        slots.append(&mut self.slot_regions);
        slots.resize(extent + 1, NO_REGION);
        self.slot_regions = slots;
        let mut generations = Vec::with_capacity(extent + 1);
        generations.append(&mut self.slot_generations);
        generations.resize(extent + 1, 0);
        self.slot_generations = generations;
        let mut route_owners = Vec::with_capacity(extent);
        route_owners.append(&mut self.route_owners);
        route_owners.resize(extent, route::Owner::default());
        self.route_owners = route_owners;
        self.advance_binding_epoch();
        self.refresh_route_pointers();
    }

    fn compact_directories(&mut self) {
        // Public capacity can fall when a removal leaves a tombstone. Only a
        // fresh allocation permits crediting a smaller table to the ledger.
        self.entries_capacity = Self::rebuild_map(&mut self.entries);
        self.trace_entries_capacity = Self::rebuild_map(&mut self.trace_entries);
        self.window_entries_capacity = Self::rebuild_map(&mut self.window_entries);
        self.coverage_capacity = Self::rebuild_map(&mut self.coverage);
        self.executable_entries_capacity = Self::rebuild_map(&mut self.executable_entries);
        self.rebuild_routes(Self::route_capacity(self.route_count));
        let used_region = self
            .regions
            .iter()
            .rposition(Option::is_some)
            .map_or(0, |i| i + 1);
        let used_slot = self
            .link_owners
            .iter()
            .rposition(|id| *id != 0)
            .unwrap_or(0);
        let extent = used_region
            .max(used_slot)
            .max(MAX_REGIONS)
            .next_power_of_two()
            .min(MAX_REGION_EXTENT);
        if extent < self.regions.len() {
            self.regions.truncate(extent);
            self.regions.shrink_to_fit();
            self.free = (0..extent)
                .rev()
                .filter(|i| self.regions[*i].is_none())
                .collect();
            self.free.reserve_exact(extent - self.free.len());
            self.link_owners.truncate(extent + 1);
            self.link_owners.shrink_to_fit();
            self.slot_regions.truncate(extent + 1);
            self.slot_regions.shrink_to_fit();
            self.slot_generations.truncate(extent + 1);
            self.slot_generations.shrink_to_fit();
            self.route_owners.truncate(extent);
            self.route_owners.shrink_to_fit();
            self.advance_binding_epoch();
        }
        self.refresh_route_pointers();
    }

    fn make_resident_space(
        &mut self,
        charge: usize,
        entries: usize,
        instructions: usize,
        trace: bool,
    ) {
        self.make_resident_space_kind(
            charge,
            entries,
            instructions,
            if trace {
                DirectoryKind::Family
            } else {
                DirectoryKind::Baseline
            },
        );
    }

    fn make_resident_space_kind(
        &mut self,
        charge: usize,
        entries: usize,
        instructions: usize,
        kind: DirectoryKind,
    ) {
        self.make_resident_space_routes(charge, entries, instructions, kind, instructions);
    }

    fn make_resident_space_routes(
        &mut self,
        charge: usize,
        entries: usize,
        instructions: usize,
        kind: DirectoryKind,
        routes: usize,
    ) {
        assert_eq!(
            self.route_header.active, 0,
            "executable admission is outside the CPU lease"
        );
        let mut compact_at = usize::MAX;
        let mut reviewed = false;
        while self
            .owner_charge
            .saturating_add(charge)
            .saturating_add(self.projected_directory_routes(entries, instructions, kind, routes))
            > MAX_CODE_BYTES
            || self.free.is_empty() && self.regions.len() == MAX_REGION_EXTENT
        {
            if !reviewed {
                self.review_protected();
                reviewed = true;
            }
            if self.window_refusals.capacity() != 0 {
                self.window_refusals = Vec::new();
                continue;
            }
            let count = self.regions.len() - self.free.len();
            if count <= compact_at {
                self.compact_directories();
                compact_at = count / 2;
                if self.owner_charge.saturating_add(charge).saturating_add(
                    self.projected_directory_routes(entries, instructions, kind, routes),
                ) <= MAX_CODE_BYTES
                    && (!self.free.is_empty() || self.regions.len() < MAX_REGION_EXTENT)
                {
                    break;
                }
            }
            let index = self
                .policy_victim()
                .expect("validated owner fits an empty resident cache");
            let identity = self.regions[index].as_ref().unwrap().code.identity();
            self.remove(RegionKey { index, identity });
            self.byte_evictions = self.byte_evictions.saturating_add(1);
        }
        if self.free.is_empty() {
            self.grow_directories();
        }
        self.reserve_routes(routes);
        match kind {
            DirectoryKind::Fragment => {
                self.executable_entries.reserve(entries);
                self.executable_entries_capacity = self
                    .executable_entries_capacity
                    .max(self.executable_entries.capacity());
                self.coverage.reserve(instructions);
                self.coverage_capacity = self.coverage_capacity.max(self.coverage.capacity());
            }
            DirectoryKind::Family => {
                self.trace_entries.reserve(entries);
                self.trace_entries_capacity = self
                    .trace_entries_capacity
                    .max(self.trace_entries.capacity());
            }
            DirectoryKind::Window => {
                self.window_entries.reserve(entries);
                self.window_entries_capacity = self
                    .window_entries_capacity
                    .max(self.window_entries.capacity());
            }
            DirectoryKind::Baseline => {
                self.entries.reserve(entries);
                self.entries_capacity = self.entries_capacity.max(self.entries.capacity());
                self.coverage.reserve(instructions);
                self.coverage_capacity = self.coverage_capacity.max(self.coverage.capacity());
            }
        }
        assert!(self.resident_charge().saturating_add(charge) <= MAX_CODE_BYTES);
    }

    fn remember_entry_rejection(&mut self, key: RegionKey, pc: u32, limits: EntryRejection) {
        let entry = self.region(key).unwrap();
        let before = Self::hash_charge::<u32, EntryRejection>(entry.rejected_capacity);
        let after = Self::hash_charge::<u32, EntryRejection>(Self::reserved_capacity(
            &entry.rejected_entries,
            usize::from(!entry.rejected_entries.contains_key(&pc)),
            entry.rejected_capacity,
        ));
        if self.resident_charge().saturating_add(after - before) > MAX_CODE_BYTES {
            return;
        }
        let entry = self.regions[key.index].as_mut().unwrap();
        entry
            .rejected_entries
            .reserve(usize::from(!entry.rejected_entries.contains_key(&pc)));
        entry.rejected_capacity = entry
            .rejected_capacity
            .max(entry.rejected_entries.capacity());
        entry.rejected_entries.insert(pc, limits);
        debug_assert!(entry.rejected_entries.capacity() <= entry.rejected_capacity);
        let extra = Self::hash_charge::<u32, EntryRejection>(entry.rejected_capacity) - before;
        entry.charge += extra;
        self.owner_charge += extra;
        if self.route_owners[key.index].flags & route::PROTECTED != 0 {
            self.protected_charge += extra;
        }
    }

    pub(super) fn owned_frontier_contains(&self, memory: &AddressSpace, pc: u32) -> bool {
        self.trace_entries
            .get(&pc)
            .or_else(|| self.coverage.get(&pc))
            .and_then(|key| self.region(*key))
            .is_some_and(|entry| entry.code.is_current(memory))
            || self.coverage_contains(memory, pc)
    }

    pub(super) fn preflight_observed(
        &mut self,
        memory: &AddressSpace,
        plan: &ObservedPlan,
    ) -> Option<ObservedAdmission> {
        if self.observed_refusals.iter().any(|refusal| {
            refusal.ownership_epoch == self.ownership_epoch
                && &refusal.fingerprint == plan.fingerprint()
        }) {
            return None;
        }
        let admission = self.check_observed_plan(memory, plan);
        if admission.is_none() {
            self.remember_observed_refusal(plan.fingerprint().clone());
        }
        admission
    }

    fn check_observed_plan(
        &self,
        memory: &AddressSpace,
        plan: &ObservedPlan,
    ) -> Option<ObservedAdmission> {
        let entries = plan.entrypoints();
        let identities = plan.parent_identities();
        if !plan.is_current(memory)
            || !plan.has_executable_gain()
            || entries.is_empty()
            || !entries.windows(2).all(|pair| pair[0].0 < pair[1].0)
            || !identities.windows(2).all(|pair| pair[0] < pair[1])
            || entries
                .binary_search_by_key(&plan.anchor(), |pc| pc.0)
                .is_err()
            || !entries.iter().map(|pc| pc.0).eq(plan.instruction_pcs())
        {
            return None;
        }
        let mut parents = HashSet::with_hasher(RandomState::default());
        let mut stale = HashSet::with_hasher(RandomState::default());
        for pc in entries {
            let Some(key) = self.trace_entries.get(&pc.0).copied() else {
                continue;
            };
            let entry = self.region(key)?;
            if !entry.code.is_current(memory) {
                stale.insert(key);
                continue;
            }
            if !entry.code.is_observed_family() || identities.binary_search(&key.identity).is_err()
            {
                return None;
            }
            parents.insert(key);
        }
        if parents.len() != identities.len() {
            return None;
        }
        for key in &parents {
            let entry = self.region(*key)?;
            if !plan.preserves(&entry.code) || !self.complete_family_aliases(*key, entry, entries) {
                return None;
            }
        }
        let mut parents: Vec<_> = parents.into_iter().collect();
        let mut stale: Vec<_> = stale.into_iter().collect();
        parents.sort_unstable_by_key(|key| key.identity);
        stale.sort_unstable_by_key(|key| key.identity);
        Some(ObservedAdmission {
            ownership_epoch: self.ownership_epoch,
            anchor: plan.anchor(),
            fingerprint: plan.fingerprint().clone(),
            parents,
            stale,
        })
    }

    fn complete_family_aliases(
        &self,
        key: RegionKey,
        entry: &CachedRegion,
        next: &[GuestAddress],
    ) -> bool {
        entry.admitted_entries == entry.code.entrypoints()
            && entry.indexed_entries == entry.code.instruction_count()
            && entry.admitted_entries.iter().all(|pc| {
                self.trace_entries.get(&pc.0) == Some(&key)
                    && next.binary_search_by_key(&pc.0, |entry| entry.0).is_ok()
            })
    }

    pub(super) fn commit_observed(
        &mut self,
        memory: &AddressSpace,
        admission: ObservedAdmission,
        code: CompiledCode,
    ) -> Result<RegionRef, CompileError> {
        if admission.ownership_epoch != self.ownership_epoch
            || code.observed_fingerprint() != Some(&admission.fingerprint)
            || !code.is_observed_family()
            || !code.is_current(memory)
            || code.trace_root() != Some(admission.anchor)
            || code.wasm().len() > MAX_CODE_BYTES
            || !code.supports_entry(admission.anchor)
            || !code
                .entrypoints()
                .iter()
                .map(|entry| entry.0)
                .eq(code.instruction_pcs())
        {
            return Err(CompileError::InvalidLimits);
        }
        for key in &admission.parents {
            let Some(entry) = self.region(*key) else {
                return Err(CompileError::InvalidLimits);
            };
            if !entry.code.is_observed_family()
                || !entry.code.is_current(memory)
                || !code.observed_grows(&entry.code)
                || !self.complete_family_aliases(*key, entry, code.entrypoints())
            {
                return Err(CompileError::InvalidLimits);
            }
        }
        let mut parents = HashSet::with_hasher(RandomState::default());
        let mut stale = HashSet::with_hasher(RandomState::default());
        for pc in code.instruction_pcs() {
            let Some(key) = self.trace_entries.get(&pc).copied() else {
                continue;
            };
            let Some(entry) = self.region(key) else {
                return Err(CompileError::InvalidLimits);
            };
            if entry.code.is_current(memory) {
                parents.insert(key);
            } else {
                stale.insert(key);
            }
        }
        if parents.len() != admission.parents.len()
            || stale.len() != admission.stale.len()
            || admission.parents.iter().any(|key| !parents.contains(key))
            || admission.stale.iter().any(|key| !stale.contains(key))
        {
            return Err(CompileError::InvalidLimits);
        }
        self.validate_candidate(&code, code.entrypoints().len())?;
        // source, identity, graph and every alias are validated before removal.
        for key in admission.stale {
            self.remove(key);
            self.source_invalidations = self.source_invalidations.saturating_add(1);
        }
        for key in admission.parents {
            self.remove(key);
        }
        self.insert_owned(code)
    }

    pub(super) fn reject_observed(&mut self, admission: ObservedAdmission) {
        if admission.ownership_epoch == self.ownership_epoch {
            self.remember_observed_refusal(admission.fingerprint);
        }
    }

    pub(super) fn preflight_window(
        &mut self,
        memory: &AddressSpace,
        plan: &WindowPlan,
    ) -> Option<WindowAdmission> {
        let ports = plan.hot_ports();
        if !plan.is_current(memory)
            || !plan.has_executable_gain()
            || plan.cross_owner_edge_count() == 0
            || ports.is_empty()
            || !ports.windows(2).all(|pair| pair[0].entry < pair[1].entry)
            || !ports.iter().any(|port| port.entry == plan.anchor())
            || !plan
                .entrypoints()
                .iter()
                .map(|entry| entry.0)
                .eq(ports.iter().map(|port| port.entry))
            || self
                .window_refusals
                .iter()
                .any(|r| r.fingerprint.same_graph(plan.fingerprint()))
            || self.contexts.windows.iter().any(|key| {
                self.region(*key).is_some_and(|entry| {
                    entry.code.is_current(memory)
                        && entry
                            .code
                            .window_fingerprint()
                            .is_some_and(|old| old.same_graph(plan.fingerprint()))
                })
            })
        {
            return None;
        }
        let (replace, stale) =
            self.window_replacements(memory, plan.fingerprint(), plan.instruction_pcs())?;
        let permit = self.reserve_window(plan)?;
        Some(WindowAdmission {
            ownership_epoch: self.ownership_epoch,
            anchor: plan.anchor(),
            ports,
            fingerprint: plan.fingerprint().clone(),
            replace,
            stale,
            permit,
        })
    }

    pub(super) fn commit_window(
        &mut self,
        memory: &AddressSpace,
        admission: WindowAdmission,
        code: CompiledCode,
    ) -> Result<RegionRef, CompileError> {
        let actual = Self::code_charge(&code, code.entrypoints().len());
        self.policy.finish(admission.permit, actual);
        if admission.ownership_epoch != self.ownership_epoch
            || !code.is_execution_window()
            || !code.is_current(memory)
            || code.trace_root() != Some(admission.anchor)
            || !code
                .entrypoints()
                .iter()
                .map(|entry| entry.0)
                .eq(admission.ports.iter().map(|port| port.entry))
            || !code.hot_spans().eq(admission.ports.iter().copied())
            || !code.supports_entry(admission.anchor)
            || code.window_fingerprint() != Some(&admission.fingerprint)
        {
            return Err(CompileError::InvalidLimits);
        }
        let Some((replace, stale)) =
            self.window_replacements(memory, &admission.fingerprint, code.instruction_pcs())
        else {
            return Err(CompileError::InvalidLimits);
        };
        if replace != admission.replace || stale != admission.stale {
            return Err(CompileError::InvalidLimits);
        }
        let migrate = self.contexts.keys_for_owners(&replace);
        if migrate
            .iter()
            .any(|key| !code.supports_entry(key.target) || code.hot_span(key.target).is_none())
        {
            return Err(CompileError::InvalidLimits);
        }
        let entries = code.entrypoints().to_vec();
        self.validate_candidate_routes(
            &code,
            entries.capacity(),
            DirectoryKind::Window,
            migrate.iter().filter(|key| key.origin.is_some()).count(),
        )?;
        for key in stale {
            self.remove(key);
            self.source_invalidations = self.source_invalidations.saturating_add(1);
        }
        for key in replace {
            self.remove(key);
        }
        let code = self.insert_with_kind_routes(
            code,
            entries,
            DirectoryKind::Window,
            migrate.iter().filter(|key| key.origin.is_some()).count(),
        )?;
        assert!(
            self.bind_window_preferences(memory, &code, &migrate),
            "validated context migration uses reserved routes"
        );
        Ok(code)
    }

    pub(super) fn reject_window(&mut self, admission: WindowAdmission) {
        self.policy.finish(admission.permit, 0);
        self.remember_window_refusal(admission.fingerprint);
    }

    fn remember_window_refusal(&mut self, fingerprint: WindowFingerprint) {
        if self
            .window_refusals
            .iter()
            .any(|refusal| refusal.fingerprint.same_graph(&fingerprint))
        {
            return;
        }
        if self.window_refusals.len() == MAX_OBSERVED_REFUSALS {
            self.window_refusals.remove(0);
        }
        let charge = fingerprint.resident_charge();
        self.window_refusals.push(WindowRefusal {
            fingerprint,
            charge,
        });
        if self.resident_charge() > MAX_CODE_BYTES {
            self.window_refusals = Vec::new();
        }
    }

    fn remember_observed_refusal(&mut self, fingerprint: ObservedPlanFingerprint) {
        self.observed_refusals
            .retain(|refusal| refusal.ownership_epoch == self.ownership_epoch);
        if self
            .observed_refusals
            .iter()
            .any(|refusal| refusal.fingerprint == fingerprint)
        {
            return;
        }
        if self.observed_refusals.len() == MAX_OBSERVED_REFUSALS {
            self.observed_refusals.remove(0);
        }
        let charge = fingerprint.resident_charge();
        self.observed_refusals.push(ObservedRefusal {
            ownership_epoch: self.ownership_epoch,
            fingerprint,
            charge,
        });
        if self.resident_charge() > MAX_CODE_BYTES {
            self.observed_refusals = Vec::new();
        }
    }

    #[cfg(test)]
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
        self.frontier_contains_with_bypass(memory, pc, self.trace_bypass_identity)
    }

    pub(in crate::process) fn precise_source_current(
        &self,
        memory: &AddressSpace,
        source: &RegionRef,
    ) -> bool {
        if !source.is_current(memory) {
            return false;
        }
        if source.is_transient() {
            return true;
        }
        self.retained(source)
            .is_some_and(|entry| entry.slot == 0 || self.reciprocal_slot(source.key, entry))
    }

    pub(in crate::process) fn precise_frontier_contains(
        &self,
        memory: &AddressSpace,
        pc: u32,
        remaining: u32,
        bypass: u64,
    ) -> bool {
        self.executable_frontier_contains(memory, pc, remaining, bypass)
    }

    pub(super) fn trace_bypass_identity(&self) -> u64 {
        self.trace_bypass_identity
    }

    pub(super) fn frontier_contains_with_bypass(
        &self,
        memory: &AddressSpace,
        pc: u32,
        bypass: u64,
    ) -> bool {
        self.trace_entries
            .get(&pc)
            .filter(|key| key.identity != bypass)
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
            self.remember_entry_rejection(owner.key, pc, limits);
            return Ok(OwnedEntryLookup::Declined);
        }
        let entry = self.region(owner.key).unwrap();
        let extra_entries =
            usize::from(entry.admitted_entries.len() == entry.admitted_entries.capacity())
                * size_of::<GuestAddress>();
        let extra_index = Self::hash_charge::<u32, RegionKey>(Self::reserved_capacity(
            &self.entries,
            1,
            self.entries_capacity,
        )) - Self::hash_charge::<u32, RegionKey>(self.entries_capacity);
        let route_capacity = self
            .route_rows
            .capacity()
            .max(Self::route_capacity(self.route_count + 1));
        let extra_routes = (route_capacity - self.route_rows.capacity()) * size_of::<route::Row>();
        if self
            .resident_charge()
            .saturating_add(extra_entries)
            .saturating_add(extra_index)
            .saturating_add(extra_routes)
            > MAX_CODE_BYTES
        {
            return Ok(OwnedEntryLookup::Declined);
        }
        self.reserve_routes(1);
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
        self.advance_ownership_epoch();
        self.advance_entries_epoch();
        self.entries.reserve(1);
        self.entries_capacity = self.entries_capacity.max(self.entries.capacity());
        self.entries.insert(pc, owner.key);
        debug_assert!(self.entries.capacity() <= self.entries_capacity);
        let entry = self.regions[owner.key.index].as_mut().unwrap();
        let before = entry.admitted_entries.capacity();
        if let Err(index) = entry
            .admitted_entries
            .binary_search_by_key(&pc, |entry| entry.0)
        {
            entry.admitted_entries.reserve_exact(1);
            entry.admitted_entries.insert(index, GuestAddress(pc));
        }
        let extra = (entry.admitted_entries.capacity() - before) * size_of::<GuestAddress>();
        entry.charge += extra;
        self.owner_charge += extra;
        if self.route_owners[owner.key.index].flags & route::PROTECTED != 0 {
            self.protected_charge += extra;
        }
        entry.indexed_entries += 1;
        entry.rejected_entries.remove(&pc);
        self.refresh_route_pc(pc);
        self.touch(owner.key.index);
        Ok(OwnedEntryLookup::Ready(owner))
    }

    pub(super) fn admit(
        &mut self,
        memory: &AddressSpace,
        code: CompiledCode,
    ) -> Result<RegionRef, CompileError> {
        self.validate_candidate(&code, code.entrypoints().len())?;
        if code.is_execution_window() {
            return Err(CompileError::InvalidLimits);
        }
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
            return self.insert_owned(code);
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
        self.insert_owned(code)
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
        self.insert_owned(code)
    }

    pub(super) fn link_owners_ptr(&self) -> *const u64 {
        self.link_owners.as_ptr()
    }

    pub(super) fn link_capacity(&self) -> usize {
        self.link_owners.len() - 1
    }

    pub(super) fn oldest_linked_slot(&self) -> u32 {
        for head in [self.route_header.oldest, self.route_header.protected_oldest] {
            let mut current = Self::route_index(head);
            let mut left = self.regions.len();
            while let Some(index) = current {
                if left == 0 {
                    break;
                }
                left -= 1;
                let Some(entry) = self.regions.get(index).and_then(Option::as_ref) else {
                    break;
                };
                let key = RegionKey {
                    index,
                    identity: entry.code.identity(),
                };
                if self.reciprocal_slot(key, entry) {
                    return entry.slot;
                }
                current = Self::route_index(self.route_owners[index].newer);
            }
        }
        0
    }

    pub(super) fn snapshot(&self) -> [u64; 12] {
        [
            self.compiled,
            self.admitted_encoded_bytes,
            self.count_evictions,
            self.byte_evictions,
            self.source_invalidations,
            (self.regions.len() - self.free.len()) as u64,
            self.bytes as u64,
            self.regions.len() as u64,
            self.resident_charge() as u64,
            MAX_CODE_BYTES as u64,
            self.link_capacity() as u64,
            self.binding_epoch,
        ]
    }

    pub(super) fn lookup(&mut self, memory: &AddressSpace, pc: u32) -> Option<RegionRef> {
        if let Some(key) = self
            .window_entries
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
        let key = self
            .entries
            .get(&pc)
            .copied()
            .filter(|key| key.identity != self.trace_bypass_identity)
            .or_else(|| {
                self.coverage.get(&pc).copied().filter(|key| {
                    key.identity != self.trace_bypass_identity
                        && self
                            .region(*key)
                            .is_some_and(|entry| entry.code.is_observed_family())
                })
            })
            .or_else(|| {
                self.executable_entries
                    .get(&pc)
                    .copied()
                    .filter(|key| key.identity != self.trace_bypass_identity)
            })
            .or_else(|| {
                // legacy source lookup may recover private cut geometry after an overlapping owner was evicted.
                self.regions.iter().enumerate().find_map(|(index, entry)| {
                    let entry = entry.as_ref()?;
                    (entry.code.identity() != self.trace_bypass_identity
                        && entry.code.is_observed_family()
                        && entry.code.contains_instruction(pc)
                        && entry.code.is_current(memory))
                    .then(|| RegionKey {
                        index,
                        identity: entry.code.identity(),
                    })
                })
            })?;
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
            .window_entries
            .get(&pc)
            .copied()
            .filter(|key| key.identity != self.trace_bypass_identity)
        {
            if let Some(linked) = self.lookup_linked_key(memory, key) {
                return Some(linked);
            }
        }
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
        let key = match self
            .entries
            .get(&pc)
            .copied()
            .filter(|key| key.identity != self.trace_bypass_identity)
        {
            Some(key) => key,
            None => self.lookup(memory, pc)?.key,
        };
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
        self.route_owners[key.index].validation = memory.code_validation_version();
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
                && self.slot_regions[slot as usize] as usize == code.key.index
            {
                Ok(())
            } else {
                Err(LinkError::Inconsistent)
            };
        }
        if owner != 0 || self.slot_regions[slot as usize] != NO_REGION {
            return Err(LinkError::Inconsistent);
        }
        let generation = self.slot_generations[slot as usize]
            .checked_add(1)
            .ok_or(LinkError::Inconsistent)?;
        self.regions[code.key.index].as_mut().unwrap().slot = slot;
        self.slot_generations[slot as usize] = generation;
        self.link_owners[slot as usize] = identity;
        self.slot_regions[slot as usize] = code.key.index as u32;
        self.route_owners[code.key.index].slot = slot;
        self.route_owners[code.key.index].generation = generation;
        self.route_owners[code.key.index].flags |= route::BOUND;
        self.advance_binding_epoch();
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
            index: self.slot_regions[slot as usize] as usize,
            identity: owner,
        };
        if !self.region(key).is_some_and(|entry| entry.slot == slot) {
            return Err(LinkError::Inconsistent);
        }
        let generation = self.slot_generations[slot as usize]
            .checked_add(1)
            .ok_or(LinkError::Inconsistent)?;
        self.regions[key.index].as_mut().unwrap().slot = 0;
        self.slot_generations[slot as usize] = generation;
        self.link_owners[slot as usize] = 0;
        self.slot_regions[slot as usize] = NO_REGION;
        self.route_owners[key.index].slot = 0;
        self.route_owners[key.index].generation = 0;
        self.route_owners[key.index].flags &= !route::BOUND;
        self.advance_binding_epoch();
        Ok(())
    }

    #[cfg(test)]
    pub(super) fn insert(&mut self, code: CompiledCode) -> RegionRef {
        // Legacy overlap controls exercise conditional index cleanup. Production
        // admission goes through the coverage checks or whole-owner replacement.
        self.insert_owned(code).unwrap()
    }

    fn insert_owned(&mut self, code: CompiledCode) -> Result<RegionRef, CompileError> {
        let entries = code.entrypoints().to_vec();
        self.insert_with_entries(code, entries)
    }

    fn insert_with_entries(
        &mut self,
        code: CompiledCode,
        entries: Vec<GuestAddress>,
    ) -> Result<RegionRef, CompileError> {
        let kind = if code.is_execution_window() {
            DirectoryKind::Window
        } else if code.trace_root().is_some() {
            DirectoryKind::Family
        } else {
            DirectoryKind::Baseline
        };
        self.insert_with_kind(code, entries, kind)
    }

    fn insert_with_kind(
        &mut self,
        code: CompiledCode,
        entries: Vec<GuestAddress>,
        kind: DirectoryKind,
    ) -> Result<RegionRef, CompileError> {
        self.insert_with_kind_routes(code, entries, kind, 0)
    }

    fn insert_with_kind_routes(
        &mut self,
        code: CompiledCode,
        entries: Vec<GuestAddress>,
        kind: DirectoryKind,
        additional_routes: usize,
    ) -> Result<RegionRef, CompileError> {
        self.validate_candidate_routes(&code, entries.capacity(), kind, additional_routes)?;
        self.advance_ownership_epoch();
        self.advance_entries_epoch();
        let trace = code.trace_root().is_some();
        let window = code.is_execution_window();
        let fragment = matches!(kind, DirectoryKind::Fragment);
        let code = Rc::new(code);
        for pc in entries.iter().filter(|_| !trace && !fragment) {
            if let Some(key) = self.entries.remove(&pc.0) {
                let entry = self.regions[key.index].as_mut().unwrap();
                entry.indexed_entries -= 1;
                if entry.indexed_entries == 0 {
                    self.remove(key);
                }
            }
        }
        let charge = Self::code_charge(&code, entries.capacity());
        self.make_resident_space_routes(
            charge,
            entries.len(),
            code.instruction_count(),
            kind,
            code.hot_spans()
                .count()
                .saturating_add(usize::from(window))
                .saturating_add(additional_routes),
        );
        let index = self.free.pop().unwrap();
        let identity = code.identity();
        let key = RegionKey { index, identity };
        for entry in &entries {
            if fragment {
                self.executable_entries.insert(entry.0, key);
            } else if window {
                self.window_entries.insert(entry.0, key);
            } else if trace {
                self.trace_entries.insert(entry.0, key);
            } else {
                self.entries.insert(entry.0, key);
            }
        }
        for pc in code.instruction_pcs().filter(|_| !trace || fragment) {
            if fragment {
                self.coverage.entry(pc).or_insert(key);
            } else {
                self.coverage.insert(pc, key);
            }
        }
        debug_assert!(self.entries.capacity() <= self.entries_capacity);
        debug_assert!(self.trace_entries.capacity() <= self.trace_entries_capacity);
        debug_assert!(self.window_entries.capacity() <= self.window_entries_capacity);
        debug_assert!(self.coverage.capacity() <= self.coverage_capacity);
        debug_assert!(self.executable_entries.capacity() <= self.executable_entries_capacity);
        self.bytes += code.wasm().len();
        self.owner_charge += charge;
        self.regions[index] = Some(CachedRegion {
            indexed_entries: entries.len(),
            admitted_entries: entries,
            code: Rc::clone(&code),
            slot: 0,
            linked_entries: LinkedEntries::default(),
            rejected_entries: HashMap::with_hasher(RandomState::default()),
            rejected_capacity: 0,
            trace_heat: [TraceHeat::default(); 4],
            charge,
            automatic: fragment || window,
            policy_baseline: 0,
        });
        if window {
            self.contexts.add_window(key);
        }
        let newest = self.route_header.newest;
        self.route_owners[index] = route::Owner {
            identity,
            generation: 0,
            validation: 0,
            slot: 0,
            older: newest,
            newer: 0,
            flags: if window { route::WINDOW } else { 0 },
            completed_work: 0,
        };
        if let Some(newest) = Self::route_index(newest) {
            self.route_owners[newest].newer = index as u32 + 1;
        } else {
            self.route_header.oldest = index as u32 + 1;
        }
        self.route_header.newest = index as u32 + 1;
        let pcs: Vec<_> = self.regions[index]
            .as_ref()
            .unwrap()
            .admitted_entries
            .iter()
            .map(|pc| pc.0)
            .collect();
        for pc in pcs {
            self.refresh_route_pc(pc);
        }
        self.refresh_route_pointers();
        self.compiled = self.compiled.saturating_add(1);
        self.admitted_encoded_bytes = self
            .admitted_encoded_bytes
            .saturating_add(code.wasm().len() as u64);
        Ok(RegionRef { key, code })
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
                .is_some_and(|index| *index as usize == key.index)
    }

    fn touch(&mut self, index: usize) {
        let protected = self.protected(index);
        if Self::route_index(self.queue_ends(protected).1) == Some(index) {
            return;
        }
        self.detach_queue(index);
        self.append_queue(index, protected);
    }

    fn remove(&mut self, key: RegionKey) {
        if self.region(key).is_none() {
            return;
        }
        self.advance_ownership_epoch();
        self.advance_entries_epoch();
        self.advance_binding_epoch();
        let entry = self.regions[key.index].take().unwrap();
        let row = self.route_owners[key.index];
        self.detach_queue(key.index);
        if row.flags & route::PROTECTED != 0 {
            self.protected_charge -= entry.charge;
            self.protected_count -= 1;
        }
        let context_keys = self.contexts.remove(key);
        self.route_owners[key.index] = route::Owner::default();
        self.bytes -= entry.code.wasm().len();
        self.owner_charge -= entry.charge;
        if self.reciprocal_slot(key, &entry) {
            self.slot_generations[entry.slot as usize] =
                self.slot_generations[entry.slot as usize].saturating_add(1);
            self.link_owners[entry.slot as usize] = 0;
            self.slot_regions[entry.slot as usize] = NO_REGION;
        }
        for pc in &entry.admitted_entries {
            if self.executable_entries.get(&pc.0) == Some(&key) {
                self.executable_entries.remove(&pc.0);
            }
            if self.window_entries.get(&pc.0) == Some(&key) {
                self.window_entries.remove(&pc.0);
            }
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
        for pc in &entry.admitted_entries {
            if self.window_entries.get(&pc.0).is_none() {
                if let Some(other) = self.contexts.windows.iter().copied().rev().find(|key| {
                    self.region(*key)
                        .is_some_and(|entry| entry.code.supports_entry(pc.0))
                }) {
                    self.window_entries.insert(pc.0, other);
                }
            }
            self.refresh_route_pc(pc.0);
        }
        for key in context_keys {
            self.refresh_route_context(key);
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

    fn advance_ownership_epoch(&mut self) {
        self.observed_refusals = Vec::new();
        self.ownership_epoch = match self.ownership_epoch.checked_add(1) {
            Some(epoch) => epoch,
            None => {
                self.observed_refusals.clear();
                1
            }
        };
    }

    fn advance_binding_epoch(&mut self) {
        // zero is sticky: an SDK that observed wrap must keep pruning.
        if self.binding_epoch != 0 {
            self.binding_epoch = self.binding_epoch.checked_add(1).unwrap_or(0);
        }
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
        if !self.can_capture_automatic()
            || self.retained(code).is_none()
            || !code.supports_entry(pc)
        {
            return false;
        }
        let entry = self.regions[code.key.index].as_mut().unwrap();
        TraceHeat::visit(&mut entry.trace_heat, pc)
    }

    pub(super) fn sample_candidate(&mut self, code: &RegionRef, pc: u32) -> bool {
        if !self.can_capture_automatic()
            || self.retained(code).is_none()
            || !code.supports_entry(pc)
        {
            return false;
        }
        let entry = self.regions[code.key.index].as_mut().unwrap();
        TraceHeat::visit_weight(&mut entry.trace_heat, pc, 1024)
    }
}

#[cfg(test)]
#[path = "cache/window_multientry_tests.rs"]
mod window_multientry_tests;

#[cfg(test)]
#[path = "cache/window_lifetime_tests.rs"]
mod window_lifetime_tests;

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

#[cfg(test)]
#[path = "cache/plan_tests.rs"]
mod plan_tests;

#[cfg(test)]
#[path = "cache/residency_tests.rs"]
mod residency_tests;

#[cfg(test)]
#[path = "cache/allocation_tests.rs"]
mod allocation_tests;

#[cfg(test)]
#[path = "cache/window_tests.rs"]
mod window_tests;

#[cfg(test)]
#[path = "cache/continuation_tests.rs"]
mod continuation_tests;

#[cfg(test)]
#[path = "cache/executable_tests.rs"]
mod executable_tests;
