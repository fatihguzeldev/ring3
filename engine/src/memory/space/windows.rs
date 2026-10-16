use super::*;

#[derive(Clone, Copy)]
pub(crate) enum PrivatePageChange32 {
    Commit {
        address: GuestAddress,
        permissions: Permissions,
    },
    Decommit {
        address: GuestAddress,
    },
    Protect {
        address: GuestAddress,
        permissions: Permissions,
    },
}
impl PrivatePageChange32 {
    fn address(self) -> GuestAddress {
        match self {
            Self::Commit { address, .. }
            | Self::Decommit { address }
            | Self::Protect { address, .. } => address,
        }
    }
}
impl AddressSpace {
    pub(crate) fn check_span_after_private_changes(
        &self,
        address: GuestAddress,
        length: usize,
        access: Access,
        changes: &[PrivatePageChange32],
    ) -> Result<(), MemoryError> {
        if changes.is_empty() {
            return self.check_span(address, length, access);
        }
        let end = u64::from(address.0)
            .checked_add(length as u64)
            .filter(|end| *end <= ADDRESS_LIMIT)
            .ok_or(MemoryError::InvalidRange)?;
        let mut at = u64::from(address.0);
        while at < end {
            let page = at / u64::from(PAGE_SIZE);
            let change = changes
                .iter()
                .find(|change| u64::from(change.address().0) == page * u64::from(PAGE_SIZE));
            let permissions = match change {
                Some(PrivatePageChange32::Commit { permissions, .. }) => Some(*permissions),
                Some(PrivatePageChange32::Protect { permissions, .. }) => {
                    self.permissions_at(GuestAddress(at as u32))?;
                    Some(*permissions)
                }
                Some(PrivatePageChange32::Decommit { .. }) => None,
                None => Some(self.permissions_at(GuestAddress(at as u32))?),
            };
            if permissions.is_none_or(|permissions| !permissions.allows(access)) {
                return Err(MemoryError::Fault(MemoryFault {
                    address: GuestAddress(at as u32),
                    access,
                    reason: if permissions.is_none() {
                        FaultReason::Unmapped
                    } else {
                        FaultReason::Permission
                    },
                }));
            }
            at = ((page + 1) * u64::from(PAGE_SIZE)).min(end);
        }
        Ok(())
    }

    pub(crate) fn read_after_private_changes(
        &self,
        address: GuestAddress,
        output: &mut [u8],
        changes: &[PrivatePageChange32],
    ) -> Result<(), MemoryError> {
        if changes.is_empty() {
            return self.read(address, output);
        }
        self.check_span_after_private_changes(address, output.len(), Access::Read, changes)?;
        let mut copied = 0;
        while copied < output.len() {
            let current = u64::from(address.0) + copied as u64;
            let page = (current / u64::from(PAGE_SIZE)) as usize;
            let within = (current % u64::from(PAGE_SIZE)) as usize;
            let count = (PAGE_SIZE as usize - within).min(output.len() - copied);
            if self.mappings[page] == 0 {
                return Err(MemoryError::NotMapped {
                    address: page_address(page),
                });
            }
            let offset = (self.mappings[page] as usize - 1) * PAGE_SIZE as usize + within;
            output[copied..copied + count].copy_from_slice(&self.backing[offset..offset + count]);
            copied += count;
        }
        Ok(())
    }

    pub(crate) fn write_thread_words32(
        &mut self,
        words: &[WordWrite32],
    ) -> Result<(), MemoryError> {
        if words.len() > 131_072 {
            return Err(MemoryError::InvalidRange);
        }
        for word in words {
            self.check_access(word.address, 4, Access::Write)?;
        }
        if words.is_empty() {
            return Ok(());
        }
        let version = self.advance_version()?;
        for word in words {
            self.copy_in_validated(word.address, &word.value.to_le_bytes(), version);
        }
        Ok(())
    }
    pub(crate) fn find_unmapped_range(
        &self,
        bounds: PageRange,
        count: u32,
        alignment: u32,
    ) -> Result<PageRange, MemoryError> {
        if count == 0 || alignment < PAGE_SIZE || !alignment.is_power_of_two() {
            return Err(MemoryError::InvalidRange);
        }
        let count = count as usize;
        let stride = (alignment / PAGE_SIZE) as usize;
        let end = bounds.first + bounds.count;
        let mut first = bounds.first.div_ceil(stride) * stride;
        while first.checked_add(count).is_some_and(|last| last <= end) {
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
    // page state and guest outputs share one version; no partial heap/virtual-memory mutation.
    pub(crate) fn apply_private_pages(
        &mut self,
        changes: &[PrivatePageChange32],
        writes: &[(GuestAddress, &[u8])],
    ) -> Result<(), MemoryError> {
        if writes.len() > 5 {
            return Err(MemoryError::InvalidRange);
        }
        let mut previous = None;
        let mut commits = 0;
        for &change in changes {
            let address = change.address();
            if !address.0.is_multiple_of(PAGE_SIZE) || previous.is_some_and(|old| old >= address.0)
            {
                return Err(MemoryError::InvalidRange);
            }
            previous = Some(address.0);
            let page = (address.0 / PAGE_SIZE) as usize;
            match change {
                PrivatePageChange32::Commit { .. } => {
                    if self.mappings[page] != 0 {
                        return Err(MemoryError::AlreadyMapped { address });
                    }
                    commits += 1;
                }
                PrivatePageChange32::Decommit { .. } | PrivatePageChange32::Protect { .. } => {
                    if self.mappings[page] == 0 {
                        return Err(MemoryError::NotMapped { address });
                    }
                }
            }
        }
        if commits > self.free.len() {
            return Err(MemoryError::Capacity);
        }
        for &(address, bytes) in writes {
            let end = u64::from(address.0)
                .checked_add(bytes.len() as u64)
                .filter(|end| *end <= ADDRESS_LIMIT)
                .ok_or(MemoryError::InvalidRange)?;
            let mut at = u64::from(address.0);
            while at < end {
                let page = (at / u64::from(PAGE_SIZE)) as usize;
                let new = changes
                    .binary_search_by_key(&(page as u32 * PAGE_SIZE), |change| change.address().0)
                    .ok()
                    .and_then(|index| match changes[index] {
                        PrivatePageChange32::Commit { permissions, .. }
                        | PrivatePageChange32::Protect { permissions, .. } => Some(permissions),
                        _ => None,
                    });
                if let Some(permissions) = new {
                    if !permissions.allows(Access::Write) {
                        return Err(MemoryError::Fault(MemoryFault {
                            address: GuestAddress(at as u32),
                            access: Access::Write,
                            reason: FaultReason::Permission,
                        }));
                    }
                } else {
                    self.access_slot(page, GuestAddress(at as u32), Access::Write)?;
                }
                at = ((at / u64::from(PAGE_SIZE) + 1) * u64::from(PAGE_SIZE)).min(end);
            }
        }
        if changes.is_empty() && writes.iter().all(|(_, bytes)| bytes.is_empty()) {
            return Ok(());
        }
        let version = self.advance_version()?;
        for &change in changes {
            if let PrivatePageChange32::Commit {
                address,
                permissions,
            } = change
            {
                let page = (address.0 / PAGE_SIZE) as usize;
                let slot = self.free.pop().unwrap() as usize;
                self.backing[slot * PAGE_SIZE as usize..(slot + 1) * PAGE_SIZE as usize].fill(0);
                self.mappings[page] = slot as u32 + 1;
                self.permissions[page] = permissions;
                self.mapping_versions[page] = version;
                self.record_mapping_incarnation(page, version);
                self.references[slot] = 1;
                self.executable_views[slot] = u32::from(permissions.allows(Access::Execute));
                self.versions[slot] = PageVersion {
                    mapping: version,
                    content: version,
                };
            }
        }
        for &(address, bytes) in writes {
            self.copy_in_validated(address, bytes, version);
        }
        for &change in changes {
            let page = (change.address().0 / PAGE_SIZE) as usize;
            match change {
                PrivatePageChange32::Decommit { .. } => self.release_mapping(page),
                PrivatePageChange32::Protect { permissions, .. } => {
                    let slot = self.mappings[page] as usize - 1;
                    self.change_permissions(page, slot, permissions, version);
                }
                PrivatePageChange32::Commit { .. } => {}
            }
        }
        if !changes.is_empty() {
            self.code_validation_version = version;
        }
        Ok(())
    }
}
