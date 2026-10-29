use super::{DispatchError32, LockRegistry32};
use crate::{
    graphics::{ResourceId, d3d8::api::HResult},
    memory::{AddressSpace, GuestAddress, PAGE_SIZE, PageRange},
};

const MAX_STAGING_PAGES: usize = 64;

pub(super) struct StagingMapping32 {
    pub(super) resource: ResourceId,
    range: PageRange,
    incarnation: u64,
    slots: [u32; MAX_STAGING_PAGES],
}

impl StagingMapping32 {
    pub(super) fn range(&self) -> PageRange {
        self.range
    }
    fn valid(&self, memory: &AddressSpace) -> bool {
        (0..self.range.count).all(|index| {
            memory.staging_page_owner(GuestAddress(
                ((self.range.first + index) * PAGE_SIZE as usize) as u32,
            )) == Some((self.slots[index], self.incarnation))
        })
    }

    pub(super) fn append_owned_ranges(
        &self,
        memory: &AddressSpace,
        ranges: &mut Vec<PageRange>,
    ) -> Result<(), DispatchError32> {
        ranges
            .try_reserve(self.range.count)
            .map_err(|_| HResult::OUT_OF_MEMORY)?;
        for index in 0..self.range.count {
            let page = self.range.first + index;
            if memory.staging_page_matches(
                GuestAddress((page * PAGE_SIZE as usize) as u32),
                self.slots[index],
                self.incarnation,
            ) {
                ranges.push(PageRange {
                    first: page,
                    count: 1,
                });
            }
        }
        Ok(())
    }
}

impl LockRegistry32 {
    pub(in crate::process) fn append_staging_exit_ranges(
        &self,
        memory: &AddressSpace,
        ranges: &mut Vec<PageRange>,
    ) -> Result<(), DispatchError32> {
        if let Some(staging) = &self.staging {
            staging.append_owned_ranges(memory, ranges)?;
        }
        Ok(())
    }

    pub(in crate::process::windows::d3d8) fn active_locks(&self) -> bool {
        !self.entries.is_empty()
    }

    pub(in crate::process::windows::d3d8) fn staging_status(&self) -> [u32; 4] {
        [
            u32::from(self.staging_reuse),
            self.staging
                .as_ref()
                .map_or(0, |staging| staging.range.count as u32 * PAGE_SIZE),
            self.staging_reuses as u32,
            (self.staging_reuses >> 32) as u32,
        ]
    }

    pub(in crate::process::windows::d3d8) fn configure_staging_reuse(
        &mut self,
        memory: &mut AddressSpace,
        enabled: bool,
    ) -> Result<(), DispatchError32> {
        if self.active_locks() {
            return Err(HResult::INVALID_CALL.into());
        }
        if !enabled {
            self.release_staging(memory)?;
        }
        self.staging_reuse = enabled;
        Ok(())
    }

    pub(super) fn release_staging(
        &mut self,
        memory: &mut AddressSpace,
    ) -> Result<(), DispatchError32> {
        if let Some(staging) = &self.staging {
            let mut ranges = Vec::new();
            staging.append_owned_ranges(memory, &mut ranges)?;
            if !ranges.is_empty() {
                memory.unmap_ranges_with_word_writes(&ranges, &[])?;
            }
            self.staging = None;
        }
        Ok(())
    }

    pub(super) fn reusable_staging(
        &mut self,
        memory: &mut AddressSpace,
        bytes: usize,
    ) -> Result<Option<PageRange>, DispatchError32> {
        if let Some(staging) = &self.staging {
            if staging.range.count * PAGE_SIZE as usize >= bytes && staging.valid(memory) {
                return Ok(Some(staging.range));
            }
            self.release_staging(memory)?;
        }
        Ok(None)
    }

    pub(super) fn cache_candidate(&self, bytes: usize, volume: Option<bool>) -> bool {
        self.staging_reuse
            && self.entries.is_empty()
            && volume.is_none()
            && bytes > 0
            && bytes <= MAX_STAGING_PAGES * PAGE_SIZE as usize
            && self.staging_reuses != u64::MAX
    }

    pub(super) fn remember_staging(
        &mut self,
        memory: &AddressSpace,
        resource: ResourceId,
        range: PageRange,
    ) {
        let mut slots = [0; MAX_STAGING_PAGES];
        let mut incarnation = 0;
        for index in 0..range.count {
            let Some((slot, version)) = memory.staging_page_owner(GuestAddress(
                ((range.first + index) * PAGE_SIZE as usize) as u32,
            )) else {
                return;
            };
            if index != 0 && incarnation != version {
                return;
            }
            slots[index] = slot;
            incarnation = version;
        }
        self.staging = Some(StagingMapping32 {
            resource,
            range,
            incarnation,
            slots,
        });
    }

    pub(super) fn reused_staging(&mut self, resource: ResourceId) {
        self.staging.as_mut().unwrap().resource = resource;
        self.staging_reuses += 1;
    }

    pub(super) fn retain_staging(&mut self, memory: &AddressSpace, range: PageRange) -> bool {
        let retained = self.staging_reuse
            && self
                .staging
                .as_ref()
                .is_some_and(|staging| staging.range == range && staging.valid(memory));
        if !retained
            && self
                .staging
                .as_ref()
                .is_some_and(|staging| staging.range == range)
        {
            self.staging = None;
        }
        retained
    }
}
