use super::*;

const TABLE_BYTES: usize = 4096 * 1025;
const TABLES_PHYSICAL: u32 = 0x201000;
const BACKING_PHYSICAL: u32 = 16 * 1024 * 1024;
const PRIVATE_PAGE: usize = 0xff000000usize >> 12;

pub(super) struct Collector {
    occupied: Box<[u32]>,
    occupied_words: [u32; 1024],
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

#[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
fn dirty_quad(actual: &[u8; 16], expected: &[u32; 4]) -> Result<u32, MemoryError> {
    use std::arch::wasm32::*;
    let actual = i32x4(
        word(actual, 0) as i32,
        word(actual, 4) as i32,
        word(actual, 8) as i32,
        word(actual, 12) as i32,
    );
    let expected = i32x4(
        expected[0] as i32,
        expected[1] as i32,
        expected[2] as i32,
        expected[3] as i32,
    );
    let optional_global = v128_and(expected, i32x4_splat(256));
    let required = v128_and(expected, i32x4_splat(!256));
    let ignored = v128_or(i32x4_splat(96), optional_global);
    if !i32x4_all_true(i32x4_eq(v128_and(actual, v128_not(ignored)), required)) {
        return Err(MemoryError::InvalidRange);
    }
    Ok(i32x4_bitmask(i32x4_ne(v128_and(actual, i32x4_splat(64)), i32x4_splat(0))) as u32)
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
            occupied_words: [0; 1024],
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
        let global = permissions.allows(Access::Read)
            && permissions.allows(Access::Execute)
            && !permissions.allows(Access::Write)
            && !private;
        // the global bit records optional eligibility; required identity and rights stay exact.
        // slot and permission changes can keep the same mapped presence.
        self.expected[page] = if mapped {
            (BACKING_PHYSICAL + (mapping - 1) * 4096)
                | if present { 5 } else { 0 }
                | if writable { 2 } else { 0 }
                | if global { 256 } else { 0 }
        } else {
            0
        };
        let bit = 1u32 << (page % 32);
        let present = self.occupied[page / 32] & bit != 0;
        if present == mapped {
            return;
        }
        let directory = page >> 10;
        let word_bit = 1u32 << ((page / 32) % 32);
        if mapped {
            self.occupied[page / 32] |= bit;
            self.occupied_words[directory] |= word_bit;
            self.directory_counts[directory] += 1;
            self.directories[directory / 32] |= 1u32 << (directory % 32);
        } else {
            self.occupied[page / 32] &= !bit;
            if self.occupied[page / 32] == 0 {
                self.occupied_words[directory] &= !word_bit;
            }
            self.directory_counts[directory] -= 1;
            if self.directory_counts[directory] == 0 {
                self.directories[directory / 32] &= !(1u32 << (directory % 32));
            }
        }
    }

    fn observe<const TRUSTED_CLEAN: bool>(
        &mut self,
        tables: &[u8],
    ) -> Result<Observation, MemoryError> {
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
        for group in 0..self.directories.len() {
            let mut remaining = self.directories[group];
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
                let mut words = self.occupied_words[directory];
                while words != 0 {
                    let index = directory * 32 + words.trailing_zeros() as usize;
                    words &= words - 1;
                    let mut pages = self.occupied[index];
                    while pages != 0 {
                        let within = pages.trailing_zeros() as usize;
                        let page = index * 32 + within;
                        #[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
                        if !TRUSTED_CLEAN
                            && within % 4 == 0
                            && pages & (15u32 << within) == 15u32 << within
                        {
                            if page < PRIVATE_PAGE + 8 && page + 4 > PRIVATE_PAGE {
                                return Err(MemoryError::InvalidRange);
                            }
                            let offset = 4096 + page * 4;
                            let actual = tables
                                .get(offset..offset + 16)
                                .ok_or(MemoryError::InvalidRange)?;
                            let expected = self
                                .expected
                                .get(page..page + 4)
                                .ok_or(MemoryError::InvalidRange)?;
                            let mut dirty = dirty_quad(
                                actual.try_into().map_err(|_| MemoryError::InvalidRange)?,
                                expected.try_into().map_err(|_| MemoryError::InvalidRange)?,
                            )?;
                            observation.leaves += 4;
                            pages &= !(15u32 << within);
                            while dirty != 0 {
                                let lane = dirty.trailing_zeros() as usize;
                                dirty &= dirty - 1;
                                self.stage_dirty(offset + lane * 4, self.expected[page + lane]);
                            }
                            continue;
                        }
                        pages &= pages - 1;
                        observation.leaves += 1;
                        if (PRIVATE_PAGE..PRIVATE_PAGE + 8).contains(&page) {
                            return Err(MemoryError::InvalidRange);
                        }
                        let offset = 4096 + page * 4;
                        let pte = word(tables, offset);
                        if TRUSTED_CLEAN && pte & 64 == 0 {
                            continue;
                        }
                        let expected = self.expected[page];
                        if pte & !(96 | (expected & 256)) != expected & !256 {
                            return Err(MemoryError::InvalidRange);
                        }
                        if pte & 64 != 0 {
                            self.stage_dirty(offset, expected);
                        }
                    }
                }
            }
        }
        Ok(observation)
    }

    #[inline]
    fn stage_dirty(&mut self, offset: usize, expected: u32) {
        self.dirty_ptes.push(offset as u32);
        let slot = ((expected & !4095) - BACKING_PHYSICAL) >> 12;
        let bit = 1u32 << (slot % 32);
        if self.seen_slots[slot as usize / 32] & bit == 0 {
            self.seen_slots[slot as usize / 32] |= bit;
            self.slots.push(slot);
        }
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
        trusted_clean: bool,
    ) -> Result<[u8; 96], MemoryError> {
        if !tables.is_cpu_page_tables_for(self.backing.as_ptr() as usize) {
            return Err(MemoryError::InvalidRange);
        }
        self.collect_external_dirty_mode(epoch, tables, trusted_clean)
    }

    #[cfg(test)]
    pub(crate) fn collect_external_dirty(
        &mut self,
        epoch: u64,
        tables: &mut [u8],
    ) -> Result<[u8; 96], MemoryError> {
        self.collect_external_dirty_mode(epoch, tables, false)
    }

    fn collect_external_dirty_mode(
        &mut self,
        epoch: u64,
        tables: &mut [u8],
        trusted_clean: bool,
    ) -> Result<[u8; 96], MemoryError> {
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
            let observation = if trusted_clean {
                collector.observe::<true>(tables)?
            } else {
                collector.observe::<false>(tables)?
            };
            let prepublication_code = self.code_validation_version;
            // publication validates every physical slot and the next version before
            // consuming any hardware dirty/accessed evidence.
            self.publish_external_dirty(&collector.slots)?;
            collector.clear_observed(tables, &observation);
            let reset_access = observation.accessed.iter().any(|&bits| bits != 0);
            let mut bytes = [0; 96];
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
                // borrowed offsets remain valid until the next collection or release.
                (88, collector.dirty_ptes.as_ptr() as usize as u32),
                (92, collector.dirty_ptes.len() as u32),
            ] {
                put(&mut bytes, offset, value);
            }
            for (offset, value) in [
                (40, self.external_mapping_epoch),
                (48, self.version),
                (56, self.code_validation_version),
                (64, self.identity),
                (80, prepublication_code),
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
    fn external_dirty_collect_global_rx_keeps_alias_publication_and_evidence_order() {
        let mut memory = AddressSpace::new(2).unwrap();
        let shared = memory.allocate_shared_pages(1, &[7]).unwrap();
        memory
            .map_shared_pages(range(0x1000), &shared, 0, Permissions::READ_WRITE, false)
            .unwrap();
        memory
            .map_shared_pages(range(0x2000), &shared, 0, Permissions::READ_EXECUTE, false)
            .unwrap();
        track(&mut memory);
        let slot = memory.mappings[1] - 1;
        let mut bytes = tables(&memory);
        let entry = word(&bytes, pte(2)) | 256;
        put(&mut bytes, pte(2), entry);
        access(&mut bytes, 2, false);
        let version = memory.transaction_version();
        let readonly = memory
            .collect_external_dirty(memory.external_mapping_epoch, &mut bytes)
            .unwrap();
        assert_eq!(word(&readonly, 20), 0);
        assert_eq!(word(&readonly, 24), 1);
        assert_eq!(word(&readonly, 28), 1);
        assert_eq!(memory.transaction_version(), version);
        assert_eq!(word(&bytes, pte(2)), entry);
        access(&mut bytes, 1, true);
        let evidence = bytes.clone();
        memory.version = u64::MAX;
        assert_eq!(
            memory.collect_external_dirty(memory.external_mapping_epoch, &mut bytes),
            Err(MemoryError::VersionExhausted)
        );
        assert_eq!(
            bytes, evidence,
            "publication failure preserves every A/D/G bit"
        );
        memory.version = version;
        let published = memory
            .collect_external_dirty(memory.external_mapping_epoch, &mut bytes)
            .unwrap();
        assert_eq!(word(&published, 20), 1);
        assert_eq!(memory.external_dirty.as_ref().unwrap().slots, [slot]);
        assert_eq!(
            memory.versions[slot as usize].content,
            memory.transaction_version()
        );
        assert_eq!(
            memory.code_validation_version(),
            memory.transaction_version()
        );
        assert_eq!(word(&bytes, pte(1)) & 64, 0);
        assert_eq!(word(&bytes, 0) & 32, 0);
        assert_eq!(word(&bytes, pte(2)), entry);
        assert_ne!(word(&memory.external_descriptor(), 108) & 32, 0);
    }

    #[test]
    fn external_dirty_collect_global_rx_rejects_ineligible_or_malformed_ptes_atomically() {
        for (permissions, private, extra) in [
            (Permissions::READ_WRITE, false, 256),
            (Permissions::READ, false, 256),
            (Permissions::EXECUTE, false, 256),
            (Permissions::ALL, false, 256),
            (Permissions::READ_EXECUTE, true, 256),
            (Permissions::READ_EXECUTE, false, 256 | 2),
            (Permissions::READ_EXECUTE, false, 256 | 512),
        ] {
            let mut memory = AddressSpace::new(2).unwrap();
            let shared = memory.allocate_shared_pages(1, &[7]).unwrap();
            memory
                .map_shared_pages(
                    range(0x1000),
                    &shared,
                    0,
                    if private {
                        Permissions::READ_WRITE
                    } else {
                        permissions
                    },
                    private,
                )
                .unwrap();
            if private {
                memory.protect(range(0x1000), permissions).unwrap();
            }
            track(&mut memory);
            let mut bytes = tables(&memory);
            let entry = word(&bytes, pte(1)) | extra;
            put(&mut bytes, pte(1), entry);
            access(&mut bytes, 1, false);
            let evidence = bytes.clone();
            let version = memory.transaction_version();
            assert_eq!(
                memory.collect_external_dirty(memory.external_mapping_epoch, &mut bytes),
                Err(MemoryError::InvalidRange)
            );
            assert_eq!(bytes, evidence);
            assert_eq!(memory.transaction_version(), version);
        }
    }

    #[test]
    fn external_dirty_collect_global_rx_mapping_ack_refreshes_permission_eligibility() {
        let mut memory = AddressSpace::new(2).unwrap();
        memory
            .map_zeroed(range(0x1000), Permissions::READ_EXECUTE)
            .unwrap();
        track(&mut memory);
        let mut bytes = tables(&memory);
        access(&mut bytes, 1, false);
        memory
            .collect_external_dirty(memory.external_mapping_epoch, &mut bytes)
            .unwrap();
        memory
            .protect(range(0x1000), Permissions::READ_WRITE)
            .unwrap();
        memory
            .acknowledge_external_mappings(memory.external_mapping_epoch, 1)
            .unwrap();
        let mut bytes = tables(&memory);
        let entry = word(&bytes, pte(1)) | 256;
        put(&mut bytes, pte(1), entry);
        access(&mut bytes, 1, true);
        let evidence = bytes.clone();
        let version = memory.transaction_version();
        assert_eq!(
            memory.collect_external_dirty(memory.external_mapping_epoch, &mut bytes),
            Err(MemoryError::InvalidRange)
        );
        assert_eq!(bytes, evidence);
        assert_eq!(memory.transaction_version(), version);
        memory
            .protect(range(0x1000), Permissions::READ_EXECUTE)
            .unwrap();
        memory
            .acknowledge_external_mappings(memory.external_mapping_epoch, 1)
            .unwrap();
        let mut bytes = tables(&memory);
        let entry = word(&bytes, pte(1)) | 256;
        put(&mut bytes, pte(1), entry);
        access(&mut bytes, 1, false);
        assert_eq!(
            word(
                &memory
                    .collect_external_dirty(memory.external_mapping_epoch, &mut bytes)
                    .unwrap(),
                20
            ),
            0
        );
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
        assert_eq!(word(&result, 8), 96);
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
    fn external_dirty_collect_sparse_word_mask_tracks_first_last_and_permissions() {
        let mut memory = AddressSpace::new(8).unwrap();
        for page in [1, 2, 32, 1023, 1024] {
            memory
                .map_zeroed(range(page * 4096), Permissions::READ_WRITE)
                .unwrap();
        }
        track(&mut memory);
        let mut bytes = tables(&memory);
        for page in [1, 2, 32, 1023, 1024] {
            access(&mut bytes, page, true);
        }
        let first = memory
            .collect_external_dirty(memory.external_mapping_epoch, &mut bytes)
            .unwrap();
        assert_eq!(word(&first, 92), 5);
        assert_eq!(
            memory.external_dirty.as_ref().unwrap().occupied_words[0],
            0x80000003
        );
        assert_eq!(memory.external_dirty.as_ref().unwrap().occupied_words[1], 1);
        memory.unmap(range(4096)).unwrap();
        memory
            .acknowledge_external_mappings(memory.external_mapping_epoch, 1)
            .unwrap();
        assert_eq!(
            memory.external_dirty.as_ref().unwrap().occupied_words[0],
            0x80000003,
            "another page keeps the first word occupied"
        );
        memory.unmap(range(8192)).unwrap();
        memory
            .acknowledge_external_mappings(memory.external_mapping_epoch, 1)
            .unwrap();
        assert_eq!(
            memory.external_dirty.as_ref().unwrap().occupied_words[0],
            0x80000002
        );
        memory.protect(range(32 * 4096), Permissions::READ).unwrap();
        memory
            .acknowledge_external_mappings(memory.external_mapping_epoch, 1)
            .unwrap();
        assert_eq!(
            memory.external_dirty.as_ref().unwrap().occupied_words[0],
            0x80000002,
            "permission-only ACK preserves occupancy"
        );
        memory
            .map_zeroed(range(4096), Permissions::READ_WRITE)
            .unwrap();
        memory
            .acknowledge_external_mappings(memory.external_mapping_epoch, 1)
            .unwrap();
        assert_eq!(
            memory.external_dirty.as_ref().unwrap().occupied_words[0],
            0x80000003
        );
        bytes = tables(&memory);
        for page in [1, 1023, 1024] {
            access(&mut bytes, page, true);
        }
        access(&mut bytes, 32, false);
        let at = pte(1023);
        let original = word(&bytes, at);
        put(&mut bytes, at, original ^ 2);
        let evidence = bytes.clone();
        let version = memory.transaction_version();
        assert_eq!(
            memory.collect_external_dirty(memory.external_mapping_epoch, &mut bytes),
            Err(MemoryError::InvalidRange)
        );
        assert_eq!(bytes, evidence);
        assert_eq!(memory.transaction_version(), version);
        put(&mut bytes, at, original);
        let retried = memory
            .collect_external_dirty(memory.external_mapping_epoch, &mut bytes)
            .unwrap();
        assert_eq!(word(&retried, 32), 4);
        assert_eq!(word(&retried, 92), 3);
        assert_eq!(
            memory.external_dirty.as_ref().unwrap().dirty_ptes,
            [pte(1) as u32, pte(1023) as u32, pte(1024) as u32]
        );
    }

    #[test]
    fn external_dirty_collect_reports_every_dirty_alias_until_next_collection() {
        let mut memory = AddressSpace::new(3).unwrap();
        let shared = memory.allocate_shared_pages(1, &[7]).unwrap();
        for address in [0x1000, 0x400000] {
            memory
                .map_shared_pages(range(address), &shared, 0, Permissions::READ_WRITE, false)
                .unwrap();
        }
        memory
            .map_zeroed(range(0x2000), Permissions::READ_WRITE)
            .unwrap();
        track(&mut memory);
        let mut bytes = tables(&memory);
        access(&mut bytes, 1, true);
        access(&mut bytes, 1024, true);
        access(&mut bytes, 2, false);
        let first = memory
            .collect_external_dirty(memory.external_mapping_epoch, &mut bytes)
            .unwrap();
        assert_eq!(first.len(), 96);
        assert_eq!(word(&first, 8), 96);
        assert_eq!(word(&first, 20), 1, "physical aliases still publish once");
        assert_eq!(word(&first, 92), 2, "every cleared D bit needs its own VA");
        let collector = memory.external_dirty.as_ref().unwrap();
        assert_eq!(
            word(&first, 88),
            collector.dirty_ptes.as_ptr() as usize as u32
        );
        assert_eq!(collector.dirty_ptes, [pte(1) as u32, pte(1024) as u32]);
        assert_eq!(word(&bytes, pte(1)) & 64, 0);
        assert_eq!(word(&bytes, pte(1024)) & 64, 0);
        assert_eq!(word(&bytes, 0) & 32, 0);
        assert_eq!(word(&bytes, 4) & 32, 0);
        assert_ne!(word(&memory.external_descriptor(), 108) & 128, 0);
        assert_eq!(
            memory.external_dirty.as_ref().unwrap().dirty_ptes,
            [pte(1) as u32, pte(1024) as u32],
            "a descriptor read does not consume the borrowed list"
        );
        access(&mut bytes, 1024, true);
        let rearmed = memory
            .collect_external_dirty(memory.external_mapping_epoch, &mut bytes)
            .unwrap();
        assert_eq!(word(&rearmed, 88), word(&first, 88));
        assert_eq!(word(&rearmed, 92), 1);
        assert_eq!(
            memory.external_dirty.as_ref().unwrap().dirty_ptes,
            [pte(1024) as u32]
        );
        access(&mut bytes, 2, false);
        let readonly = memory
            .collect_external_dirty(memory.external_mapping_epoch, &mut bytes)
            .unwrap();
        assert_eq!(word(&readonly, 88), word(&first, 88));
        assert_eq!(
            word(&readonly, 92),
            0,
            "read-only evidence needs no VA invalidation"
        );
        assert_eq!(word(&readonly, 24), 1);
        assert_eq!(
            word(&readonly, 28),
            1,
            "legacy blanket-flush contract is unchanged"
        );
        assert_eq!(word(&readonly, 20), 0);
    }

    #[test]
    fn external_dirty_collect_trusted_clean_retains_read_and_write_rearming() {
        let mut memory = AddressSpace::new(2).unwrap();
        memory
            .map_zeroed(range(0x1000), Permissions::READ_WRITE)
            .unwrap();
        memory.map_zeroed(range(0x2000), Permissions::READ).unwrap();
        track(&mut memory);
        let mut bytes = tables(&memory);
        for dirty in [false, true, false, true] {
            access(&mut bytes, 1, dirty);
            access(&mut bytes, 2, false);
            let before = memory.transaction_version();
            let result = memory
                .collect_external_dirty_mode(memory.external_mapping_epoch, &mut bytes, true)
                .unwrap();
            assert_eq!(word(&result, 20), u32::from(dirty));
            assert_eq!(word(&result, 24), 1);
            assert_eq!(word(&result, 28), 1);
            assert_eq!(word(&result, 32), 2);
            assert_eq!(word(&result, 36), 1);
            assert_eq!(word(&bytes, 0) & 32, 0);
            assert_eq!(word(&bytes, pte(1)) & 64, 0);
            assert_eq!(memory.transaction_version() > before, dirty);
        }
        access(&mut bytes, 2, false);
        put(&mut bytes, pte(2), word(&tables(&memory), pte(2)) ^ 2);
        assert_eq!(
            memory.collect_external_dirty(memory.external_mapping_epoch, &mut bytes),
            Err(MemoryError::InvalidRange)
        );
        let result = memory
            .collect_external_dirty_mode(memory.external_mapping_epoch, &mut bytes, true)
            .unwrap();
        assert_eq!(word(&result, 20), 0);
        assert_eq!(word(&result, 28), 1);
    }

    #[test]
    fn external_dirty_collect_trusted_dirty_corruption_is_atomic_and_retryable() {
        let mut memory = AddressSpace::new(2).unwrap();
        for page in [1, 2] {
            memory
                .map_zeroed(range(page * 4096), Permissions::READ_WRITE)
                .unwrap();
        }
        track(&mut memory);
        let mut bytes = tables(&memory);
        access(&mut bytes, 1, true);
        access(&mut bytes, 2, true);
        let original = word(&bytes, pte(2));
        put(&mut bytes, pte(2), original ^ 4096);
        let evidence = bytes.clone();
        let version = memory.transaction_version();
        assert_eq!(
            memory.collect_external_dirty_mode(memory.external_mapping_epoch, &mut bytes, true),
            Err(MemoryError::InvalidRange)
        );
        assert_eq!(bytes, evidence);
        assert_eq!(memory.transaction_version(), version);
        put(&mut bytes, pte(2), original);
        let result = memory
            .collect_external_dirty_mode(memory.external_mapping_epoch, &mut bytes, true)
            .unwrap();
        assert_eq!(word(&result, 20), 2);
        assert_eq!(word(&result, 92), 2);
        for page in [1, 2] {
            assert_eq!(word(&bytes, pte(page)) & 64, 0);
        }
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
    fn external_dirty_collect_reports_prepublication_code_epoch() {
        let mut memory = AddressSpace::new(3).unwrap();
        memory.map_zeroed(range(0x1000), Permissions::ALL).unwrap();
        memory.map_zeroed(range(0x2000), Permissions::ALL).unwrap();
        track(&mut memory);
        let old_import = memory.code_validation_version();
        memory.write(GuestAddress(0x2000), &[9]).unwrap();
        let host_changed = memory.code_validation_version();
        assert_ne!(host_changed, old_import);
        let mut bytes = tables(&memory);
        access(&mut bytes, 1, true);
        let result = memory
            .collect_external_dirty(memory.external_mapping_epoch, &mut bytes)
            .unwrap();
        assert_eq!(result.len(), 96);
        assert_eq!(word(&result, 8), 96);
        let before = u64::from_le_bytes(result.get(80..88).unwrap().try_into().unwrap());
        let after = u64::from_le_bytes(result[56..64].try_into().unwrap());
        assert_eq!(
            before, host_changed,
            "the stamp includes unseen host writes before collection"
        );
        assert!(
            after > before,
            "guest dirty publication has its own later stamp"
        );
        assert_eq!(after, memory.transaction_version());
        assert_ne!(word(&memory.external_descriptor(), 108) & 4, 0);
        let readonly = memory
            .collect_external_dirty(memory.external_mapping_epoch, &mut bytes)
            .unwrap();
        assert_eq!(
            u64::from_le_bytes(readonly.get(80..88).unwrap().try_into().unwrap()),
            after
        );
        assert_eq!(
            u64::from_le_bytes(readonly[56..64].try_into().unwrap()),
            after
        );
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
    fn external_dirty_collect_dense_quads_preserve_sparse_counts_and_alias_deduplication() {
        let mut memory = AddressSpace::new(6).unwrap();
        let shared = memory.allocate_shared_pages(1, &[7]).unwrap();
        for (page, permissions) in [
            (4, Permissions::READ_WRITE),
            (5, Permissions::READ_EXECUTE),
            (9, Permissions::READ_WRITE),
        ] {
            memory
                .map_shared_pages(range(page * 4096), &shared, 0, permissions, false)
                .unwrap();
        }
        for (page, permissions) in [
            (6, Permissions::NONE),
            (7, Permissions::READ_WRITE),
            (1024, Permissions::READ_WRITE),
        ] {
            memory.map_zeroed(range(page * 4096), permissions).unwrap();
        }
        track(&mut memory);
        let mut bytes = tables(&memory);
        for page in [4, 5, 7, 9] {
            access(&mut bytes, page, true);
        }
        for page in [6, 1024] {
            access(&mut bytes, page, false);
        }
        let result = memory
            .collect_external_dirty(memory.external_mapping_epoch, &mut bytes)
            .unwrap();
        assert_eq!(
            word(&result, 20),
            2,
            "three dirty aliases still publish one physical slot"
        );
        assert_eq!(
            word(&result, 32),
            6,
            "dense and partial groups count occupied leaves only"
        );
        assert_eq!(word(&result, 36), 2);
        assert_eq!(word(&result, 24), 1);
        assert_eq!(word(&result, 28), 1);
        for page in [4, 5, 6, 7, 9, 1024] {
            assert_eq!(word(&bytes, pte(page)) & 64, 0);
        }
        assert_eq!(word(&bytes, 0) & 32, 0);
        assert_eq!(word(&bytes, 4) & 32, 0);
    }

    #[test]
    fn external_dirty_collect_dense_quad_validates_every_lane_before_publication() {
        let mut memory = AddressSpace::new(4).unwrap();
        for page in 4..8 {
            memory
                .map_zeroed(range(page * 4096), Permissions::READ_WRITE)
                .unwrap();
        }
        track(&mut memory);
        for dirty in [false, true] {
            for lane in 0..4 {
                let mut bytes = tables(&memory);
                for page in 4..8 {
                    access(&mut bytes, page, dirty);
                }
                let offset = pte(4 + lane);
                let original = word(&bytes, offset);
                put(&mut bytes, offset, original ^ 2);
                let before = bytes.clone();
                let version = memory.transaction_version();
                assert_eq!(
                    memory.collect_external_dirty(memory.external_mapping_epoch, &mut bytes),
                    Err(MemoryError::InvalidRange),
                    "dirty={dirty} lane={lane}"
                );
                assert_eq!(
                    bytes, before,
                    "malformed read-only and dirty leaves preserve all A/D evidence"
                );
                assert_eq!(memory.transaction_version(), version);
                put(&mut bytes, offset, original);
                let retry = memory
                    .collect_external_dirty(memory.external_mapping_epoch, &mut bytes)
                    .unwrap();
                assert_eq!(word(&retry, 20), if dirty { 4 } else { 0 });
                assert_eq!(word(&retry, 32), 4);
                assert_eq!(word(&retry, 36), 1);
                assert_eq!(word(&retry, 28), 1);
                for page in 4..8 {
                    assert_eq!(word(&bytes, pte(page)) & 64, 0);
                }
                assert_eq!(word(&bytes, 0) & 32, 0);
            }
        }
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
