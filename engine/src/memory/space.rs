use super::address::{ADDRESS_LIMIT, GUEST_PAGES};
use super::code::{PageVersion, new_identity};
use super::*;

pub struct AddressSpace {
    backing: Box<[u8]>,
    mappings: Box<[u32]>,
    permissions: Box<[Permissions]>,
    free: Vec<u32>,
    identity: u64,
    version: u64,
    versions: Box<[PageVersion]>,
}

impl AddressSpace {
    pub(crate) fn identity(&self) -> u64 {
        self.identity
    }

    pub fn snapshot_code(
        &self,
        address: GuestAddress,
        length: usize,
    ) -> Result<CodeSnapshot, MemoryError> {
        if length == 0 {
            return Err(MemoryError::InvalidRange);
        }
        self.check_access(address, length, Access::Execute)?;
        let first = (address.0 / PAGE_SIZE) as usize;
        let last = ((u64::from(address.0) + length as u64 - 1) / u64::from(PAGE_SIZE)) as usize;
        let mut versions = Vec::new();
        versions
            .try_reserve_exact(last - first + 1)
            .map_err(|_| MemoryError::Allocation)?;
        for page in first..=last {
            versions.push(self.versions[self.mappings[page] as usize - 1]);
        }
        Ok(CodeSnapshot {
            identity: self.identity,
            first,
            versions: versions.into_boxed_slice(),
        })
    }

    pub fn is_code_current(&self, snapshot: &CodeSnapshot) -> bool {
        self.identity == snapshot.identity
            && snapshot
                .versions
                .iter()
                .enumerate()
                .all(|(index, version)| {
                    let mapping = self.mappings[snapshot.first + index];
                    mapping != 0 && self.versions[mapping as usize - 1] == *version
                })
    }

    pub fn new(resident_pages: u32) -> Result<Self, MemoryError> {
        let bytes = u64::from(resident_pages) * u64::from(PAGE_SIZE);
        if resident_pages == 0 || bytes > u64::from(u32::MAX) || bytes > isize::MAX as u64 {
            return Err(MemoryError::Capacity);
        }
        let backing = filled(bytes as usize, 0)?.into_boxed_slice();
        let mappings = filled(GUEST_PAGES, 0)?.into_boxed_slice();
        let permissions = filled(resident_pages as usize, Permissions::NONE)?.into_boxed_slice();
        let versions = filled(resident_pages as usize, PageVersion::default())?.into_boxed_slice();
        let mut free = Vec::new();
        free.try_reserve_exact(resident_pages as usize)
            .map_err(|_| MemoryError::Allocation)?;
        free.extend((0..resident_pages).rev());
        Ok(Self {
            backing,
            mappings,
            permissions,
            free,
            identity: new_identity()?,
            version: 0,
            versions,
        })
    }

    pub fn capacity_pages(&self) -> u32 {
        self.permissions.len() as u32
    }

    pub fn mapped_pages(&self) -> u32 {
        self.capacity_pages() - self.free.len() as u32
    }

    pub fn map_zeroed(
        &mut self,
        range: PageRange,
        permissions: Permissions,
    ) -> Result<(), MemoryError> {
        for page in range.indices() {
            if self.mappings[page] != 0 {
                return Err(MemoryError::AlreadyMapped {
                    address: page_address(page),
                });
            }
        }
        if range.count > self.free.len() {
            return Err(MemoryError::Capacity);
        }
        let version = self.advance_version()?;
        for page in range.indices() {
            let slot = self.free.pop().unwrap();
            let offset = slot as usize * PAGE_SIZE as usize;
            self.backing[offset..offset + PAGE_SIZE as usize].fill(0);
            self.permissions[slot as usize] = permissions;
            self.versions[slot as usize] = PageVersion {
                mapping: version,
                content: version,
            };
            self.mappings[page] = slot + 1;
        }
        Ok(())
    }

    pub fn protect(
        &mut self,
        range: PageRange,
        permissions: Permissions,
    ) -> Result<(), MemoryError> {
        self.require_mapped(range)?;
        let version = self.advance_version()?;
        for page in range.indices() {
            let slot = self.mappings[page] as usize - 1;
            self.permissions[slot] = permissions;
            self.versions[slot].mapping = version;
        }
        Ok(())
    }

    pub fn unmap(&mut self, range: PageRange) -> Result<(), MemoryError> {
        self.require_mapped(range)?;
        self.advance_version()?;
        for page in range.indices() {
            let slot = self.mappings[page] - 1;
            self.mappings[page] = 0;
            self.permissions[slot as usize] = Permissions::NONE;
            self.free.push(slot);
        }
        Ok(())
    }

    pub fn resolve(
        &self,
        address: GuestAddress,
        access: Access,
    ) -> Result<BackingOffset, MemoryError> {
        let page = (address.0 / PAGE_SIZE) as usize;
        let slot = self.access_slot(page, address, access)?;
        Ok(BackingOffset(
            slot as u32 * PAGE_SIZE + address.0 % PAGE_SIZE,
        ))
    }

    pub fn read(&self, address: GuestAddress, output: &mut [u8]) -> Result<(), MemoryError> {
        self.copy_out(address, output, Access::Read)
    }

    pub fn fetch(&self, address: GuestAddress, output: &mut [u8]) -> Result<(), MemoryError> {
        self.copy_out(address, output, Access::Execute)
    }

    pub fn write(&mut self, address: GuestAddress, input: &[u8]) -> Result<(), MemoryError> {
        self.check_access(address, input.len(), Access::Write)?;
        if input.is_empty() {
            return Ok(());
        }
        let version = self.advance_version()?;
        self.copy_in_validated(address, input, version);
        Ok(())
    }

    pub fn write_words32(&mut self, words: &[WordWrite32]) -> Result<(), MemoryError> {
        if words.len() > MAX_WORD_WRITES32 {
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

    fn copy_in_validated(&mut self, address: GuestAddress, input: &[u8], version: u64) {
        let mut copied = 0;
        while copied < input.len() {
            let current = address.0 as u64 + copied as u64;
            let page = current as usize / PAGE_SIZE as usize;
            let within = current as usize % PAGE_SIZE as usize;
            let count = (PAGE_SIZE as usize - within).min(input.len() - copied);
            let offset = (self.mappings[page] as usize - 1) * PAGE_SIZE as usize + within;
            self.backing[offset..offset + count].copy_from_slice(&input[copied..copied + count]);
            self.versions[self.mappings[page] as usize - 1].content = version;
            copied += count;
        }
    }

    fn copy_out(
        &self,
        address: GuestAddress,
        output: &mut [u8],
        access: Access,
    ) -> Result<(), MemoryError> {
        self.check_access(address, output.len(), access)?;
        let mut copied = 0;
        while copied < output.len() {
            let current = address.0 as u64 + copied as u64;
            let page = current as usize / PAGE_SIZE as usize;
            let within = current as usize % PAGE_SIZE as usize;
            let count = (PAGE_SIZE as usize - within).min(output.len() - copied);
            let offset = (self.mappings[page] as usize - 1) * PAGE_SIZE as usize + within;
            output[copied..copied + count].copy_from_slice(&self.backing[offset..offset + count]);
            copied += count;
        }
        Ok(())
    }

    fn check_access(
        &self,
        address: GuestAddress,
        length: usize,
        access: Access,
    ) -> Result<(), MemoryError> {
        if length == 0 {
            return Ok(());
        }
        let end = u64::from(address.0).checked_add(length as u64);
        let Some(end) = end.filter(|&end| end <= ADDRESS_LIMIT) else {
            return Err(fault(address, access, FaultReason::AddressOverflow));
        };
        let first = address.0 as usize / PAGE_SIZE as usize;
        let last = ((end - 1) / u64::from(PAGE_SIZE)) as usize;
        for page in first..=last {
            let at = GuestAddress(address.0.max(page_address(page).0));
            self.access_slot(page, at, access)?;
        }
        Ok(())
    }

    fn access_slot(
        &self,
        page: usize,
        address: GuestAddress,
        access: Access,
    ) -> Result<usize, MemoryError> {
        let mapping = self.mappings[page];
        if mapping == 0 {
            return Err(fault(address, access, FaultReason::Unmapped));
        }
        let slot = mapping as usize - 1;
        if !self.permissions[slot].allows(access) {
            return Err(fault(address, access, FaultReason::Permission));
        }
        Ok(slot)
    }

    #[cfg(test)]
    pub(crate) fn exhaust_versions_for_test(&mut self) {
        self.version = u64::MAX;
    }

    fn advance_version(&mut self) -> Result<u64, MemoryError> {
        let next = self
            .version
            .checked_add(1)
            .ok_or(MemoryError::VersionExhausted)?;
        self.version = next;
        Ok(next)
    }

    fn require_mapped(&self, range: PageRange) -> Result<(), MemoryError> {
        for page in range.indices() {
            if self.mappings[page] == 0 {
                return Err(MemoryError::NotMapped {
                    address: page_address(page),
                });
            }
        }
        Ok(())
    }
}

fn filled<T: Clone>(count: usize, value: T) -> Result<Vec<T>, MemoryError> {
    let mut values = Vec::new();
    values
        .try_reserve_exact(count)
        .map_err(|_| MemoryError::Allocation)?;
    values.resize(count, value);
    Ok(values)
}

fn page_address(page: usize) -> GuestAddress {
    GuestAddress(page as u32 * PAGE_SIZE)
}

fn fault(address: GuestAddress, access: Access, reason: FaultReason) -> MemoryError {
    MemoryError::Fault(MemoryFault {
        address,
        access,
        reason,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocates_only_the_requested_resident_capacity() {
        let space = AddressSpace::new(2).unwrap();
        assert_eq!(space.capacity_pages(), 2);
        assert_eq!(space.mapped_pages(), 0);
        assert_eq!(space.backing.len(), 2 * PAGE_SIZE as usize);
    }

    #[test]
    fn rejects_unrepresentable_capacity_before_allocating() {
        assert!(matches!(AddressSpace::new(0), Err(MemoryError::Capacity)));
        assert!(matches!(
            AddressSpace::new(u32::MAX),
            Err(MemoryError::Capacity)
        ));
    }

    #[test]
    fn snapshots_track_the_current_executable_mapping() {
        let mut space = AddressSpace::new(1).unwrap();
        let range = PageRange::new(GuestAddress(0x1000), 1).unwrap();
        space.map_zeroed(range, Permissions::ALL).unwrap();
        let snapshot = space.snapshot_code(GuestAddress(0x1000), 4).unwrap();
        assert!(space.is_code_current(&snapshot));
        space.write(GuestAddress(0x1000), &[0x90]).unwrap();
        assert!(!space.is_code_current(&snapshot));
    }

    #[test]
    fn exhausted_versions_reject_every_mutation_without_changing_memory() {
        let mut space = AddressSpace::new(2).unwrap();
        let mapped = PageRange::new(GuestAddress(0x1000), 1).unwrap();
        let vacant = PageRange::new(GuestAddress(0x2000), 1).unwrap();
        space.map_zeroed(mapped, Permissions::ALL).unwrap();
        space.write(GuestAddress(0x1000), &[0x90]).unwrap();
        let snapshot = space.snapshot_code(GuestAddress(0x1000), 1).unwrap();
        space.version = u64::MAX;

        assert_eq!(
            space.map_zeroed(vacant, Permissions::ALL),
            Err(MemoryError::VersionExhausted)
        );
        assert_eq!(
            space.protect(mapped, Permissions::NONE),
            Err(MemoryError::VersionExhausted)
        );
        assert_eq!(space.unmap(mapped), Err(MemoryError::VersionExhausted));
        assert_eq!(
            space.write(GuestAddress(0x1000), &[0xcc]),
            Err(MemoryError::VersionExhausted)
        );
        space.write(GuestAddress(0x1000), &[]).unwrap();
        assert_eq!(space.mapped_pages(), 1);
        assert!(space.is_code_current(&snapshot));
        assert!(space.resolve(GuestAddress(0x2000), Access::Read).is_err());
        let mut bytes = [0];
        space.fetch(GuestAddress(0x1000), &mut bytes).unwrap();
        assert_eq!(bytes, [0x90]);
    }

    #[test]
    fn the_final_version_is_usable_but_never_wraps() {
        let mut space = AddressSpace::new(1).unwrap();
        let range = PageRange::new(GuestAddress(0), 1).unwrap();
        space.version = u64::MAX - 1;
        space.map_zeroed(range, Permissions::ALL).unwrap();
        let snapshot = space.snapshot_code(GuestAddress(0), 1).unwrap();
        assert_eq!(
            space.write(GuestAddress(0), &[1]),
            Err(MemoryError::VersionExhausted)
        );
        assert!(space.is_code_current(&snapshot));
        assert_eq!(space.version, u64::MAX);
    }

    #[test]
    fn word_batch_uses_one_last_version_for_all_pages_and_overlaps() {
        let mut space = AddressSpace::new(3).unwrap();
        space
            .map_zeroed(
                PageRange::new(GuestAddress(0x1000), 3).unwrap(),
                Permissions::ALL,
            )
            .unwrap();
        let first = space.snapshot_code(GuestAddress(0x1004), 4).unwrap();
        let crossing = space.snapshot_code(GuestAddress(0x1ffe), 6).unwrap();
        let unrelated = space.snapshot_code(GuestAddress(0x3000), 4).unwrap();
        let mappings = space
            .versions
            .iter()
            .map(|version| version.mapping)
            .collect::<Vec<_>>();
        let mut words = [WordWrite32 {
            address: GuestAddress(0x1004),
            value: 0,
        }; MAX_WORD_WRITES32];
        for (index, word) in words[..15].iter_mut().enumerate() {
            word.value = 0xa000_0000 + index as u32;
        }
        words[15] = WordWrite32 {
            address: GuestAddress(0x1ffe),
            value: 0x4433_2211,
        };
        words[16] = WordWrite32 {
            address: GuestAddress(0x2000),
            value: 0x8877_6655,
        };
        space.version = u64::MAX - 1;
        space.write_words32(&words).unwrap();
        assert_eq!(space.version, u64::MAX);
        assert!(!space.is_code_current(&first));
        assert!(!space.is_code_current(&crossing));
        assert!(space.is_code_current(&unrelated));
        for address in [0x1000, 0x2000] {
            let slot = space.mappings[address / PAGE_SIZE as usize] as usize - 1;
            assert_eq!(space.versions[slot].content, u64::MAX);
        }
        assert_eq!(
            space
                .versions
                .iter()
                .map(|version| version.mapping)
                .collect::<Vec<_>>(),
            mappings
        );
        let mut bytes = [0; 6];
        space.read(GuestAddress(0x1ffe), &mut bytes).unwrap();
        assert_eq!(bytes, [0x11, 0x22, 0x55, 0x66, 0x77, 0x88]);
        let current = space.snapshot_code(GuestAddress(0x1000), 8192).unwrap();
        let backing = space.backing.to_vec();
        assert_eq!(
            space.write_words32(&words[..1]),
            Err(MemoryError::VersionExhausted)
        );
        space.write_words32(&[]).unwrap();
        assert_eq!(
            space.write_words32(&[words[0]; 18]),
            Err(MemoryError::InvalidRange)
        );
        assert_eq!(
            space.write_words32(&[
                words[0],
                WordWrite32 {
                    address: GuestAddress(0x9000),
                    value: 1
                },
            ]),
            Err(fault(
                GuestAddress(0x9000),
                Access::Write,
                FaultReason::Unmapped
            ))
        );
        assert_eq!(space.backing.as_ref(), backing);
        assert!(space.is_code_current(&current));
        assert!(space.is_code_current(&unrelated));
        assert_eq!(space.version, u64::MAX);
        assert_eq!(space.mapped_pages(), 3);
    }

    #[test]
    fn failed_word_preflight_does_not_consume_the_last_available_version() {
        let mut space = AddressSpace::new(2).unwrap();
        space
            .map_zeroed(
                PageRange::new(GuestAddress(0x1000), 2).unwrap(),
                Permissions::ALL,
            )
            .unwrap();
        let snapshot = space.snapshot_code(GuestAddress(0x1ffe), 4).unwrap();
        let words = [
            WordWrite32 {
                address: GuestAddress(0x1ffe),
                value: 0x4433_2211,
            },
            WordWrite32 {
                address: GuestAddress(0x9000),
                value: 0x8877_6655,
            },
        ];
        let backing = space.backing.to_vec();
        space.version = u64::MAX - 1;
        assert_eq!(
            space.write_words32(&words),
            Err(fault(
                GuestAddress(0x9000),
                Access::Write,
                FaultReason::Unmapped
            ))
        );
        assert_eq!(space.version, u64::MAX - 1);
        assert_eq!(space.backing.as_ref(), backing);
        assert!(space.is_code_current(&snapshot));
        space.write_words32(&words[..1]).unwrap();
        assert_eq!(space.version, u64::MAX);
        assert!(!space.is_code_current(&snapshot));
    }
}
