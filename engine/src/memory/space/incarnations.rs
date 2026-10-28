use super::*;
impl AddressSpace {
    pub(crate) fn observe_mapping_incarnations(
        &mut self,
        range: PageRange,
    ) -> Result<(), MemoryError> {
        let end = range
            .first
            .checked_add(range.count)
            .filter(|end| *end <= GUEST_PAGES)
            .ok_or(MemoryError::InvalidRange)?;
        if range.count == 0 {
            return Err(MemoryError::InvalidRange);
        }
        let missing = (range.first..end)
            .filter(|page| {
                self.mapping_incarnations
                    .binary_search_by_key(page, |(page, _)| *page)
                    .is_err()
            })
            .count();
        self.mapping_incarnations
            .try_reserve(missing)
            .map_err(|_| MemoryError::Allocation)?;
        for page in range.first..end {
            if let Err(index) = self
                .mapping_incarnations
                .binary_search_by_key(&page, |(page, _)| *page)
            {
                let version = if self.mappings[page] == 0 {
                    0
                } else {
                    self.mapping_versions[page]
                };
                self.mapping_incarnations.insert(index, (page, version));
            }
        }
        Ok(())
    }
    pub(crate) fn mapping_incarnation_at(&self, address: GuestAddress) -> Result<u64, MemoryError> {
        self.permissions_at(address)?;
        let page = (address.0 / PAGE_SIZE) as usize;
        self.mapping_incarnations
            .binary_search_by_key(&page, |(page, _)| *page)
            .map(|index| self.mapping_incarnations[index].1)
            .map_err(|_| MemoryError::InvalidRange)
    }
    pub(super) fn record_mapping_incarnation(&mut self, page: usize, version: u64) {
        #[cfg(feature = "external-cpu")]
        {
            self.external_mapping_epoch = self.version;
        }
        if !self.mapping_incarnations.is_empty()
            && let Ok(index) = self
                .mapping_incarnations
                .binary_search_by_key(&page, |(page, _)| *page)
        {
            self.mapping_incarnations[index].1 = version;
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn range(address: u32, count: u32) -> PageRange {
        PageRange::new(GuestAddress(address), count).unwrap()
    }
    #[test]
    fn observed_identity_survives_rights_and_data_changes_but_rejects_same_address_remap() {
        let mut memory = AddressSpace::new(2).unwrap();
        let range = range(0x1000, 1);
        memory.map_zeroed(range, Permissions::READ_WRITE).unwrap();
        memory.observe_mapping_incarnations(range).unwrap();
        let identity = memory.mapping_incarnation_at(GuestAddress(0x1000)).unwrap();
        let strict = memory.mapping_version_at(GuestAddress(0x1000)).unwrap();
        memory.protect(range, Permissions::READ).unwrap();
        memory.protect(range, Permissions::READ_WRITE).unwrap();
        memory.write(GuestAddress(0x1000), &[7]).unwrap();
        assert_ne!(
            memory.mapping_version_at(GuestAddress(0x1000)).unwrap(),
            strict
        );
        assert_eq!(
            memory.mapping_incarnation_at(GuestAddress(0x1000)).unwrap(),
            identity
        );
        memory.unmap(range).unwrap();
        memory.map_zeroed(range, Permissions::READ_WRITE).unwrap();
        assert_ne!(
            memory.mapping_incarnation_at(GuestAddress(0x1000)).unwrap(),
            identity
        );
        assert_eq!(memory.mapping_incarnations.len(), 1);
    }
    #[test]
    fn alias_replacement_renews_identity_and_copy_on_write_preserves_the_same_view() {
        let mut memory = AddressSpace::new(3).unwrap();
        let pages = memory.allocate_shared_pages(1, &[1]).unwrap();
        let range = range(0x3000, 1);
        memory.observe_mapping_incarnations(range).unwrap();
        memory
            .map_shared_pages(range, &pages, 0, Permissions::READ_WRITE, true)
            .unwrap();
        let identity = memory.mapping_incarnation_at(GuestAddress(0x3000)).unwrap();
        memory.write(GuestAddress(0x3000), &[2]).unwrap();
        assert_eq!(
            memory.mapping_incarnation_at(GuestAddress(0x3000)).unwrap(),
            identity
        );
        memory.unmap(range).unwrap();
        memory
            .map_shared_pages(range, &pages, 0, Permissions::READ_WRITE, false)
            .unwrap();
        assert_ne!(
            memory.mapping_incarnation_at(GuestAddress(0x3000)).unwrap(),
            identity
        );
    }
}
