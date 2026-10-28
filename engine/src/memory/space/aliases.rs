use super::*;

pub(crate) struct SharedPages {
    identity: u64,
    slots: Box<[u32]>,
}

pub(crate) struct PreparedSharedPages {
    identity: u64,
    version: u64,
    slots: Box<[u32]>,
    bytes: Vec<u8>,
}

impl AddressSpace {
    pub(crate) fn find_alias_range(
        &self,
        bounds: PageRange,
        page_count: u32,
        alignment: u32,
    ) -> Result<PageRange, MemoryError> {
        if page_count == 0 || alignment < PAGE_SIZE || !alignment.is_power_of_two() {
            return Err(MemoryError::InvalidRange);
        }
        let count = page_count as usize;
        let stride = (alignment / PAGE_SIZE) as usize;
        let mut first = bounds.first.div_ceil(stride) * stride;
        let end = bounds.first + bounds.count;
        while first + count <= end {
            if let Some(occupied) = self.mappings[first..first + count]
                .iter()
                .position(|mapping| *mapping != 0)
            {
                first = (first + occupied + 1).div_ceil(stride) * stride;
            } else {
                return Ok(PageRange { first, count });
            }
        }
        Err(MemoryError::Capacity)
    }

    pub(crate) fn prepare_shared_pages(
        &self,
        page_count: u32,
        bytes: &[u8],
    ) -> Result<PreparedSharedPages, MemoryError> {
        let count = page_count as usize;
        if count == 0 || bytes.len() as u64 > u64::from(page_count) * u64::from(PAGE_SIZE) {
            return Err(MemoryError::InvalidRange);
        }
        if count > self.free.len() {
            return Err(MemoryError::Capacity);
        }
        self.ensure_transaction_versions(1)?;
        let mut slots = Vec::new();
        slots
            .try_reserve_exact(count)
            .map_err(|_| MemoryError::Allocation)?;
        slots.extend(self.free.iter().rev().take(count).copied());
        let mut data = Vec::new();
        data.try_reserve_exact(bytes.len())
            .map_err(|_| MemoryError::Allocation)?;
        data.extend_from_slice(bytes);
        Ok(PreparedSharedPages {
            identity: self.identity,
            version: self.version,
            slots: slots.into_boxed_slice(),
            bytes: data,
        })
    }
    pub(crate) fn commit_shared_pages(&mut self, prepared: PreparedSharedPages) -> SharedPages {
        assert_eq!(
            (self.identity, self.version),
            (prepared.identity, prepared.version)
        );
        let version = self.advance_version().unwrap();
        for (page, &slot) in prepared.slots.iter().enumerate() {
            assert_eq!(self.free.pop(), Some(slot));
            let offset = slot as usize * PAGE_SIZE as usize;
            let target = &mut self.backing[offset..offset + PAGE_SIZE as usize];
            target.fill(0);
            let source = (page * PAGE_SIZE as usize).min(prepared.bytes.len());
            let length = (prepared.bytes.len() - source).min(PAGE_SIZE as usize);
            target[..length].copy_from_slice(&prepared.bytes[source..source + length]);
            self.references[slot as usize] = 1;
            self.versions[slot as usize] = PageVersion {
                mapping: version,
                content: version,
            };
        }
        SharedPages {
            identity: self.identity,
            slots: prepared.slots,
        }
    }
    #[cfg(test)]
    pub(crate) fn allocate_shared_pages(
        &mut self,
        page_count: u32,
        bytes: &[u8],
    ) -> Result<SharedPages, MemoryError> {
        let prepared = self.prepare_shared_pages(page_count, bytes)?;
        Ok(self.commit_shared_pages(prepared))
    }
    pub(crate) fn release_shared_pages(&mut self, pages: SharedPages) -> Result<(), MemoryError> {
        self.validate_shared(&pages)?;
        for &slot in &pages.slots {
            self.release_slot(slot);
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn map_shared_pages(
        &mut self,
        range: PageRange,
        pages: &SharedPages,
        first: usize,
        permissions: Permissions,
        copy_on_write: bool,
    ) -> Result<(), MemoryError> {
        self.validate_shared(pages)?;
        let end = first
            .checked_add(range.count)
            .ok_or(MemoryError::InvalidRange)?;
        let slots = pages
            .slots
            .get(first..end)
            .ok_or(MemoryError::InvalidRange)?;
        for (page, &slot) in range.indices().zip(slots) {
            if self.mappings[page] != 0 {
                return Err(MemoryError::AlreadyMapped {
                    address: page_address(page),
                });
            }
            if self.references[slot as usize] == u32::MAX
                || (permissions.allows(Access::Execute)
                    && self.executable_views[slot as usize] == u32::MAX)
            {
                return Err(MemoryError::Capacity);
            }
        }
        if copy_on_write {
            if range.count > self.free.len() {
                return Err(MemoryError::Capacity);
            }
            if self.private_slots.is_none() {
                let private = filled(GUEST_PAGES, 0)?.into_boxed_slice();
                // allocate metadata before consuming a memory version or a physical slot.
                if self.version == u64::MAX {
                    return Err(MemoryError::VersionExhausted);
                }
                self.private_slots = Some(private);
            }
        }
        let version = self.advance_version()?;
        self.code_validation_version = version;
        self.code_source_version = version;
        for (page, &slot) in range.indices().zip(slots) {
            self.references[slot as usize] += 1;
            self.executable_views[slot as usize] += u32::from(permissions.allows(Access::Execute));
            self.mappings[page] = slot + 1;
            self.permissions[page] = permissions;
            self.mapping_versions[page] = version;
            self.record_mapping_incarnation(page, version);
            if copy_on_write {
                let private = self.free.pop().unwrap();
                self.references[private as usize] = 1;
                self.private_slots.as_mut().unwrap()[page] = private + 1;
            }
        }
        Ok(())
    }

    pub(crate) fn prepare_alias_capacity(
        &mut self,
        shared_pages: usize,
        private_pages: usize,
    ) -> Result<(), MemoryError> {
        if shared_pages
            .checked_add(private_pages)
            .is_none_or(|count| count > self.free.len())
        {
            return Err(MemoryError::Capacity);
        }
        self.ensure_transaction_versions(1 + u64::from(shared_pages != 0))?;
        if private_pages != 0 && self.private_slots.is_none() {
            self.private_slots = Some(filled(GUEST_PAGES, 0)?.into_boxed_slice());
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn map_shared_page_list(
        &mut self,
        range: PageRange,
        pages: &[(&SharedPages, usize)],
        permissions: Permissions,
        copy_on_write: bool,
    ) -> Result<(), MemoryError> {
        let mut rights = Vec::new();
        rights
            .try_reserve_exact(pages.len())
            .map_err(|_| MemoryError::Allocation)?;
        rights.extend(
            pages
                .iter()
                .map(|&(pages, index)| (pages, index, permissions)),
        );
        self.map_shared_page_rights(range, &rights, copy_on_write)
    }
    pub(crate) fn is_shared_alias(
        &self,
        address: GuestAddress,
        pages: &SharedPages,
        index: usize,
    ) -> bool {
        self.identity == pages.identity
            && pages.slots.get(index).is_some_and(|&slot| {
                self.mappings[address.0 as usize / PAGE_SIZE as usize] == slot + 1
            })
    }

    pub(crate) fn protect_copy_with_word_write32(
        &mut self,
        range: PageRange,
        permissions: Permissions,
        word: super::WordWrite32,
    ) -> Result<(), MemoryError> {
        self.require_mapped(range)?;
        self.check_access(word.address, 4, Access::Write)?;
        let required = range
            .indices()
            .filter(|&page| {
                self.references[self.mappings[page] as usize - 1] > 1
                    && self
                        .private_slots
                        .as_ref()
                        .is_none_or(|slots| slots[page] == 0)
            })
            .count();
        self.prepare_alias_capacity(0, required)?;
        let version = self.advance_version()?;
        for page in range.indices() {
            let slot = self.mappings[page] as usize - 1;
            if self.references[slot] > 1
                && self
                    .private_slots
                    .as_ref()
                    .is_none_or(|slots| slots[page] == 0)
            {
                let private = self.free.pop().unwrap();
                self.references[private as usize] = 1;
                self.private_slots.as_mut().unwrap()[page] = private + 1;
            }
        }
        self.copy_in_validated(word.address, &word.value.to_le_bytes(), version);
        self.code_validation_version = version;
        self.code_source_version = version;
        for page in range.indices() {
            let slot = self.mappings[page] as usize - 1;
            self.change_permissions(page, slot, permissions, version);
        }
        Ok(())
    }

    pub(crate) fn protect_alias_pages(
        &mut self,
        pages: &[(GuestAddress, Permissions)],
    ) -> Result<(), MemoryError> {
        if pages.is_empty() {
            return Err(MemoryError::InvalidRange);
        }
        for &(address, _) in pages {
            if !address.0.is_multiple_of(PAGE_SIZE) {
                return Err(MemoryError::InvalidRange);
            }
            self.require_mapped(PageRange::new(address, 1)?)?;
        }
        let version = self.advance_version()?;
        self.code_validation_version = version;
        self.code_source_version = version;
        for &(address, permissions) in pages {
            let page = (address.0 / PAGE_SIZE) as usize;
            let slot = self.mappings[page] as usize - 1;
            self.change_permissions(page, slot, permissions, version);
        }
        Ok(())
    }
    #[cfg(test)]
    pub(crate) fn map_shared_page_rights(
        &mut self,
        range: PageRange,
        pages: &[(&SharedPages, usize, Permissions)],
        copy_on_write: bool,
    ) -> Result<(), MemoryError> {
        self.map_shared_page_rights_with_word_writes(range, pages, copy_on_write, &[])
    }

    pub(crate) fn map_shared_page_rights_with_word_writes(
        &mut self,
        range: PageRange,
        pages: &[(&SharedPages, usize, Permissions)],
        copy_on_write: bool,
        words: &[WordWrite32],
    ) -> Result<(), MemoryError> {
        if range.count != pages.len() {
            return Err(MemoryError::InvalidRange);
        }
        for (page, (owner, index, permissions)) in range.indices().zip(pages) {
            self.validate_shared(owner)?;
            let slot = *owner.slots.get(*index).ok_or(MemoryError::InvalidRange)?;
            if self.mappings[page] != 0 {
                return Err(MemoryError::AlreadyMapped {
                    address: page_address(page),
                });
            }
            if self.references[slot as usize] > u32::MAX - GUEST_PAGES as u32 {
                return Err(MemoryError::Capacity);
            }
            if permissions.allows(Access::Execute)
                && self.executable_views[slot as usize] > u32::MAX - GUEST_PAGES as u32
            {
                return Err(MemoryError::Capacity);
            }
        }
        let mut previous_end = 0;
        for word in words {
            let start = u64::from(word.address.0);
            if start < previous_end {
                return Err(MemoryError::InvalidRange);
            }
            self.check_access(word.address, 4, Access::Write)?;
            previous_end = start + 4;
        }
        if copy_on_write {
            if range.count > self.free.len() {
                return Err(MemoryError::Capacity);
            }
            if self.private_slots.is_none() {
                let private = filled(GUEST_PAGES, 0)?.into_boxed_slice();
                self.ensure_transaction_versions(1)?;
                self.private_slots = Some(private);
            }
        }
        let version = self.advance_version()?;
        self.code_validation_version = version;
        self.code_source_version = version;
        for (page, (owner, index, permissions)) in range.indices().zip(pages) {
            let slot = owner.slots[*index];
            self.references[slot as usize] += 1;
            self.executable_views[slot as usize] += u32::from(permissions.allows(Access::Execute));
            self.mappings[page] = slot + 1;
            self.permissions[page] = *permissions;
            self.mapping_versions[page] = version;
            self.record_mapping_incarnation(page, version);
            if copy_on_write {
                let private = self.free.pop().unwrap();
                self.references[private as usize] = 1;
                self.private_slots.as_mut().unwrap()[page] = private + 1;
            }
        }
        for word in words {
            self.copy_in_validated(word.address, &word.value.to_le_bytes(), version);
        }
        Ok(())
    }
    pub(crate) fn read_shared_pages(
        &self,
        pages: &SharedPages,
        offset: usize,
        output: &mut [u8],
    ) -> Result<(), MemoryError> {
        self.validate_shared(pages)?;
        let end = offset
            .checked_add(output.len())
            .ok_or(MemoryError::InvalidRange)?;
        if end as u64 > pages.slots.len() as u64 * u64::from(PAGE_SIZE) {
            return Err(MemoryError::InvalidRange);
        }
        let mut copied = 0;
        while copied < output.len() {
            let current = offset + copied;
            let within = current % PAGE_SIZE as usize;
            let count = (PAGE_SIZE as usize - within).min(output.len() - copied);
            let slot = pages.slots[current / PAGE_SIZE as usize] as usize;
            let at = slot * PAGE_SIZE as usize + within;
            output[copied..copied + count].copy_from_slice(&self.backing[at..at + count]);
            copied += count;
        }
        Ok(())
    }

    pub(crate) fn shared_content_version(
        &self,
        pages: &SharedPages,
        page: usize,
    ) -> Result<u64, MemoryError> {
        self.validate_shared(pages)?;
        let slot = *pages.slots.get(page).ok_or(MemoryError::InvalidRange)?;
        Ok(self.versions[slot as usize].content)
    }

    fn validate_shared(&self, pages: &SharedPages) -> Result<(), MemoryError> {
        // the non-clone owner keeps its physical references until consumed by release.
        if pages.identity != self.identity {
            return Err(MemoryError::InvalidRange);
        }
        Ok(())
    }

    pub(super) fn page_version(&self, page: usize) -> PageVersion {
        PageVersion {
            mapping: self.mapping_versions[page],
            content: self.versions[self.mappings[page] as usize - 1].content,
        }
    }

    pub(super) fn change_permissions(
        &mut self,
        page: usize,
        slot: usize,
        permissions: Permissions,
        version: u64,
    ) {
        #[cfg(feature = "external-cpu")]
        {
            self.external_mapping_epoch = version;
        }
        self.executable_views[slot] -= u32::from(self.permissions[page].allows(Access::Execute));
        self.executable_views[slot] += u32::from(permissions.allows(Access::Execute));
        self.permissions[page] = permissions;
        self.mapping_versions[page] = version;
        self.versions[slot].mapping = version;
    }

    pub(super) fn release_mapping(&mut self, page: usize) {
        let slot = self.mappings[page] - 1;
        self.executable_views[slot as usize] -=
            u32::from(self.permissions[page].allows(Access::Execute));
        self.release_slot(slot);
        self.mappings[page] = 0;
        self.record_mapping_incarnation(page, 0);
        self.permissions[page] = Permissions::NONE;
        if let Some(private) = self.private_slots.as_mut() {
            let slot = std::mem::take(&mut private[page]);
            if slot != 0 {
                self.release_slot(slot - 1);
            }
        }
    }

    fn release_slot(&mut self, slot: u32) {
        self.references[slot as usize] -= 1;
        if self.references[slot as usize] == 0 {
            self.free.push(slot);
        }
    }

    #[inline]
    pub(super) fn writable_slot(&mut self, page: usize, version: u64) -> usize {
        let shared = self.mappings[page] as usize - 1;
        self.writable_slot_from(page, shared, version)
    }

    #[inline]
    pub(super) fn writable_slot_from(&mut self, page: usize, shared: usize, version: u64) -> usize {
        let private = self.private_slots.as_ref().map_or(0, |slots| slots[page]);
        if private == 0 {
            shared
        } else {
            self.detach_private_slot(page, shared, private as usize - 1, version)
        }
    }

    #[cold]
    #[inline(never)]
    fn detach_private_slot(
        &mut self,
        page: usize,
        shared: usize,
        private: usize,
        version: u64,
    ) -> usize {
        self.private_slots.as_mut().unwrap()[page] = 0;
        let from = shared * PAGE_SIZE as usize;
        let to = private * PAGE_SIZE as usize;
        self.backing
            .copy_within(from..from + PAGE_SIZE as usize, to);
        let executable = u32::from(self.permissions[page].allows(Access::Execute));
        self.executable_views[shared] -= executable;
        self.executable_views[private] = executable;
        self.versions[private] = PageVersion {
            mapping: self.mapping_versions[page],
            content: version,
        };
        self.mappings[page] = private as u32 + 1;
        #[cfg(feature = "external-cpu")]
        {
            self.external_mapping_epoch = version;
        }
        self.release_slot(shared as u32);
        private
    }
}

#[cfg(test)]
mod tests;
