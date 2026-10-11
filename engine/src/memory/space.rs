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
    code_validation_version: u64,
    versions: Box<[PageVersion]>,
}

impl AddressSpace {
    pub(crate) fn identity(&self) -> u64 {
        self.identity
    }

    pub(crate) fn code_validation_version(&self) -> u64 {
        self.code_validation_version
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
            code_validation_version: 0,
            versions,
        })
    }

    pub fn capacity_pages(&self) -> u32 {
        self.permissions.len() as u32
    }

    pub fn mapped_pages(&self) -> u32 {
        self.capacity_pages() - self.free.len() as u32
    }

    /// finds an unmapped fit without changing mappings or consuming versions.
    pub fn find_free_range(
        &self,
        bounds: PageRange,
        page_count: u32,
        alignment: u32,
    ) -> Result<PageRange, MemoryError> {
        if page_count == 0 || alignment < PAGE_SIZE || !alignment.is_power_of_two() {
            return Err(MemoryError::InvalidRange);
        }
        let count = page_count as usize;
        if count > self.free.len() || count > bounds.count {
            return Err(MemoryError::Capacity);
        }
        let stride = (alignment / PAGE_SIZE) as usize;
        let end = bounds.first + bounds.count;
        let mut first = bounds.first.div_ceil(stride) * stride;
        while first + count <= end {
            if self.mappings[first..first + count]
                .iter()
                .all(|mapping| *mapping == 0)
            {
                return Ok(PageRange { first, count });
            }
            first += stride;
        }
        Err(MemoryError::Capacity)
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
        self.code_validation_version = version;
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
        self.code_validation_version = version;
        for page in range.indices() {
            let slot = self.mappings[page] as usize - 1;
            self.permissions[slot] = permissions;
            self.versions[slot].mapping = version;
        }
        Ok(())
    }

    pub(crate) fn permissions_at(&self, address: GuestAddress) -> Result<Permissions, MemoryError> {
        let page = (address.0 / PAGE_SIZE) as usize;
        let mapping = self.mappings[page];
        if mapping == 0 {
            return Err(MemoryError::NotMapped {
                address: page_address(page),
            });
        }
        Ok(self.permissions[mapping as usize - 1])
    }

    pub(crate) fn protect_with_word_write32(
        &mut self,
        range: PageRange,
        permissions: Permissions,
        word: WordWrite32,
    ) -> Result<(), MemoryError> {
        self.require_mapped(range)?;
        self.check_access(word.address, 4, Access::Write)?;
        let version = self.advance_version()?;
        self.copy_in_validated(word.address, &word.value.to_le_bytes(), version);
        self.code_validation_version = version;
        for page in range.indices() {
            let slot = self.mappings[page] as usize - 1;
            self.permissions[slot] = permissions;
            self.versions[slot].mapping = version;
        }
        Ok(())
    }

    pub fn unmap(&mut self, range: PageRange) -> Result<(), MemoryError> {
        self.require_mapped(range)?;
        self.code_validation_version = self.advance_version()?;
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

    pub fn exchange32(&mut self, address: GuestAddress, value: u32) -> Result<u32, MemoryError> {
        let mut old = [0; 4];
        self.read(address, &mut old)?;
        self.check_access(address, old.len(), Access::Write)?;
        let version = self.advance_version()?;
        self.copy_in_validated(address, &value.to_le_bytes(), version);
        Ok(u32::from_le_bytes(old))
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
            let slot = self.mappings[page] as usize - 1;
            let offset = slot * PAGE_SIZE as usize + within;
            self.backing[offset..offset + count].copy_from_slice(&input[copied..copied + count]);
            self.versions[slot].content = version;
            if self.permissions[slot].allows(Access::Execute) {
                self.code_validation_version = version;
            }
            copied += count;
        }
    }

    fn copy_out(
        &self,
        address: GuestAddress,
        output: &mut [u8],
        access: Access,
    ) -> Result<(), MemoryError> {
        if output.is_empty() {
            return Ok(());
        }
        let within = address.0 as usize % PAGE_SIZE as usize;
        if output.len() <= PAGE_SIZE as usize - within {
            let page = address.0 as usize / PAGE_SIZE as usize;
            let slot = self.access_slot(page, address, access)?;
            let offset = slot * PAGE_SIZE as usize + within;
            output.copy_from_slice(&self.backing[offset..offset + output.len()]);
            return Ok(());
        }
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
#[path = "protection_transaction_tests.rs"]
mod protection_transaction_tests;

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
        assert_eq!(space.code_validation_version(), u64::MAX);
    }

    #[test]
    fn data_writes_preserve_code_validation_but_consume_the_shared_version_budget() {
        let mut space = AddressSpace::new(2).unwrap();
        space
            .map_zeroed(
                PageRange::new(GuestAddress(0x1000), 1).unwrap(),
                Permissions::READ_WRITE,
            )
            .unwrap();
        assert_eq!(space.code_validation_version(), space.version);
        space
            .map_zeroed(
                PageRange::new(GuestAddress(0x2000), 1).unwrap(),
                Permissions::ALL,
            )
            .unwrap();
        let snapshot = space.snapshot_code(GuestAddress(0x2000), 1).unwrap();
        let code_version = space.code_validation_version();
        space.version = u64::MAX - 2;

        space.write(GuestAddress(0x1000), &[1]).unwrap();
        assert_eq!(space.version, u64::MAX - 1);
        assert_eq!(space.code_validation_version(), code_version);
        let crossing = WordWrite32 {
            address: GuestAddress(0x1ffe),
            value: 0x4433_2211,
        };
        assert_eq!(
            space.write_words32(&[
                crossing,
                WordWrite32 {
                    address: GuestAddress(0x9000),
                    value: 0,
                },
            ]),
            Err(fault(
                GuestAddress(0x9000),
                Access::Write,
                FaultReason::Unmapped
            ))
        );
        assert_eq!(space.version, u64::MAX - 1);
        assert_eq!(space.code_validation_version(), code_version);
        assert!(space.is_code_current(&snapshot));

        space
            .write_words32(&[
                crossing,
                WordWrite32 {
                    address: GuestAddress(0x1004),
                    value: 0,
                },
            ])
            .unwrap();
        assert_eq!(space.version, u64::MAX);
        assert_eq!(space.code_validation_version(), u64::MAX);
        assert!(!space.is_code_current(&snapshot));
        let current = space.snapshot_code(GuestAddress(0x2000), 1).unwrap();
        assert_eq!(
            space.write(GuestAddress(0x2000), &[0]),
            Err(MemoryError::VersionExhausted)
        );
        space.write(GuestAddress(0x2000), &[]).unwrap();
        space.write_words32(&[]).unwrap();
        assert!(space.is_code_current(&current));
        assert_eq!(space.code_validation_version(), u64::MAX);
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

    #[derive(Debug, PartialEq)]
    struct ExchangeState {
        backing: Vec<u8>,
        mappings: Vec<u32>,
        permissions: Vec<Permissions>,
        free: Vec<u32>,
        versions: Vec<PageVersion>,
        identity: u64,
        version: u64,
        code_version: u64,
    }

    fn exchange_state(space: &AddressSpace) -> ExchangeState {
        ExchangeState {
            backing: space.backing.to_vec(),
            mappings: space.mappings.to_vec(),
            permissions: space.permissions.to_vec(),
            free: space.free.clone(),
            versions: space.versions.to_vec(),
            identity: space.identity,
            version: space.version,
            code_version: space.code_validation_version(),
        }
    }

    #[test]
    fn exchange_access_faults_preserve_all_owners_even_when_versions_are_exhausted() {
        for version in [u64::MAX - 1, u64::MAX] {
            for (first_bits, second_bits, address, access, at, reason) in [
                (
                    0,
                    Some(3),
                    0x1011,
                    Access::Read,
                    0x1011,
                    FaultReason::Permission,
                ),
                (
                    2,
                    Some(3),
                    0x1011,
                    Access::Read,
                    0x1011,
                    FaultReason::Permission,
                ),
                (
                    4,
                    Some(3),
                    0x1011,
                    Access::Read,
                    0x1011,
                    FaultReason::Permission,
                ),
                (
                    5,
                    Some(3),
                    0x1011,
                    Access::Write,
                    0x1011,
                    FaultReason::Permission,
                ),
                (
                    1,
                    Some(2),
                    0x1ffe,
                    Access::Read,
                    0x2000,
                    FaultReason::Permission,
                ),
                (1, None, 0x1ffe, Access::Read, 0x2000, FaultReason::Unmapped),
                (
                    7,
                    Some(1),
                    0x1ffe,
                    Access::Write,
                    0x2000,
                    FaultReason::Permission,
                ),
                (
                    1,
                    Some(3),
                    0x1ffe,
                    Access::Write,
                    0x1ffe,
                    FaultReason::Permission,
                ),
                (
                    7,
                    Some(3),
                    0x9001,
                    Access::Read,
                    0x9001,
                    FaultReason::Unmapped,
                ),
                (
                    7,
                    Some(3),
                    u32::MAX,
                    Access::Read,
                    u32::MAX,
                    FaultReason::AddressOverflow,
                ),
            ] {
                let mut space = AddressSpace::new(3).unwrap();
                let first = PageRange::new(GuestAddress(0x1000), 1).unwrap();
                let second = PageRange::new(GuestAddress(0x2000), 1).unwrap();
                space.map_zeroed(first, Permissions::ALL).unwrap();
                space
                    .write(GuestAddress(0x1ffc), &[0xa5, 0x5a, 0x11, 0x22])
                    .unwrap();
                if let Some(bits) = second_bits {
                    space.map_zeroed(second, Permissions::ALL).unwrap();
                    space
                        .write(GuestAddress(0x2000), &[0x33, 0x44, 0x5a, 0xa5])
                        .unwrap();
                    space
                        .protect(second, Permissions::from_bits(bits).unwrap())
                        .unwrap();
                }
                space
                    .protect(first, Permissions::from_bits(first_bits).unwrap())
                    .unwrap();
                space
                    .map_zeroed(
                        PageRange::new(GuestAddress(0), 1).unwrap(),
                        Permissions::ALL,
                    )
                    .unwrap();
                space
                    .write(GuestAddress(0), &[0xaa, 0x55, 0x33, 0xcc])
                    .unwrap();
                let snapshot = space.snapshot_code(GuestAddress(0), 4).unwrap();
                space.version = version;
                let before = exchange_state(&space);
                for _ in 0..2 {
                    assert_eq!(
                        space.exchange32(GuestAddress(address), 0xdead_beef),
                        Err(fault(GuestAddress(at), access, reason))
                    );
                    assert_eq!(exchange_state(&space), before);
                    assert!(space.is_code_current(&snapshot));
                }
            }
        }
    }

    #[test]
    fn exchange_uses_one_final_version_for_mixed_pages_and_identical_values() {
        for (first_permissions, second_permissions) in [
            (Permissions::READ_WRITE, Permissions::READ_WRITE),
            (Permissions::ALL, Permissions::READ_WRITE),
            (Permissions::READ_WRITE, Permissions::ALL),
            (Permissions::ALL, Permissions::ALL),
        ] {
            for identical in [false, true] {
                let mut space = AddressSpace::new(3).unwrap();
                for (base, permissions) in [
                    (0x1000, first_permissions),
                    (0x3000, Permissions::ALL),
                    (0x2000, second_permissions),
                ] {
                    space
                        .map_zeroed(PageRange::new(GuestAddress(base), 1).unwrap(), permissions)
                        .unwrap();
                }
                space
                    .write(
                        GuestAddress(0x1ffc),
                        &[0xa5, 0x5a, 0x11, 0x22, 0x33, 0x44, 0x5a, 0xa5],
                    )
                    .unwrap();
                let snapshots = [0x1000, 0x2000].map(|base| {
                    space
                        .permissions_at(GuestAddress(base))
                        .unwrap()
                        .allows(Access::Execute)
                        .then(|| space.snapshot_code(GuestAddress(base), 1).unwrap())
                });
                let unrelated = space.snapshot_code(GuestAddress(0x3000), 1).unwrap();
                space.version = u64::MAX - 1;
                let mut expected = exchange_state(&space);
                let value = if identical {
                    0x4433_2211_u32
                } else {
                    0xaabb_ccdd
                };
                for (index, byte) in value.to_le_bytes().into_iter().enumerate() {
                    let offset = space
                        .resolve(GuestAddress(0x1ffe + index as u32), Access::Write)
                        .unwrap()
                        .0;
                    expected.backing[offset as usize] = byte;
                }
                for base in [0x1000, 0x2000] {
                    let slot = space.mappings[base as usize / PAGE_SIZE as usize] as usize - 1;
                    expected.versions[slot].content = u64::MAX;
                }
                expected.version = u64::MAX;
                if snapshots.iter().any(Option::is_some) {
                    expected.code_version = u64::MAX;
                }
                assert_eq!(
                    space.exchange32(GuestAddress(0x1ffe), value),
                    Ok(0x4433_2211)
                );
                assert_eq!(exchange_state(&space), expected);
                for snapshot in snapshots.into_iter().flatten() {
                    assert!(!space.is_code_current(&snapshot));
                }
                assert!(space.is_code_current(&unrelated));
                for _ in 0..2 {
                    assert_eq!(
                        space.exchange32(GuestAddress(0x1ffe), 0xdead_beef),
                        Err(MemoryError::VersionExhausted)
                    );
                    assert_eq!(exchange_state(&space), expected);
                    assert!(space.is_code_current(&unrelated));
                }
            }
        }
    }

    #[test]
    fn failed_exchange_preflight_leaves_the_last_version_for_a_valid_exchange() {
        let mut space = AddressSpace::new(2).unwrap();
        space
            .map_zeroed(
                PageRange::new(GuestAddress(0x1000), 2).unwrap(),
                Permissions::ALL,
            )
            .unwrap();
        space
            .write(GuestAddress(0x1ffe), &[0x11, 0x22, 0x33, 0x44])
            .unwrap();
        space
            .protect(
                PageRange::new(GuestAddress(0x2000), 1).unwrap(),
                Permissions::READ,
            )
            .unwrap();
        let snapshot = space.snapshot_code(GuestAddress(0x1000), 1).unwrap();
        space.version = u64::MAX - 1;
        let before = exchange_state(&space);
        assert_eq!(
            space.exchange32(GuestAddress(0x1ffe), 0xaabb_ccdd),
            Err(fault(
                GuestAddress(0x2000),
                Access::Write,
                FaultReason::Permission
            ))
        );
        assert_eq!(exchange_state(&space), before);
        assert!(space.is_code_current(&snapshot));
        assert_eq!(space.exchange32(GuestAddress(0x1004), 0xaabb_ccdd), Ok(0));
        assert_eq!(space.version, u64::MAX);
        assert_eq!(space.code_validation_version(), u64::MAX);
        assert!(!space.is_code_current(&snapshot));
        let before = exchange_state(&space);
        assert_eq!(
            space.exchange32(GuestAddress(0x1ffe), 0),
            Err(fault(
                GuestAddress(0x2000),
                Access::Write,
                FaultReason::Permission
            ))
        );
        assert_eq!(exchange_state(&space), before);
    }
}
