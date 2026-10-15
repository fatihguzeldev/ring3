use super::{GuestComError32, GuestComObjects32};
use crate::{
    memory::{AddressSpace, GuestAddress, MAX_WORD_WRITES32, MemoryError, WordWrite32},
    windows::com::{ComIdentity32, ReferenceState32, ReferenceTransition32},
};

impl GuestComObjects32 {
    pub(crate) fn state(&self, pointer: GuestAddress) -> Result<ReferenceState32, GuestComError32> {
        Ok(self.registry.state(pointer)?)
    }

    pub(crate) fn preview_reference_changes(
        &self,
        memory: &AddressSpace,
        retain: &[GuestAddress],
        release: &[GuestAddress],
        hold: &[GuestAddress],
        unhold: &[GuestAddress],
    ) -> Result<Vec<ReferenceTransition32>, GuestComError32> {
        for &pointer in retain.iter().chain(release).chain(hold).chain(unhold) {
            self.preflight(memory, pointer)?;
        }
        Ok(self
            .registry
            .preview_reference_changes(retain, release, hold, unhold)?)
    }

    pub(crate) fn apply_reference_changes(
        &mut self,
        memory: &mut AddressSpace,
        retain: &[GuestAddress],
        release: &[GuestAddress],
    ) -> Result<Vec<ComIdentity32>, GuestComError32> {
        self.apply_lifetime_changes(memory, retain, release, &[], &[])
    }

    pub(crate) fn apply_lifetime_changes(
        &mut self,
        memory: &mut AddressSpace,
        retain: &[GuestAddress],
        release: &[GuestAddress],
        hold: &[GuestAddress],
        unhold: &[GuestAddress],
    ) -> Result<Vec<ComIdentity32>, GuestComError32> {
        self.apply_lifetime_changes_with_writes(memory, retain, release, hold, unhold, &[])
    }

    pub(crate) fn apply_lifetime_changes_with_writes(
        &mut self,
        memory: &mut AddressSpace,
        retain: &[GuestAddress],
        release: &[GuestAddress],
        hold: &[GuestAddress],
        unhold: &[GuestAddress],
        writes: &[WordWrite32],
    ) -> Result<Vec<ComIdentity32>, GuestComError32> {
        if writes.len() > MAX_WORD_WRITES32 {
            return Err(MemoryError::InvalidRange.into());
        }
        let mut ordered = [WordWrite32 {
            address: GuestAddress(0),
            value: 0,
        }; MAX_WORD_WRITES32];
        ordered[..writes.len()].copy_from_slice(writes);
        let ordered = &mut ordered[..writes.len()];
        ordered.sort_unstable_by_key(|word| word.address.0);
        if ordered
            .windows(2)
            .any(|pair| u64::from(pair[0].address.0) + 4 > u64::from(pair[1].address.0))
        {
            return Err(MemoryError::InvalidRange.into());
        }
        for &pointer in retain.iter().chain(release).chain(hold).chain(unhold) {
            self.preflight(memory, pointer)?;
        }
        let prepared = self
            .registry
            .prepare_reference_changes(retain, release, hold, unhold)?;
        let mut ranges = Vec::new();
        ranges
            .try_reserve_exact(prepared.destroyed().len())
            .map_err(|_| GuestComError32::Allocation)?;
        for identity in prepared.destroyed() {
            let index = self
                .allocations
                .binary_search_by_key(&identity.id, |allocation| allocation.id)
                .map_err(|_| GuestComError32::StaleMapping)?;
            ranges.push(self.allocations[index].range);
        }
        ranges.sort_unstable_by_key(|range| range.first);
        if !ranges.is_empty() {
            memory.unmap_ranges_with_word_writes(&ranges, ordered)?;
        } else if !ordered.is_empty() {
            memory.write_words32(ordered)?;
        }
        let destroyed = prepared.publish();
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

#[cfg(test)]
mod tests;
