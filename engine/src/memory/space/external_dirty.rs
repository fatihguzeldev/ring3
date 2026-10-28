use super::*;

const TABLE_BYTES: usize = 4096 * 1025;
const TABLES_PHYSICAL: u32 = 0x201000;
const BACKING_PHYSICAL: u32 = 16 * 1024 * 1024;
const PRIVATE_PAGE: usize = 0xff000000usize >> 12;

pub(super) struct Collector {
    occupied: Box<[u32]>,
    expected: Box<[u32]>,
    directory_counts: [u16; 1024],
    directories: [u32; 32],
    slots: Vec<u32>,
    seen_slots: Box<[u32]>,
    dirty_ptes: Vec<u32>,
}

struct Observation {
    accessed: [u32; 32],
    leaves: u32,
    directories: u32,
}

fn word(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

fn put(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

impl Collector {
    fn new(memory: &AddressSpace) -> Result<Self, MemoryError> {
        let mut slots = Vec::new();
        slots
            .try_reserve_exact(memory.references.len())
            .map_err(|_| MemoryError::Allocation)?;
        let mut dirty_ptes = Vec::new();
        // aliases need one staged offset per dirty VA, independently of slot deduplication.
        dirty_ptes
            .try_reserve_exact(GUEST_PAGES)
            .map_err(|_| MemoryError::Allocation)?;
        let mut collector = Self {
            occupied: filled(GUEST_PAGES / 32, 0)?.into_boxed_slice(),
            expected: filled(GUEST_PAGES, 0)?.into_boxed_slice(),
            directory_counts: [0; 1024],
            directories: [0; 32],
            slots,
            seen_slots: filled(memory.references.len().div_ceil(32), 0)?.into_boxed_slice(),
            dirty_ptes,
        };
        for (page, &mapping) in memory.mappings.iter().enumerate() {
            if mapping != 0 {
                if (PRIVATE_PAGE..PRIVATE_PAGE + 8).contains(&page) {
                    return Err(MemoryError::InvalidRange);
                }
                collector.update_mapping(
                    page,
                    mapping,
                    memory.permissions[page],
                    memory
                        .private_slots
                        .as_ref()
                        .is_some_and(|slots| slots[page] != 0),
                );
            }
        }
        Ok(collector)
    }

    pub(super) fn update_mapping(
        &mut self,
        page: usize,
        mapping: u32,
        permissions: Permissions,
        private: bool,
    ) {
        let mapped = mapping != 0;
        let present = permissions.allows(Access::Read)
            || permissions.allows(Access::Write)
            || permissions.allows(Access::Execute);
        let writable = permissions.allows(Access::Write) && !private;
        // slot and permission changes can keep the same mapped presence.
        self.expected[page] = if mapped {
            (BACKING_PHYSICAL + (mapping - 1) * 4096)
                | if present { 5 } else { 0 }
                | if writable { 2 } else { 0 }
        } else {
            0
        };
        let bit = 1u32 << (page % 32);
        let present = self.occupied[page / 32] & bit != 0;
        if present == mapped {
            return;
        }
        let directory = page >> 10;
        if mapped {
            self.occupied[page / 32] |= bit;
            self.directory_counts[directory] += 1;
            self.directories[directory / 32] |= 1u32 << (directory % 32);
        } else {
            self.occupied[page / 32] &= !bit;
            self.directory_counts[directory] -= 1;
            if self.directory_counts[directory] == 0 {
                self.directories[directory / 32] &= !(1u32 << (directory % 32));
            }
        }
    }

    fn observe(&mut self, tables: &[u8]) -> Result<Observation, MemoryError> {
        for &slot in &self.slots {
            self.seen_slots[slot as usize / 32] &= !(1u32 << (slot % 32));
        }
        self.slots.clear();
        self.dirty_ptes.clear();
        let mut observation = Observation {
            accessed: [0; 32],
            leaves: 0,
            directories: 0,
        };
        for (group, &directories) in self.directories.iter().enumerate() {
            let mut remaining = directories;
            while remaining != 0 {
                let directory = group * 32 + remaining.trailing_zeros() as usize;
                remaining &= remaining - 1;
                observation.directories += 1;
                let entry = word(tables, directory * 4);
                if entry & !32 != (TABLES_PHYSICAL + directory as u32 * 4096) | 7 {
                    return Err(MemoryError::InvalidRange);
                }
                if entry & 32 == 0 {
                    continue;
                }
                observation.accessed[group] |= 1u32 << (directory % 32);
                for index in directory * 32..directory * 32 + 32 {
                    let mut pages = self.occupied[index];
                    while pages != 0 {
                        let page = index * 32 + pages.trailing_zeros() as usize;
                        pages &= pages - 1;
                        observation.leaves += 1;
                        if (PRIVATE_PAGE..PRIVATE_PAGE + 8).contains(&page) {
                            return Err(MemoryError::InvalidRange);
                        }
                        let expected = self.expected[page];
                        let offset = 4096 + page * 4;
                        let pte = word(tables, offset);
                        if pte & !96 != expected {
                            return Err(MemoryError::InvalidRange);
                        }
                        if pte & 64 != 0 {
                            self.dirty_ptes.push(offset as u32);
                            let slot = ((expected & !4095) - BACKING_PHYSICAL) >> 12;
                            let bit = 1u32 << (slot % 32);
                            if self.seen_slots[slot as usize / 32] & bit == 0 {
                                self.seen_slots[slot as usize / 32] |= bit;
                                self.slots.push(slot);
                            }
                        }
                    }
                }
            }
        }
        Ok(observation)
    }

    fn clear_observed(&self, tables: &mut [u8], observation: &Observation) {
        for (group, &directories) in observation.accessed.iter().enumerate() {
            let mut remaining = directories;
            while remaining != 0 {
                let directory = group * 32 + remaining.trailing_zeros() as usize;
                remaining &= remaining - 1;
                let offset = directory * 4;
                let entry = word(tables, offset) & !32;
                put(tables, offset, entry);
            }
        }
        for &offset in &self.dirty_ptes {
            let offset = offset as usize;
            let pte = word(tables, offset) & !64;
            put(tables, offset, pte);
        }
    }
}

impl AddressSpace {
    pub(crate) fn collect_external_dirty_tables(
        &mut self,
        epoch: u64,
        tables: &mut crate::abi::external_memory::ExternalMemory,
    ) -> Result<[u8; 80], MemoryError> {
        if !tables.is_cpu_page_tables_for(self.backing.as_ptr() as usize) {
            return Err(MemoryError::InvalidRange);
        }
        self.collect_external_dirty(epoch, tables)
    }

    pub(crate) fn collect_external_dirty(
        &mut self,
        epoch: u64,
        tables: &mut [u8],
    ) -> Result<[u8; 80], MemoryError> {
        if tables.len() != TABLE_BYTES
            || epoch != self.external_mapping_epoch
            || self
                .external_mapping_changes
                .as_ref()
                .is_none_or(|changes| !changes.pages.is_empty())
        {
            return Err(MemoryError::InvalidRange);
        }
        let mut collector = match self.external_dirty.take() {
            Some(collector) => collector,
            None => Collector::new(self)?,
        };
        let result = (|| {
            let observation = collector.observe(tables)?;
            // publication validates every physical slot and the next version before
            // consuming any hardware dirty/accessed evidence.
            self.publish_external_dirty(&collector.slots)?;
            collector.clear_observed(tables, &observation);
            let reset_access = observation.accessed.iter().any(|&bits| bits != 0);
            let mut bytes = [0; 80];
            crate::abi::header::write(&mut bytes, *b"R3DC");
            for (offset, value) in [
                (16, collector.slots.as_ptr() as usize as u32),
                (20, collector.slots.len() as u32),
                (24, u32::from(reset_access)),
                (28, u32::from(reset_access || !collector.slots.is_empty())),
                (32, observation.leaves),
                (36, observation.directories),
                (72, self.capacity_pages()),
                (76, self.versions.as_ptr() as usize as u32),
            ] {
                put(&mut bytes, offset, value);
            }
            for (offset, value) in [
                (40, self.external_mapping_epoch),
                (48, self.version),
                (56, self.code_validation_version),
                (64, self.identity),
            ] {
                bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
            }
            Ok(bytes)
        })();
        self.external_dirty = Some(collector);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn range(address: u32) -> PageRange {
        PageRange::new(GuestAddress(address), 1).unwrap()
    }

    fn word(bytes: &[u8], offset: usize) -> u32 {
        u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
    }

    fn put(bytes: &mut [u8], offset: usize, value: u32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn pte(page: usize) -> usize {
        4096 + page * 4
    }

    fn tables(memory: &AddressSpace) -> Vec<u8> {
        let mut bytes = vec![0; TABLE_BYTES];
        for directory in 0..1024 {
            put(
                &mut bytes,
                directory * 4,
                TABLES_PHYSICAL + directory as u32 * 4096 | 7,
            );
        }
        for (page, &mapping) in memory.mappings.iter().enumerate() {
            if mapping == 0 {
                continue;
            }
            let permissions = memory.permissions[page];
            let present = permissions.allows(Access::Read)
                || permissions.allows(Access::Write)
                || permissions.allows(Access::Execute);
            let writable = permissions.allows(Access::Write)
                && memory
                    .private_slots
                    .as_ref()
                    .is_none_or(|slots| slots[page] == 0);
            put(
                &mut bytes,
                pte(page),
                BACKING_PHYSICAL + (mapping - 1) * 4096
                    | if present { 5 } else { 0 }
                    | if writable { 2 } else { 0 },
            );
        }
        bytes
    }

    fn track(memory: &mut AddressSpace) {
        memory.external_mapping_changes = Some(external::MappingChanges::new().unwrap());
    }

    fn access(bytes: &mut [u8], page: usize, dirty: bool) {
        let offset = (page >> 10) * 4;
        let directory = word(bytes, offset) | 32;
        put(bytes, offset, directory);
        if dirty {
            let value = word(bytes, pte(page)) | 64;
            put(bytes, pte(page), value);
        }
    }

    #[test]
    fn external_dirty_collect_deduplicates_aliases_and_ignores_private_pages() {
        let mut memory = AddressSpace::new(4).unwrap();
        let shared = memory.allocate_shared_pages(1, &[7]).unwrap();
        memory
            .map_shared_pages(range(0x1000), &shared, 0, Permissions::READ_WRITE, false)
            .unwrap();
        memory
            .map_shared_pages(range(0x2000), &shared, 0, Permissions::READ_EXECUTE, false)
            .unwrap();
        memory
            .map_zeroed(range(0x400000), Permissions::NONE)
            .unwrap();
        track(&mut memory);
        let mut bytes = tables(&memory);
        access(&mut bytes, 1, true);
        access(&mut bytes, 2, true);
        access(&mut bytes, 1024, false);
        access(&mut bytes, PRIVATE_PAGE, true);
        let private_before = word(&bytes, pte(PRIVATE_PAGE));
        let old_version = memory.transaction_version();
        let result = memory
            .collect_external_dirty(memory.external_mapping_epoch, &mut bytes)
            .unwrap();
        assert_eq!(&result[..4], b"R3DC");
        assert_eq!(word(&result, 8), 80);
        assert_eq!(word(&result, 20), 1, "physical aliases publish one slot");
        assert_eq!(word(&result, 24), 1);
        assert_eq!(word(&result, 28), 1);
        assert_eq!(
            word(&result, 32),
            3,
            "mapped NOACCESS page is still resident"
        );
        assert_eq!(word(&result, 36), 2);
        assert!(memory.transaction_version() > old_version);
        assert_eq!(
            memory.code_validation_version(),
            memory.transaction_version()
        );
        assert_eq!(word(&bytes, pte(1)) & 64, 0);
        assert_eq!(word(&bytes, pte(2)) & 64, 0);
        assert_eq!(word(&bytes, 0) & 32, 0);
        assert_eq!(word(&bytes, 4) & 32, 0);
        assert_eq!(word(&bytes, pte(PRIVATE_PAGE)), private_before);
        assert_ne!(word(&bytes, (PRIVATE_PAGE >> 10) * 4) & 32, 0);
    }

    #[test]
    fn external_dirty_collect_read_only_reset_requires_flush_then_empty_does_not() {
        let mut memory = AddressSpace::new(2).unwrap();
        memory
            .map_zeroed(range(0x1000), Permissions::READ_WRITE)
            .unwrap();
        track(&mut memory);
        let mut bytes = tables(&memory);
        access(&mut bytes, 1, false);
        let before = memory.transaction_version();
        let result = memory
            .collect_external_dirty(memory.external_mapping_epoch, &mut bytes)
            .unwrap();
        assert_eq!(word(&result, 20), 0);
        assert_eq!(word(&result, 24), 1);
        assert_eq!(word(&result, 28), 1);
        assert_eq!(memory.transaction_version(), before);
        let empty = memory
            .collect_external_dirty(memory.external_mapping_epoch, &mut bytes)
            .unwrap();
        assert_eq!(word(&empty, 24), 0);
        assert_eq!(word(&empty, 28), 0);
        assert_eq!(word(&empty, 32), 0);
        access(&mut bytes, 1, true);
        let written = memory
            .collect_external_dirty(memory.external_mapping_epoch, &mut bytes)
            .unwrap();
        assert_eq!(word(&written, 20), 1);
        assert_eq!(word(&written, 28), 1);
        assert_eq!(
            word(&result, 16),
            word(&written, 16),
            "preallocated result pointer stays stable"
        );
    }

    #[test]
    fn external_dirty_collect_stale_epoch_pending_maps_and_malformed_tables_are_atomic() {
        let mut memory = AddressSpace::new(3).unwrap();
        memory
            .map_zeroed(range(0x1000), Permissions::READ_WRITE)
            .unwrap();
        memory
            .map_zeroed(range(0x2000), Permissions::READ_WRITE)
            .unwrap();
        track(&mut memory);
        let mut bytes = tables(&memory);
        access(&mut bytes, 1, true);
        access(&mut bytes, 2, true);
        let before = bytes.clone();
        let version = memory.transaction_version();
        assert_eq!(
            memory.collect_external_dirty(memory.external_mapping_epoch + 1, &mut bytes),
            Err(MemoryError::InvalidRange)
        );
        assert_eq!(bytes, before);
        assert_eq!(memory.transaction_version(), version);
        let at = pte(2);
        put(&mut bytes, at, word(&before, at) ^ 4096);
        let malformed = bytes.clone();
        assert_eq!(
            memory.collect_external_dirty(memory.external_mapping_epoch, &mut bytes),
            Err(MemoryError::InvalidRange)
        );
        assert_eq!(bytes, malformed);
        assert_eq!(memory.transaction_version(), version);
        let mut retry = before;
        let collected = memory
            .collect_external_dirty(memory.external_mapping_epoch, &mut retry)
            .unwrap();
        assert_eq!(
            word(&collected, 20),
            2,
            "a failed scan does not strand staged evidence"
        );
        assert_eq!(word(&retry, pte(1)) & 64, 0);
        assert_eq!(word(&retry, pte(2)) & 64, 0);
        memory.protect(range(0x1000), Permissions::READ).unwrap();
        let mut pending = tables(&memory);
        access(&mut pending, 2, true);
        let before = pending.clone();
        assert_eq!(
            memory.collect_external_dirty(memory.external_mapping_epoch, &mut pending),
            Err(MemoryError::InvalidRange)
        );
        assert_eq!(pending, before);
    }

    #[test]
    fn external_dirty_collect_version_exhaustion_preserves_evidence() {
        let mut memory = AddressSpace::new(2).unwrap();
        memory
            .map_zeroed(range(0x1000), Permissions::READ_WRITE)
            .unwrap();
        track(&mut memory);
        let mut bytes = tables(&memory);
        access(&mut bytes, 1, true);
        let before = bytes.clone();
        memory.version = u64::MAX;
        assert_eq!(
            memory.collect_external_dirty(memory.external_mapping_epoch, &mut bytes),
            Err(MemoryError::VersionExhausted)
        );
        assert_eq!(bytes, before);
        assert_eq!(memory.transaction_version(), u64::MAX);
        memory.version = memory.external_mapping_epoch;
        let retried = memory
            .collect_external_dirty(memory.external_mapping_epoch, &mut bytes)
            .unwrap();
        assert_eq!(
            word(&retried, 20),
            1,
            "publication failure leaves the next collection complete"
        );
        assert_eq!(word(&bytes, pte(1)) & 64, 0);
    }

    #[test]
    fn external_dirty_collect_permission_only_ack_rejects_stale_ptes_and_preserves_presence() {
        let mut memory = AddressSpace::new(2).unwrap();
        memory
            .map_zeroed(range(0x1000), Permissions::READ_WRITE)
            .unwrap();
        track(&mut memory);
        let mut bytes = tables(&memory);
        memory
            .collect_external_dirty(memory.external_mapping_epoch, &mut bytes)
            .unwrap();
        let writable = bytes.clone();
        let mapping = memory.mappings[1];
        memory.protect(range(0x1000), Permissions::READ).unwrap();
        memory
            .acknowledge_external_mappings(memory.external_mapping_epoch, 1)
            .unwrap();
        assert_eq!(memory.mappings[1], mapping);
        let mut stale = writable;
        access(&mut stale, 1, true);
        let before = stale.clone();
        let version = memory.transaction_version();
        assert_eq!(
            memory.collect_external_dirty(memory.external_mapping_epoch, &mut stale),
            Err(MemoryError::InvalidRange)
        );
        assert_eq!(stale, before);
        assert_eq!(memory.transaction_version(), version);
        let mut bytes = tables(&memory);
        access(&mut bytes, 1, false);
        let read = memory
            .collect_external_dirty(memory.external_mapping_epoch, &mut bytes)
            .unwrap();
        assert_eq!(word(&read, 20), 0);
        assert_eq!(word(&read, 32), 1);
        assert_eq!(word(&read, 28), 1);
        memory.protect(range(0x1000), Permissions::NONE).unwrap();
        memory
            .acknowledge_external_mappings(memory.external_mapping_epoch, 1)
            .unwrap();
        let mut bytes = tables(&memory);
        access(&mut bytes, 1, false);
        let none = memory
            .collect_external_dirty(memory.external_mapping_epoch, &mut bytes)
            .unwrap();
        assert_eq!(
            word(&none, 32),
            1,
            "permission-only NOACCESS retains occupied VA"
        );
        memory
            .protect(range(0x1000), Permissions::READ.with_guard())
            .unwrap();
        memory
            .acknowledge_external_mappings(memory.external_mapping_epoch, 1)
            .unwrap();
        let mut bytes = tables(&memory);
        access(&mut bytes, 1, false);
        let guarded = memory
            .collect_external_dirty(memory.external_mapping_epoch, &mut bytes)
            .unwrap();
        assert_eq!(word(&guarded, 20), 0);
        assert_eq!(word(&guarded, 32), 1);
        memory
            .protect(range(0x1000), Permissions::READ_WRITE)
            .unwrap();
        memory
            .acknowledge_external_mappings(memory.external_mapping_epoch, 1)
            .unwrap();
        let mut bytes = tables(&memory);
        access(&mut bytes, 1, true);
        let written = memory
            .collect_external_dirty(memory.external_mapping_epoch, &mut bytes)
            .unwrap();
        assert_eq!(word(&written, 20), 1);
        assert_eq!(memory.mappings[1], mapping);
    }

    #[test]
    fn external_dirty_collect_ack_tracks_unmap_remap_and_private_detach() {
        let mut memory = AddressSpace::new(4).unwrap();
        let shared = memory.allocate_shared_pages(1, &[9]).unwrap();
        memory
            .map_shared_pages(range(0x1000), &shared, 0, Permissions::READ_WRITE, false)
            .unwrap();
        memory
            .map_shared_pages(range(0x2000), &shared, 0, Permissions::READ_WRITE, true)
            .unwrap();
        track(&mut memory);
        let mut bytes = tables(&memory);
        memory
            .collect_external_dirty(memory.external_mapping_epoch, &mut bytes)
            .unwrap();
        let shared_slot = memory.mappings[1] - 1;
        memory.external_write_fault(0x2000).unwrap();
        memory.unmap(range(0x1000)).unwrap();
        memory
            .map_zeroed(range(0x400000), Permissions::READ_WRITE)
            .unwrap();
        let count = memory
            .external_mapping_changes
            .as_ref()
            .unwrap()
            .pages
            .len() as u32;
        memory
            .acknowledge_external_mappings(memory.external_mapping_epoch, count)
            .unwrap();
        let mut bytes = tables(&memory);
        access(&mut bytes, 2, true);
        access(&mut bytes, 1024, true);
        let old_shared = memory.versions[shared_slot as usize].content;
        let result = memory
            .collect_external_dirty(memory.external_mapping_epoch, &mut bytes)
            .unwrap();
        assert_eq!(word(&result, 20), 2);
        assert_eq!(word(&result, 32), 2);
        assert_eq!(word(&result, 36), 2);
        assert_eq!(memory.versions[shared_slot as usize].content, old_shared);
        let old_slot = memory.mappings[1024] - 1;
        memory.unmap(range(0x400000)).unwrap();
        memory
            .map_zeroed(range(0x500000), Permissions::READ_WRITE)
            .unwrap();
        memory
            .map_zeroed(range(0x400000), Permissions::READ_WRITE)
            .unwrap();
        assert_ne!(
            memory.mappings[1024] - 1,
            old_slot,
            "same VA now names another physical slot"
        );
        memory
            .acknowledge_external_mappings(memory.external_mapping_epoch, 2)
            .unwrap();
        let old_version = memory.versions[old_slot as usize].content;
        let mut bytes = tables(&memory);
        access(&mut bytes, 1024, true);
        let remapped = memory
            .collect_external_dirty(memory.external_mapping_epoch, &mut bytes)
            .unwrap();
        assert_eq!(word(&remapped, 20), 1);
        assert_eq!(memory.versions[old_slot as usize].content, old_version);
    }
}
