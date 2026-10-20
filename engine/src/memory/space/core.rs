use super::{AddressSpace, GuestAddress, MemoryError};
use crate::memory::{Access, FaultReason, MemoryFault, PagingBacking32, PagingCheckError32};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CoreMemoryView32 {
    pub mappings: usize,
    pub permissions: usize,
    pub backing: usize,
    pub backing_pages: u32,
    pub executable_views: usize,
    pub private_slots: usize,
    pub transaction_version: usize,
    pub page_versions: usize,
    pub code_validation_version: usize,
    pub expected_code_version: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CoreWriteOutcome {
    Complete,
    CodeInvalidated,
}

impl AddressSpace {
    // raw locations are valid only while the owner retains an exclusive synchronous cpu lifetime.
    pub(crate) fn core_view(&mut self) -> CoreMemoryView32 {
        CoreMemoryView32 {
            mappings: self.mappings.as_ptr() as usize,
            permissions: self.permissions.as_ptr() as usize,
            backing: self.backing.as_mut_ptr() as usize,
            backing_pages: self.capacity_pages(),
            executable_views: self.executable_views.as_ptr() as usize,
            private_slots: self
                .private_slots
                .as_ref()
                .map_or(0, |slots| slots.as_ptr() as usize),
            transaction_version: std::ptr::from_mut(&mut self.version) as usize,
            page_versions: self.versions.as_mut_ptr() as usize,
            code_validation_version: std::ptr::from_ref(&self.code_validation_version) as usize,
            expected_code_version: self.code_validation_version,
        }
    }

    pub(crate) fn core_read(&self, address: u32, width: u32) -> Result<u32, MemoryError> {
        let bytes = width & 0xff;
        if width & !0x1ff != 0 || !matches!(bytes, 1 | 2 | 4) {
            return Err(MemoryError::InvalidRange);
        }
        if width & 0x100 != 0 {
            self.cpu_paging_check32(
                GuestAddress(address),
                bytes as usize,
                Access::Write,
                PagingBacking32::Modify,
            )
            .map_err(|error| match error {
                PagingCheckError32::Memory(error) => error,
                PagingCheckError32::Fault(facts) => {
                    if self.guard_page(facts.linear()).is_some() {
                        self.record_guard_service_fault(facts.linear());
                    }
                    MemoryError::Fault(MemoryFault {
                        address: facts.linear(),
                        access: facts.access(),
                        reason: if facts.present() {
                            FaultReason::Permission
                        } else {
                            FaultReason::Unmapped
                        },
                    })
                }
            })?;
        }
        let mut value = [0; 4];
        self.cpu_read(GuestAddress(address), &mut value[..bytes as usize])?;
        Ok(u32::from_le_bytes(value))
    }

    pub(crate) fn core_write(
        &mut self,
        address: u32,
        width: u32,
        value: u32,
    ) -> Result<CoreWriteOutcome, MemoryError> {
        if !matches!(width, 1 | 2 | 4) {
            return Err(MemoryError::InvalidRange);
        }
        let before = self.code_validation_version;
        self.cpu_write(
            GuestAddress(address),
            &value.to_le_bytes()[..width as usize],
        )?;
        Ok(if self.code_validation_version == before {
            CoreWriteOutcome::Complete
        } else {
            CoreWriteOutcome::CodeInvalidated
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::*;

    fn range(address: u32) -> PageRange {
        PageRange::new(GuestAddress(address), 1).unwrap()
    }

    #[test]
    fn view_uses_the_canonical_allocations_without_changing_versions() {
        let mut memory = AddressSpace::new(3).unwrap();
        memory
            .map_zeroed(range(0x1000), Permissions::READ_WRITE)
            .unwrap();
        let version = memory.transaction_version();
        let view = memory.core_view();
        assert_eq!(view.mappings, memory.mappings.as_ptr() as usize);
        assert_eq!(view.permissions, memory.permissions.as_ptr() as usize);
        assert_eq!(view.backing, memory.backing.as_ptr() as usize);
        assert_eq!(view.backing_pages, 3);
        assert_eq!(
            view.executable_views,
            memory.executable_views.as_ptr() as usize
        );
        assert_eq!(view.private_slots, 0);
        assert_eq!(
            view.transaction_version,
            std::ptr::from_ref(&memory.version) as usize
        );
        assert_eq!(view.page_versions, memory.versions.as_ptr() as usize);
        assert_eq!(
            view.code_validation_version,
            std::ptr::from_ref(&memory.code_validation_version) as usize
        );
        assert_eq!(view.expected_code_version, memory.code_validation_version());
        assert_eq!(std::mem::size_of::<PageVersion>(), 16);
        assert_eq!(std::mem::offset_of!(PageVersion, content), 8);
        assert_eq!(memory.transaction_version(), version);
    }

    #[test]
    fn scalar_accesses_publish_canonical_data_and_versions() {
        let mut memory = AddressSpace::new(2).unwrap();
        memory
            .map_zeroed(range(0x1000), Permissions::READ_WRITE)
            .unwrap();
        let before = memory.transaction_version();
        let code_before = memory.code_validation_version();
        assert_eq!(
            memory.core_write(0x1003, 4, 0x8765_4321).unwrap(),
            super::CoreWriteOutcome::Complete
        );
        assert_eq!(memory.core_read(0x1003, 1).unwrap(), 0x21);
        assert_eq!(memory.core_read(0x1003, 2).unwrap(), 0x4321);
        assert_eq!(memory.core_read(0x1003, 4).unwrap(), 0x8765_4321);
        assert_eq!(memory.transaction_version(), before + 1);
        let slot = memory.mappings[1] as usize - 1;
        assert_eq!(memory.versions[slot].content, before + 1);
        assert_eq!(memory.code_validation_version(), code_before);
        assert_eq!(
            memory.core_write(0x1003, 3, 0),
            Err(MemoryError::InvalidRange)
        );
        assert_eq!(memory.core_read(0x1003, 3), Err(MemoryError::InvalidRange));
        assert_eq!(memory.core_read(0x1003, 4).unwrap(), 0x8765_4321);
        assert_eq!(memory.transaction_version(), before + 1);
    }

    #[test]
    fn crossing_a_faulting_page_never_publishes_a_partial_store() {
        let mut memory = AddressSpace::new(2).unwrap();
        memory
            .map_zeroed(range(0x1000), Permissions::READ_WRITE)
            .unwrap();
        memory.write(GuestAddress(0x1ffe), &[0xaa, 0xbb]).unwrap();
        let before = memory.transaction_version();
        assert!(matches!(
            memory.core_write(0x1ffe, 4, 0x1234_5678),
            Err(MemoryError::Fault(MemoryFault {
                address: GuestAddress(0x2000),
                access: Access::Write,
                reason: FaultReason::Unmapped
            }))
        ));
        assert_eq!(memory.core_read(0x1ffe, 2).unwrap(), 0xbbaa);
        assert_eq!(memory.transaction_version(), before);
        memory.protect(range(0x1000), Permissions::READ).unwrap();
        assert!(matches!(
            memory.core_write(0x1000, 1, 9),
            Err(MemoryError::Fault(MemoryFault {
                access: Access::Write,
                reason: FaultReason::Permission,
                ..
            }))
        ));
    }

    #[test]
    fn modify_checks_both_rights_in_page_order_before_reading() {
        let mut memory = AddressSpace::new(2).unwrap();
        memory.map_zeroed(range(0x1000), Permissions::READ).unwrap();
        assert_eq!(
            memory.core_read(0x1ffe, 0x104),
            Err(MemoryError::Fault(MemoryFault {
                address: GuestAddress(0x1ffe),
                access: Access::Write,
                reason: FaultReason::Permission,
            }))
        );
        assert_eq!(
            memory.core_read(0x1ffe, 4),
            Err(MemoryError::Fault(MemoryFault {
                address: GuestAddress(0x2000),
                access: Access::Read,
                reason: FaultReason::Unmapped,
            }))
        );
        memory
            .protect(range(0x1000), Permissions::READ_WRITE)
            .unwrap();
        memory.core_write(0x1000, 4, 0x1234_5678).unwrap();
        assert_eq!(memory.core_read(0x1000, 0x104).unwrap(), 0x1234_5678);
        memory
            .protect(range(0x1000), Permissions::from_bits(2).unwrap())
            .unwrap();
        assert_eq!(
            memory.core_read(0x1000, 0x104),
            Err(MemoryError::Fault(MemoryFault {
                address: GuestAddress(0x1000),
                access: Access::Write,
                reason: FaultReason::Permission,
            }))
        );
        assert_eq!(
            memory.core_read(0x1000, 0x204),
            Err(MemoryError::InvalidRange)
        );
    }

    #[test]
    fn scalar_spans_wrap_at_the_guest_address_limit() {
        let mut memory = AddressSpace::new(2).unwrap();
        memory
            .map_zeroed(range(0xffff_f000), Permissions::READ_WRITE)
            .unwrap();
        memory
            .map_zeroed(range(0), Permissions::READ_WRITE)
            .unwrap();
        assert_eq!(
            memory.core_write(0xffff_fffe, 4, 0x8765_4321).unwrap(),
            super::CoreWriteOutcome::Complete
        );
        assert_eq!(memory.core_read(0xffff_fffe, 4).unwrap(), 0x8765_4321);
        assert_eq!(memory.core_read(0xffff_fffe, 2).unwrap(), 0x4321);
        assert_eq!(memory.core_read(0, 2).unwrap(), 0x8765);
    }

    #[test]
    fn modify_fault_retains_the_canonical_guard_service_witness() {
        let mut memory = AddressSpace::new(1).unwrap();
        memory
            .map_zeroed(range(0x1000), Permissions::READ_WRITE.with_guard())
            .unwrap();
        let version = memory.transaction_version();
        memory.begin_guard_service().unwrap();
        assert_eq!(
            memory.core_read(0x1234, 0x104),
            Err(MemoryError::Fault(MemoryFault {
                address: GuestAddress(0x1234),
                access: Access::Write,
                reason: FaultReason::Permission,
            }))
        );
        let guard = memory.end_guard_service().unwrap();
        assert_eq!(guard.address(), GuestAddress(0x1000));
        assert!(guard.is_current(&memory));
        assert_eq!(memory.transaction_version(), version);
    }

    #[test]
    fn copy_on_write_keeps_the_shared_backing_unchanged() {
        let mut memory = AddressSpace::new(3).unwrap();
        let pages = memory.allocate_shared_pages(1, &[7]).unwrap();
        memory
            .map_shared_pages(range(0x1000), &pages, 0, Permissions::READ_WRITE, false)
            .unwrap();
        memory
            .map_shared_pages(range(0x5000), &pages, 0, Permissions::READ_WRITE, true)
            .unwrap();
        assert_ne!(memory.core_view().private_slots, 0);
        assert_eq!(
            memory.core_write(0x5000, 1, 9).unwrap(),
            super::CoreWriteOutcome::Complete
        );
        assert_eq!(memory.core_read(0x1000, 1).unwrap(), 7);
        assert_eq!(memory.core_read(0x5000, 1).unwrap(), 9);
    }

    #[test]
    fn a_nonexecutable_alias_still_reports_code_invalidation() {
        let mut memory = AddressSpace::new(1).unwrap();
        let pages = memory.allocate_shared_pages(1, &[0x90]).unwrap();
        memory
            .map_shared_pages(range(0x1000), &pages, 0, Permissions::READ_WRITE, false)
            .unwrap();
        memory
            .map_shared_pages(range(0x3000), &pages, 0, Permissions::READ_EXECUTE, false)
            .unwrap();
        let code = memory.snapshot_code(GuestAddress(0x3000), 1).unwrap();
        assert_eq!(
            memory.core_write(0x1000, 1, 0xc3).unwrap(),
            super::CoreWriteOutcome::CodeInvalidated
        );
        assert!(!memory.is_code_current(&code));
        let mut byte = [0];
        memory.fetch(GuestAddress(0x3000), &mut byte).unwrap();
        assert_eq!(byte, [0xc3]);
    }

    #[test]
    fn version_exhaustion_is_neutral_before_store() {
        let mut memory = AddressSpace::new(1).unwrap();
        memory
            .map_zeroed(range(0x1000), Permissions::READ_WRITE)
            .unwrap();
        memory.exhaust_versions_for_test();
        assert_eq!(
            memory.core_write(0x1000, 4, u32::MAX),
            Err(MemoryError::VersionExhausted)
        );
        assert_eq!(memory.core_read(0x1000, 4).unwrap(), 0);
    }
}
