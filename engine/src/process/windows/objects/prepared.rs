use super::{GuestComError32, GuestComObjects32};
use crate::{
    memory::{AddressSpace, GuestAddress, MAX_WORD_WRITES32, MemoryError, PageRange, WordWrite32},
    windows::com::{ComIdentity32, OwnedReferenceChanges32},
};

pub(crate) struct PreparedGuestLifetime32 {
    references: OwnedReferenceChanges32,
    memory_identity: u64,
    memory_version: u64,
    ranges: Vec<PageRange>,
    writes: [WordWrite32; MAX_WORD_WRITES32],
    write_count: usize,
}

#[cfg(test)]
mod tests;

impl PreparedGuestLifetime32 {
    pub(crate) fn destroyed(&self) -> &[ComIdentity32] {
        self.references.destroyed()
    }
}

impl GuestComObjects32 {
    pub(crate) fn prepare_lifetime_changes(
        &self,
        memory: &AddressSpace,
        retain: &[GuestAddress],
        release: &[GuestAddress],
        hold: &[GuestAddress],
        unhold: &[GuestAddress],
        writes: &[WordWrite32],
    ) -> Result<PreparedGuestLifetime32, GuestComError32> {
        self.prepare_lifetime_changes_with_unmaps(
            memory,
            retain,
            release,
            hold,
            unhold,
            writes,
            &[],
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn prepare_lifetime_changes_with_unmaps(
        &self,
        memory: &AddressSpace,
        retain: &[GuestAddress],
        release: &[GuestAddress],
        hold: &[GuestAddress],
        unhold: &[GuestAddress],
        writes: &[WordWrite32],
        extra_ranges: &[PageRange],
    ) -> Result<PreparedGuestLifetime32, GuestComError32> {
        let references = self.prepare_reference_receipt(memory, retain, release, hold, unhold)?;
        self.prepare_lifetime_receipt(memory, references, writes, extra_ranges)
    }

    pub(crate) fn prepare_reference_receipt(
        &self,
        memory: &AddressSpace,
        retain: &[GuestAddress],
        release: &[GuestAddress],
        hold: &[GuestAddress],
        unhold: &[GuestAddress],
    ) -> Result<OwnedReferenceChanges32, GuestComError32> {
        for &pointer in retain.iter().chain(release).chain(hold).chain(unhold) {
            self.preflight(memory, pointer)?;
        }
        Ok(self
            .registry
            .prepare_owned_reference_changes(retain, release, hold, unhold)?)
    }

    pub(crate) fn prepare_lifetime_receipt(
        &self,
        memory: &AddressSpace,
        references: OwnedReferenceChanges32,
        writes: &[WordWrite32],
        extra_ranges: &[PageRange],
    ) -> Result<PreparedGuestLifetime32, GuestComError32> {
        self.registry
            .can_commit_owned_reference_changes(&references)?;
        for transition in references.transitions() {
            self.preflight(memory, transition.identity.identity_pointer)?;
        }
        if writes.len() > MAX_WORD_WRITES32 {
            return Err(MemoryError::InvalidRange.into());
        }
        let mut ranges = Vec::new();
        ranges
            .try_reserve_exact(
                references
                    .destroyed()
                    .len()
                    .checked_add(extra_ranges.len())
                    .ok_or(GuestComError32::Capacity)?,
            )
            .map_err(|_| GuestComError32::Allocation)?;
        for identity in references.destroyed() {
            let index = self
                .allocations
                .binary_search_by_key(&identity.id, |allocation| allocation.id)
                .map_err(|_| GuestComError32::StaleMapping)?;
            ranges.push(self.allocations[index].range);
        }
        for range in extra_ranges {
            let end = range
                .first
                .checked_add(range.count)
                .filter(|end| *end <= 1 << 20)
                .ok_or(MemoryError::InvalidRange)?;
            if range.count == 0 {
                return Err(MemoryError::InvalidRange.into());
            }
            if self.vtables.iter().any(|table| {
                let page = table.address.0 as usize / crate::memory::PAGE_SIZE as usize;
                range.first <= page && page < end
            }) {
                return Err(MemoryError::InvalidRange.into());
            }
            for allocation in &self.allocations {
                if range.first < allocation.range.first + allocation.range.count
                    && allocation.range.first < end
                    && !references
                        .destroyed()
                        .iter()
                        .any(|identity| identity.id == allocation.id)
                {
                    return Err(MemoryError::InvalidRange.into());
                }
            }
            for page in range.first..end {
                memory
                    .mapping_version_at(GuestAddress((page as u32) * crate::memory::PAGE_SIZE))?;
            }
            ranges.push(*range);
        }
        ranges.sort_unstable_by_key(|range| range.first);
        ranges.dedup();
        if ranges
            .windows(2)
            .any(|pair| pair[0].first + pair[0].count > pair[1].first)
        {
            return Err(MemoryError::InvalidRange.into());
        }

        let mut copied = [WordWrite32 {
            address: GuestAddress(0),
            value: 0,
        }; MAX_WORD_WRITES32];
        copied[..writes.len()].copy_from_slice(writes);
        copied[..writes.len()].sort_unstable_by_key(|word| word.address.0);
        for (index, word) in copied[..writes.len()].iter().enumerate() {
            if index != 0 && u64::from(copied[index - 1].address.0) + 4 > u64::from(word.address.0)
            {
                return Err(MemoryError::InvalidRange.into());
            }
            memory.check_span(word.address, 4, crate::memory::Access::Write)?;
            let first = word.address.0 as usize / crate::memory::PAGE_SIZE as usize;
            let last = (u64::from(word.address.0) + 3) as usize / crate::memory::PAGE_SIZE as usize;
            if ranges
                .iter()
                .any(|range| range.indices().contains(&first) || range.indices().contains(&last))
            {
                return Err(MemoryError::InvalidRange.into());
            }
        }
        if (!ranges.is_empty() || !writes.is_empty()) && memory.transaction_version() == u64::MAX {
            return Err(MemoryError::VersionExhausted.into());
        }
        Ok(PreparedGuestLifetime32 {
            references,
            memory_identity: memory.identity(),
            memory_version: memory.transaction_version(),
            ranges,
            writes: copied,
            write_count: writes.len(),
        })
    }

    pub(crate) fn can_commit_lifetime_changes(
        &self,
        memory: &AddressSpace,
        prepared: &PreparedGuestLifetime32,
    ) -> Result<(), GuestComError32> {
        if prepared.memory_identity != memory.identity()
            || prepared.memory_version != memory.transaction_version()
        {
            return Err(GuestComError32::StaleMapping);
        }
        self.registry
            .can_commit_owned_reference_changes(&prepared.references)?;
        for transition in prepared.references.transitions() {
            self.preflight(memory, transition.identity.identity_pointer)?;
        }
        Ok(())
    }

    pub(crate) fn commit_lifetime_changes(
        &mut self,
        memory: &mut AddressSpace,
        prepared: PreparedGuestLifetime32,
    ) -> Result<Vec<ComIdentity32>, GuestComError32> {
        self.can_commit_lifetime_changes(memory, &prepared)?;
        let writes = &prepared.writes[..prepared.write_count];
        if !prepared.ranges.is_empty() {
            memory.unmap_ranges_with_word_writes(&prepared.ranges, writes)?;
        } else if !writes.is_empty() {
            memory.write_words32(writes)?;
        }
        let destroyed = self
            .registry
            .commit_owned_reference_changes(prepared.references);
        if !destroyed.is_empty() {
            self.allocations.retain(|allocation| {
                destroyed
                    .binary_search_by_key(&allocation.id, |identity| identity.id)
                    .is_err()
            });
        }
        Ok(destroyed)
    }
}
