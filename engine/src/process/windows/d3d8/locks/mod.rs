mod mapping;
mod publication;
mod staging;

use super::{DispatchError32, memory::read_bytes};
use crate::{
    graphics::{ResourceId, d3d8::resources::LockInfo},
    memory::{AddressSpace, PageRange},
};

pub(super) struct GuestLock32 {
    pub resource: ResourceId,
    pub info: LockInfo,
    pub range: PageRange,
    pub mapping: u64,
    pub data_offset: u32,
}

pub(super) struct LockRegistry32 {
    range: PageRange,
    memory_identity: Option<u64>,
    entries: Vec<GuestLock32>,
    limit: usize,
    staging_reuse: bool,
    staging: Option<staging::StagingMapping32>,
    staging_reuses: u64,
}

pub(super) struct PreparedLockDisposal32 {
    pub ranges: Vec<PageRange>,
    pub tokens: Vec<(ResourceId, u64)>,
    pub staging: bool,
}

impl LockRegistry32 {
    pub(super) fn terminate_process(&mut self) {
        self.entries.clear();
        self.staging = None;
    }

    pub(super) fn prepare_disposal(
        &self,
        memory: &AddressSpace,
        resources: &[ResourceId],
    ) -> Result<PreparedLockDisposal32, DispatchError32> {
        let count = self
            .entries
            .iter()
            .filter(|entry| resources.contains(&entry.resource))
            .count();
        let mut ranges = Vec::new();
        let mut tokens = Vec::new();
        ranges.try_reserve_exact(count).map_err(|_| {
            DispatchError32::Api(crate::graphics::d3d8::api::HResult::OUT_OF_MEMORY)
        })?;
        tokens.try_reserve_exact(count).map_err(|_| {
            DispatchError32::Api(crate::graphics::d3d8::api::HResult::OUT_OF_MEMORY)
        })?;
        for entry in self
            .entries
            .iter()
            .filter(|entry| resources.contains(&entry.resource))
        {
            self.validate_mapping(memory, entry)?;
            ranges.push(entry.range);
            tokens.push((entry.resource, entry.info.token));
        }
        let staging = self
            .staging
            .as_ref()
            .is_some_and(|staging| resources.contains(&staging.resource));
        if staging
            && let Some(cached) = &self.staging
            && !self
                .entries
                .iter()
                .any(|entry| entry.range == cached.range())
        {
            cached.append_owned_ranges(memory, &mut ranges)?;
        }
        ranges.sort_unstable_by_key(|range| range.first);
        ranges.dedup();
        tokens.sort_unstable_by_key(|&(resource, token)| lock_identity(resource, token));
        Ok(PreparedLockDisposal32 {
            ranges,
            tokens,
            staging,
        })
    }

    pub(super) fn commit_disposal(&mut self, prepared: &PreparedLockDisposal32) {
        if prepared.staging {
            self.staging = None;
        }
        self.entries.retain(|entry| {
            prepared
                .tokens
                .binary_search_by_key(
                    &lock_identity(entry.resource, entry.info.token),
                    |&(resource, token)| lock_identity(resource, token),
                )
                .is_err()
        });
    }

    pub(super) fn new(range: PageRange, limit: u32) -> Result<Self, ()> {
        if limit == 0 || range.first == 0 {
            return Err(());
        }
        let mut entries = Vec::new();
        entries.try_reserve_exact(limit as usize).map_err(|_| ())?;
        Ok(Self {
            range,
            memory_identity: None,
            entries,
            limit: limit as usize,
            staging_reuse: false,
            staging: None,
            staging_reuses: 0,
        })
    }

    pub(super) fn contains_resource(&self, resource: ResourceId) -> bool {
        self.entries.iter().any(|entry| entry.resource == resource)
    }

    fn staging_bytes(
        &self,
        memory: &AddressSpace,
        index: usize,
    ) -> Result<Vec<u8>, DispatchError32> {
        let entry = &self.entries[index];
        self.validate_mapping(memory, entry)?;
        read_bytes(
            memory,
            (entry.range.first * crate::memory::PAGE_SIZE as usize) as u32 + entry.data_offset,
            entry.info.length as usize,
        )
        .map_err(Into::into)
    }
}

fn lock_identity(resource: ResourceId, token: u64) -> (u64, u32, u32, u64) {
    (resource.epoch, resource.slot, resource.generation, token)
}

#[cfg(test)]
mod tests;
