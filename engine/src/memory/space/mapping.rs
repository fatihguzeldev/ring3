use super::*;

pub(crate) struct PageInitialization<'a> {
    pub address: GuestAddress,
    pub permissions: Permissions,
    pub bytes: &'a [u8],
}

impl AddressSpace {
    #[cfg(test)]
    pub(crate) fn install_pages(
        &mut self,
        pages: &[PageInitialization<'_>],
    ) -> Result<(), MemoryError> {
        self.install_pages_with_word_writes(pages, &[])
    }

    pub(crate) fn install_pages_with_word_writes(
        &mut self,
        pages: &[PageInitialization<'_>],
        words: &[WordWrite32],
    ) -> Result<(), MemoryError> {
        if pages.is_empty() {
            return Err(MemoryError::InvalidRange);
        }
        let mut previous = None;
        for page in pages {
            if !page.address.0.is_multiple_of(PAGE_SIZE)
                || page.bytes.len() > PAGE_SIZE as usize
                || previous.is_some_and(|address| address >= page.address.0)
            {
                return Err(MemoryError::InvalidRange);
            }
            previous = Some(page.address.0);
            if self.mappings[(page.address.0 / PAGE_SIZE) as usize] != 0 {
                return Err(MemoryError::AlreadyMapped {
                    address: page.address,
                });
            }
        }
        if pages.len() > self.free.len() {
            return Err(MemoryError::Capacity);
        }
        self.validate_mapping_words(words)?;
        let version = self.advance_version()?;
        self.code_validation_version = version;
        for page in pages {
            let slot = self.free.pop().unwrap() as usize;
            let offset = slot * PAGE_SIZE as usize;
            let target = &mut self.backing[offset..offset + PAGE_SIZE as usize];
            target[..page.bytes.len()].copy_from_slice(page.bytes);
            target[page.bytes.len()..].fill(0);
            let guest_page = (page.address.0 / PAGE_SIZE) as usize;
            self.permissions[guest_page] = page.permissions;
            self.mapping_versions[guest_page] = version;
            self.references[slot] = 1;
            self.executable_views[slot] = u32::from(page.permissions.allows(Access::Execute));
            self.versions[slot] = PageVersion {
                mapping: version,
                content: version,
            };
            self.mappings[(page.address.0 / PAGE_SIZE) as usize] = slot as u32 + 1;
        }
        for word in words {
            self.copy_in_validated(word.address, &word.value.to_le_bytes(), version);
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn unmap_ranges(&mut self, ranges: &[PageRange]) -> Result<(), MemoryError> {
        self.unmap_ranges_with_word_writes(ranges, &[])
    }

    pub(crate) fn unmap_ranges_with_word_writes(
        &mut self,
        ranges: &[PageRange],
        words: &[WordWrite32],
    ) -> Result<(), MemoryError> {
        if ranges.is_empty() {
            return Err(MemoryError::InvalidRange);
        }
        let mut previous_end = 0;
        for range in ranges {
            let end = range
                .first
                .checked_add(range.count)
                .ok_or(MemoryError::InvalidRange)?;
            if range.count == 0 || range.first < previous_end || end > GUEST_PAGES {
                return Err(MemoryError::InvalidRange);
            }
            previous_end = end;
            self.require_mapped(*range)?;
        }
        self.validate_mapping_words(words)?;
        let mut range_index = 0;
        for word in words {
            let start = u64::from(word.address.0);
            while range_index < ranges.len()
                && (ranges[range_index].first + ranges[range_index].count) as u64
                    * u64::from(PAGE_SIZE)
                    <= start
            {
                range_index += 1;
            }
            if let Some(range) = ranges.get(range_index)
                && range.first as u64 * u64::from(PAGE_SIZE) < start + 4
            {
                return Err(MemoryError::InvalidRange);
            }
        }
        let version = self.advance_version()?;
        self.code_validation_version = version;
        for word in words {
            self.copy_in_validated(word.address, &word.value.to_le_bytes(), version);
        }
        for range in ranges {
            for page in range.indices() {
                self.release_mapping(page);
            }
        }
        Ok(())
    }

    fn validate_mapping_words(&self, words: &[WordWrite32]) -> Result<(), MemoryError> {
        let mut previous_end = 0;
        for word in words {
            let start = u64::from(word.address.0);
            if start < previous_end {
                return Err(MemoryError::InvalidRange);
            }
            self.check_access(word.address, 4, Access::Write)?;
            previous_end = start + 4;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
