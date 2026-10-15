use std::sync::Arc;

use crate::{
    loader::{ModuleError32, ModuleId32, ModuleSet32},
    memory::{AddressSpace, PageRange, WordWrite32},
};

#[derive(Debug)]
pub struct UnloadPlan32 {
    memory_identity: u64,
    memory_version: u64,
    set_revision: u64,
    set_identity: Arc<()>,
    ids: Vec<ModuleId32>,
    ranges: Vec<PageRange>,
    removed_edges: Vec<(ModuleId32, ModuleId32)>,
}

impl ModuleSet32 {
    pub fn plan_unload(
        &self,
        memory: &AddressSpace,
        ids: &[ModuleId32],
    ) -> Result<UnloadPlan32, ModuleError32> {
        self.plan_unload_rollback(memory, ids, &[])
    }

    pub fn plan_unload_rollback(
        &self,
        memory: &AddressSpace,
        ids: &[ModuleId32],
        edges: &[(ModuleId32, ModuleId32)],
    ) -> Result<UnloadPlan32, ModuleError32> {
        if ids.is_empty() && edges.is_empty() {
            return Err(ModuleError32::InvalidRequest);
        }
        if self.modules.iter().any(|module| {
            !ids.contains(&module.id)
                && module
                    .dependencies
                    .iter()
                    .any(|target| ids.contains(target) && !edges.contains(&(module.id, *target)))
        }) {
            return Err(ModuleError32::ModuleInUse);
        }
        let mut owned_ids = Vec::new();
        owned_ids
            .try_reserve_exact(ids.len())
            .map_err(|_| ModuleError32::Allocation)?;
        let mut ranges = Vec::new();
        let mut removed_edges = Vec::new();
        removed_edges
            .try_reserve_exact(edges.len())
            .map_err(|_| ModuleError32::Allocation)?;
        for &(importer, target) in edges {
            let module = self.get(importer).ok_or(ModuleError32::NotFound)?;
            if !module.dependencies.contains(&target)
                || self.get(target).is_none()
                || removed_edges.contains(&(importer, target))
            {
                return Err(ModuleError32::InvalidRequest);
            }
            removed_edges.push((importer, target));
        }
        for id in ids {
            if owned_ids.contains(id) {
                return Err(ModuleError32::InvalidRequest);
            }
            let module = self.get(*id).ok_or(ModuleError32::NotFound)?;
            ranges
                .try_reserve(module.ranges.len())
                .map_err(|_| ModuleError32::Allocation)?;
            ranges.extend_from_slice(&module.ranges);
            owned_ids.push(*id);
        }
        Ok(UnloadPlan32 {
            memory_identity: memory.identity(),
            memory_version: memory.transaction_version(),
            set_revision: self.revision,
            set_identity: Arc::clone(&self.identity),
            ids: owned_ids,
            ranges,
            removed_edges,
        })
    }
}

impl UnloadPlan32 {
    pub fn add_range(&mut self, range: PageRange) -> Result<(), ModuleError32> {
        if self.ranges.iter().any(|current| {
            current.first < range.first + range.count && range.first < current.first + current.count
        }) {
            return Err(ModuleError32::AddressConflict);
        }
        self.ranges
            .try_reserve(1)
            .map_err(|_| ModuleError32::Allocation)?;
        self.ranges.push(range);
        Ok(())
    }

    pub fn ids(&self) -> &[ModuleId32] {
        &self.ids
    }

    pub fn unmap(
        self,
        memory: &mut AddressSpace,
        set: &mut ModuleSet32,
    ) -> Result<(), ModuleError32> {
        self.unmap_with_writes(memory, set, &[])
    }

    pub fn unmap_with_writes(
        mut self,
        memory: &mut AddressSpace,
        set: &mut ModuleSet32,
        writes: &[WordWrite32],
    ) -> Result<(), ModuleError32> {
        if !Arc::ptr_eq(&set.identity, &self.set_identity)
            || memory.identity() != self.memory_identity
            || memory.transaction_version() != self.memory_version
            || set.revision != self.set_revision
        {
            return Err(ModuleError32::StalePlan);
        }
        let revision = set.revision.checked_add(1).ok_or(ModuleError32::Capacity)?;
        self.ranges.sort_unstable_by_key(|range| range.first);
        if !self.ranges.is_empty() {
            memory.unmap_ranges_with_word_writes(&self.ranges, writes)?;
        } else if !writes.is_empty() {
            memory.write_words32(writes)?;
        }
        for module in &mut set.modules {
            module
                .dependencies
                .retain(|target| !self.removed_edges.contains(&(module.id, *target)));
        }
        set.modules.retain(|module| !self.ids.contains(&module.id));
        if set.root.is_some_and(|root| self.ids.contains(&root)) {
            set.root = None;
        }
        set.revision = revision;
        Ok(())
    }
}
