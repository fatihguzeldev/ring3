use super::{Access, AddressSpace, GuestAddress, MemoryError, PAGE_SIZE};

pub(crate) struct DirtyPages {
    pub(crate) bits: Box<[u8]>,
    pub(crate) slots: Box<[u32]>,
    pub(crate) len: u32,
}

impl DirtyPages {
    pub(crate) fn new(pages: u32) -> Result<Self, MemoryError> {
        Ok(Self {
            bits: super::filled(pages as usize, 0)?.into_boxed_slice(),
            slots: super::filled(pages as usize, 0)?.into_boxed_slice(),
            len: 0,
        })
    }

    pub(crate) fn publish(&mut self, memory: &mut AddressSpace) -> Result<(), MemoryError> {
        if self.bits.len() != memory.capacity_pages() as usize
            || self.slots.len() != self.bits.len()
        {
            return Err(MemoryError::InvalidRange);
        }
        let slots = self
            .slots
            .get(..self.len as usize)
            .ok_or(MemoryError::InvalidRange)?;
        memory.publish_cpu_dirty(slots)?;
        for &slot in slots {
            self.bits[slot as usize] = 0;
        }
        self.len = 0;
        Ok(())
    }
}

impl AddressSpace {
    pub(crate) fn runtime_store(
        &mut self,
        dirty: &mut DirtyPages,
        address: u32,
        width: u32,
        value: u32,
    ) -> Result<bool, MemoryError> {
        if !matches!(width, 1 | 2 | 4) || dirty.bits.len() != self.capacity_pages() as usize {
            return Err(MemoryError::InvalidRange);
        }
        self.cpu_check_span(GuestAddress(address), width as usize, Access::Write)?;
        let page = address as usize / PAGE_SIZE as usize;
        let offset = address as usize & (PAGE_SIZE as usize - 1);
        let slot = self.mappings[page] as usize - 1;
        let ordinary = offset + width as usize <= PAGE_SIZE as usize
            && self.executable_views[slot] == 0
            && self
                .private_slots
                .as_ref()
                .is_none_or(|slots| slots[page] == 0);
        if ordinary {
            self.ensure_transaction_versions(1)?;
            if dirty.bits[slot] == 0 {
                let entry = dirty
                    .slots
                    .get_mut(dirty.len as usize)
                    .ok_or(MemoryError::Capacity)?;
                *entry = slot as u32;
                dirty.len += 1;
                dirty.bits[slot] = 1;
            }
            let start = slot * PAGE_SIZE as usize + offset;
            self.backing[start..start + width as usize]
                .copy_from_slice(&value.to_le_bytes()[..width as usize]);
            Ok(false)
        } else {
            dirty.publish(self)?;
            let before = self.code_validation_version();
            self.cpu_write(
                GuestAddress(address),
                &value.to_le_bytes()[..width as usize],
            )?;
            Ok(self.code_validation_version() != before)
        }
    }

    pub(crate) fn publish_cpu_dirty(&mut self, slots: &[u32]) -> Result<(), MemoryError> {
        if slots.is_empty() {
            return Ok(());
        }
        if slots.iter().any(|&slot| {
            self.references
                .get(slot as usize)
                .is_none_or(|&count| count == 0)
        }) {
            return Err(MemoryError::InvalidRange);
        }
        let version = self.advance_version()?;
        for &slot in slots {
            self.versions[slot as usize].content = version;
            if self.executable_views[slot as usize] != 0 {
                self.code_validation_version = version;
                self.code_source_version = version;
                #[cfg(feature = "external-cpu")]
                {
                    self.external_executable_epoch = version;
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::{Access, FaultReason, MemoryFault, PageRange, Permissions};

    fn range(address: u32) -> PageRange {
        PageRange::new(GuestAddress(address), 1).unwrap()
    }

    #[test]
    fn ordinary_stores_deduplicate_physical_slots_and_publish_once() {
        let mut memory = AddressSpace::new(2).unwrap();
        let pages = memory.allocate_shared_pages(1, &[0]).unwrap();
        for address in [0x1000, 0x5000] {
            memory
                .map_shared_pages(range(address), &pages, 0, Permissions::READ_WRITE, false)
                .unwrap();
        }
        let mut dirty = DirtyPages::new(2).unwrap();
        let version = memory.transaction_version();
        assert!(
            !memory
                .runtime_store(&mut dirty, 0x1000, 4, 0x12345678)
                .unwrap()
        );
        assert!(!memory.runtime_store(&mut dirty, 0x5001, 1, 0xaa).unwrap());
        assert_eq!(memory.core_read(0x1000, 4).unwrap(), 0x1234aa78);
        assert_eq!(memory.transaction_version(), version);
        assert_eq!(dirty.len, 1);
        dirty.publish(&mut memory).unwrap();
        assert_eq!(memory.transaction_version(), version + 1);
        assert_eq!(dirty.len, 0);
        dirty.publish(&mut memory).unwrap();
        assert_eq!(memory.transaction_version(), version + 1);
    }

    #[test]
    fn late_page_fault_does_not_partially_write_or_record_a_slot() {
        let mut memory = AddressSpace::new(2).unwrap();
        memory
            .map_zeroed(range(0x1000), Permissions::READ_WRITE)
            .unwrap();
        memory.write(GuestAddress(0x1ffe), &[0xaa, 0xbb]).unwrap();
        let mut dirty = DirtyPages::new(2).unwrap();
        let version = memory.transaction_version();
        assert!(matches!(
            memory.runtime_store(&mut dirty, 0x1ffe, 4, 0),
            Err(MemoryError::Fault(MemoryFault {
                address: GuestAddress(0x2000),
                access: Access::Write,
                reason: FaultReason::Unmapped
            }))
        ));
        assert_eq!(memory.core_read(0x1ffe, 2).unwrap(), 0xbbaa);
        assert_eq!(dirty.len, 0);
        assert_eq!(memory.transaction_version(), version);
    }

    #[test]
    fn executable_alias_store_invalidates_after_one_committed_write() {
        let mut memory = AddressSpace::new(2).unwrap();
        let pages = memory.allocate_shared_pages(1, &[0x90]).unwrap();
        memory
            .map_shared_pages(range(0x1000), &pages, 0, Permissions::READ_WRITE, false)
            .unwrap();
        memory
            .map_shared_pages(range(0x5000), &pages, 0, Permissions::READ_EXECUTE, false)
            .unwrap();
        let source = memory.snapshot_code(GuestAddress(0x5000), 1).unwrap();
        let version = memory.transaction_version();
        let mut dirty = DirtyPages::new(2).unwrap();
        assert!(memory.runtime_store(&mut dirty, 0x1000, 1, 0xc3).unwrap());
        assert_eq!(memory.transaction_version(), version + 1);
        assert!(!memory.is_code_current(&source));
        assert_eq!(memory.core_read(0x5000, 1).unwrap(), 0xc3);
        assert_eq!(dirty.len, 0);
    }

    #[test]
    fn cow_and_version_exhaustion_preserve_original_backing() {
        let mut memory = AddressSpace::new(3).unwrap();
        let pages = memory.allocate_shared_pages(1, &[7]).unwrap();
        memory
            .map_shared_pages(range(0x1000), &pages, 0, Permissions::READ_WRITE, false)
            .unwrap();
        memory
            .map_shared_pages(range(0x5000), &pages, 0, Permissions::READ_WRITE, true)
            .unwrap();
        let mut dirty = DirtyPages::new(3).unwrap();
        memory.runtime_store(&mut dirty, 0x5000, 1, 9).unwrap();
        assert_eq!(memory.core_read(0x1000, 1).unwrap(), 7);
        assert_eq!(memory.core_read(0x5000, 1).unwrap(), 9);
        memory.exhaust_versions_for_test();
        assert_eq!(
            memory.runtime_store(&mut dirty, 0x1000, 1, 11),
            Err(MemoryError::VersionExhausted)
        );
        assert_eq!(memory.core_read(0x1000, 1).unwrap(), 7);
    }

    #[test]
    fn malformed_dirty_shape_is_rejected_before_publication() {
        let mut memory = AddressSpace::new(2).unwrap();
        memory.map_zeroed(range(0x1000), Permissions::READ_WRITE).unwrap();
        memory.map_zeroed(range(0x5000), Permissions::READ_WRITE).unwrap();
        let version = memory.transaction_version();
        let mut dirty = DirtyPages::new(1).unwrap();
        dirty.slots[0] = 1;
        dirty.len = 1;
        assert_eq!(dirty.publish(&mut memory), Err(MemoryError::InvalidRange));
        assert_eq!(memory.transaction_version(), version);
        assert_eq!(dirty.len, 1);
    }
}
