//! preferences choose among capabilities; they are never capabilities themselves.
use super::*;

pub(in crate::process) const MAX_PREFERENCES: usize = 4096;
const WAYS: usize = 4;
const CONTEXT_BYTES: usize = 2 << 20;
const MAX_WINDOW_OVERLAP: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(in crate::process) struct PreferredKey {
    pub origin: Option<u32>,
    pub target: u32,
}
#[derive(Clone, Copy)]
struct Preference {
    key: PreferredKey,
    owner: RegionKey,
    stamp: u64,
}

pub(super) struct ContextBook {
    rows: Box<[Option<Preference>]>,
    pub(super) windows: Vec<RegionKey>,
    stamp: u64,
}
impl Default for ContextBook {
    fn default() -> Self {
        Self {
            rows: (0..MAX_PREFERENCES).map(|_| None).collect(),
            windows: Vec::new(),
            stamp: 0,
        }
    }
}
impl ContextBook {
    // reserve the bounded private context index in the shared
    // residency budget, rather than allocating outside a full executable cache.
    pub(super) fn charge(&self) -> usize {
        CONTEXT_BYTES
    }
    fn bucket(key: PreferredKey) -> usize {
        (route::context_hash(key.target, key.origin) as usize & (MAX_PREFERENCES / WAYS - 1)) * WAYS
    }
    pub(super) fn get(&self, key: PreferredKey) -> Option<RegionKey> {
        let base = Self::bucket(key);
        self.rows[base..base + WAYS]
            .iter()
            .flatten()
            .find(|r| r.key == key)
            .map(|r| r.owner)
    }
    pub(super) fn keys(&self) -> impl Iterator<Item = PreferredKey> + '_ {
        self.rows.iter().flatten().map(|r| r.key)
    }
    pub(super) fn can_add_window_after(&self, removed: usize) -> bool {
        let count = self.windows.len().saturating_sub(removed).saturating_add(1);
        let capacity = self
            .windows
            .capacity()
            .max(count.max(4).next_power_of_two());
        self.rows.len() * size_of::<Option<Preference>>() + capacity * size_of::<RegionKey>()
            <= CONTEXT_BYTES
    }
    pub(super) fn add_window(&mut self, key: RegionKey) {
        if self.windows.len() == self.windows.capacity() {
            let target = self
                .windows
                .len()
                .saturating_add(1)
                .max(4)
                .next_power_of_two();
            self.windows.reserve_exact(target - self.windows.len());
        }
        self.windows.push(key);
    }
    fn eviction_for(&self, key: PreferredKey) -> Option<PreferredKey> {
        let base = Self::bucket(key);
        if self.rows[base..base + WAYS]
            .iter()
            .any(|row| row.is_none() || row.is_some_and(|r| r.key == key))
        {
            return None;
        }
        (base..base + WAYS)
            .min_by_key(|&i| self.rows[i].unwrap().stamp)
            .map(|i| self.rows[i].unwrap().key)
    }
    fn bind(&mut self, key: PreferredKey, owner: RegionKey) -> Option<PreferredKey> {
        self.stamp = self.stamp.saturating_add(1);
        let base = Self::bucket(key);
        let index = (base..base + WAYS)
            .find(|&i| self.rows[i].is_some_and(|r| r.key == key))
            .or_else(|| (base..base + WAYS).find(|&i| self.rows[i].is_none()))
            .unwrap_or_else(|| {
                (base..base + WAYS)
                    .min_by_key(|&i| self.rows[i].unwrap().stamp)
                    .unwrap()
            });
        let old = self.rows[index].map(|r| r.key).filter(|old| *old != key);
        self.rows[index] = Some(Preference {
            key,
            owner,
            stamp: self.stamp,
        });
        old
    }
    pub(super) fn keys_for_owners(&self, owners: &[RegionKey]) -> Vec<PreferredKey> {
        self.rows
            .iter()
            .flatten()
            .filter(|r| owners.contains(&r.owner))
            .map(|r| r.key)
            .collect()
    }
    pub(super) fn remove(&mut self, owner: RegionKey) -> Vec<PreferredKey> {
        let mut keys = Vec::new();
        for row in self.rows.iter_mut() {
            if row.is_some_and(|r| r.owner == owner) {
                keys.push(row.take().unwrap().key);
            }
        }
        self.windows.retain(|key| *key != owner);
        keys
    }
}

impl CoreCache32 {
    fn preference_resources(&self, keys: &[PreferredKey]) -> Option<usize> {
        let new = keys
            .iter()
            .filter(|key| self.route_position_key(**key).is_err())
            .count();
        let mut removed: Vec<_> = keys
            .iter()
            .filter_map(|key| self.contexts.eviction_for(*key))
            .filter(|key| key.origin.is_some() && self.route_position_key(*key).is_ok())
            .collect();
        removed.sort_unstable();
        removed.dedup();
        let rows = self
            .route_count
            .saturating_add(new)
            .saturating_sub(removed.len());
        let capacity = self.route_rows.capacity().max(Self::route_capacity(rows));
        let extra = capacity.saturating_sub(self.route_rows.capacity()) * size_of::<route::Row>();
        (self.resident_charge().saturating_add(extra) <= MAX_CODE_BYTES).then_some(rows)
    }
    fn current_preference(
        &self,
        memory: &AddressSpace,
        key: PreferredKey,
        bypass: u64,
    ) -> Option<RegionRef> {
        let owner = self.contexts.get(key)?;
        if owner.identity == bypass {
            return None;
        }
        let entry = self.region(owner)?;
        if !entry.code.is_current(memory)
            || entry.slot != 0 && !self.reciprocal_slot(owner, entry)
            || !entry.code.supports_entry(key.target)
            || entry.code.hot_span(key.target).is_none()
        {
            return None;
        }
        Some(RegionRef {
            key: owner,
            code: entry.code.clone(),
        })
    }
    pub(in crate::process) fn bind_window_preferences(
        &mut self,
        memory: &AddressSpace,
        code: &RegionRef,
        keys: &[PreferredKey],
    ) -> bool {
        if self.route_header.active != 0
            || self.retained(code).is_none()
            || !code.is_current(memory)
            || !code.is_execution_window()
            || keys
                .iter()
                .any(|key| !code.supports_entry(key.target) || code.hot_span(key.target).is_none())
        {
            return false;
        }
        if keys.len() > MAX_PREFERENCES {
            return false;
        }
        let mut keys = keys.to_vec();
        keys.sort_unstable();
        keys.dedup();
        if self.preference_resources(&keys).is_none() {
            return false;
        }
        for key in &keys {
            if let Some(old) = self.contexts.bind(*key, code.key) {
                self.refresh_route_context(old);
            }
        }
        let new = keys
            .iter()
            .filter(|key| self.route_position_key(**key).is_err())
            .count();
        self.reserve_routes(new);
        for key in keys {
            self.refresh_route_context(key);
        }
        self.advance_entries_epoch();
        true
    }
    pub(in crate::process) fn can_select_window_candidate(
        &self,
        memory: &AddressSpace,
        plan: &WindowPlan,
    ) -> bool {
        self.can_select_window_candidate_for(
            memory,
            plan,
            &[PreferredKey {
                origin: None,
                target: plan.anchor(),
            }],
        )
    }

    pub(in crate::process) fn can_select_window_candidate_for(
        &self,
        memory: &AddressSpace,
        plan: &WindowPlan,
        bindings: &[PreferredKey],
    ) -> bool {
        let ports = plan.fingerprint().hot_ports();
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
        {
            return false;
        }
        if self.contexts.windows.iter().any(|key| {
            self.region(*key).is_some_and(|entry| {
                entry.code.is_current(memory)
                    && entry
                        .code
                        .window_fingerprint()
                        .is_some_and(|old| old.same_graph(plan.fingerprint()))
            })
        }) {
            if bindings
                .iter()
                .any(|key| !ports.iter().any(|port| port.entry == key.target))
            {
                return false;
            }
            return self.preference_resources(bindings).is_some();
        }
        !self
            .window_refusals
            .iter()
            .any(|r| r.fingerprint.same_graph(plan.fingerprint()))
            && self
                .window_replacements(memory, plan.fingerprint(), plan.instruction_pcs())
                .is_some()
            && self.policy.can_reserve(
                &policy::SourceKey::Window(plan.fingerprint().clone()),
                plan.instruction_pcs().count().saturating_mul(32),
            )
    }

    pub(in crate::process) fn reuse_window(
        &mut self,
        memory: &AddressSpace,
        plan: &WindowPlan,
        keys: &[PreferredKey],
    ) -> Option<RegionRef> {
        if !plan.is_current(memory) || !plan.has_executable_gain() {
            return None;
        }
        let owner = self.contexts.windows.iter().copied().find(|key| {
            self.region(*key).is_some_and(|entry| {
                entry.code.is_current(memory)
                    && entry
                        .code
                        .window_fingerprint()
                        .is_some_and(|old| old.same_graph(plan.fingerprint()))
            })
        })?;
        let entry = self.region(owner)?;
        let code = RegionRef {
            key: owner,
            code: entry.code.clone(),
        };
        self.bind_window_preferences(memory, &code, keys)
            .then_some(code)
    }
    fn clean_stale_preference(&mut self, memory: &AddressSpace, key: PreferredKey) {
        if let Some(owner) = self.contexts.get(key).filter(|owner| {
            self.region(*owner)
                .is_some_and(|entry| !entry.code.is_current(memory))
        }) {
            self.remove(owner);
            self.source_invalidations = self.source_invalidations.saturating_add(1);
        }
    }
    pub(in crate::process) fn lookup_executable_context(
        &mut self,
        memory: &AddressSpace,
        origin: Option<u32>,
        pc: u32,
    ) -> Option<RegionRef> {
        self.clean_stale_preference(memory, PreferredKey { origin, target: pc });
        if let Some(code) = self.current_preference(
            memory,
            PreferredKey { origin, target: pc },
            self.trace_bypass_identity,
        ) {
            self.touch(code.key.index);
            return Some(code);
        }
        self.lookup_executable(memory, pc)
    }
    pub(in crate::process) fn lookup_executable_linked_context(
        &mut self,
        memory: &AddressSpace,
        origin: Option<u32>,
        pc: u32,
        remaining: u32,
    ) -> Option<(RegionRef, u32)> {
        if let Some(code) = self.current_preference(
            memory,
            PreferredKey { origin, target: pc },
            self.trace_bypass_identity,
        ) {
            if code.hot_span(pc)?.instructions > remaining {
                return None;
            }
            return self.lookup_linked_key(memory, code.key);
        }
        self.lookup_executable_linked(memory, pc, remaining)
    }
    pub(super) fn window_replacements(
        &self,
        memory: &AddressSpace,
        fingerprint: &WindowFingerprint,
        pcs: impl Iterator<Item = u32>,
    ) -> Option<(Vec<RegionKey>, Vec<RegionKey>)> {
        let mut replace = Vec::new();
        let mut stale = Vec::new();
        let mut current = Vec::new();
        for key in self.contexts.windows.iter().copied() {
            let Some(entry) = self.region(key) else {
                continue;
            };
            let old = entry.code.window_fingerprint().unwrap();
            if !entry.code.is_current(memory) {
                if fingerprint.overlaps(old) {
                    stale.push(key);
                }
            } else if old.same_graph(fingerprint) {
                return None;
            } else if fingerprint.strictly_covers(old) {
                replace.push(key);
            } else {
                current.push(key);
            }
        }
        if !self
            .contexts
            .can_add_window_after(replace.len() + stale.len())
        {
            return None;
        }
        for pc in pcs {
            if current
                .iter()
                .filter(|key| self.region(**key).unwrap().code.contains_instruction(pc))
                .count()
                >= MAX_WINDOW_OVERLAP
            {
                return None;
            }
        }
        replace.sort_unstable_by_key(|key| key.identity);
        stale.sort_unstable_by_key(|key| key.identity);
        Some((replace, stale))
    }
}
