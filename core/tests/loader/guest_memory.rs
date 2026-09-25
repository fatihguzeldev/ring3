use ring3_core::execution::{Access, GuestMemory, MemoryError, PAGE_SIZE, Permissions};

#[test]
fn sparse_pages_are_zeroed_and_support_cross_page_access() {
    let mut memory = GuestMemory::new(3);
    memory
        .map_zeroed(0x1_0000_0000, 2 * PAGE_SIZE, Permissions::READ_WRITE)
        .unwrap();
    let mut bytes = [1; 8];
    memory.read(0x1_0000_0ffc, &mut bytes).unwrap();
    assert_eq!(bytes, [0; 8]);
    memory
        .write(0x1_0000_0ffc, &[1, 2, 3, 4, 5, 6, 7, 8])
        .unwrap();
    memory.read(0x1_0000_0ffc, &mut bytes).unwrap();
    assert_eq!(bytes, [1, 2, 3, 4, 5, 6, 7, 8]);
    assert_eq!(memory.mapped_pages(), 2);
}

#[test]
fn permissions_are_independent_and_failed_writes_are_atomic() {
    let mut memory = GuestMemory::new(2);
    memory
        .map_zeroed(0x1000, 2 * PAGE_SIZE, Permissions::READ_WRITE)
        .unwrap();
    memory
        .protect(0x2000, PAGE_SIZE, Permissions::READ_EXECUTE)
        .unwrap();
    assert_eq!(
        memory.write(0x1ffe, &[1, 2, 3, 4]),
        Err(MemoryError::PermissionDenied {
            address: 0x2000,
            access: Access::Write
        })
    );
    let mut bytes = [9; 2];
    memory.read(0x1ffe, &mut bytes).unwrap();
    assert_eq!(bytes, [0, 0]);
    memory.fetch(0x2000, &mut bytes).unwrap();
    assert_eq!(
        memory.fetch(0x1000, &mut bytes),
        Err(MemoryError::PermissionDenied {
            address: 0x1000,
            access: Access::Execute
        })
    );
}

#[test]
fn rejected_maps_and_protection_changes_leave_existing_pages_unchanged() {
    let mut memory = GuestMemory::new(3);
    memory
        .map_zeroed(0x2000, PAGE_SIZE, Permissions::READ_WRITE)
        .unwrap();
    assert_eq!(
        memory.map_zeroed(0x1000, 2 * PAGE_SIZE, Permissions::READ),
        Err(MemoryError::AlreadyMapped { address: 0x2000 })
    );
    assert_eq!(memory.mapped_pages(), 1);
    assert_eq!(
        memory.protect(0x2000, 2 * PAGE_SIZE, Permissions::READ),
        Err(MemoryError::Unmapped { address: 0x3000 })
    );
    memory.write(0x2000, &[7]).unwrap();
    assert_eq!(
        memory.map_zeroed(0x4000, 3 * PAGE_SIZE, Permissions::READ),
        Err(MemoryError::PageLimitExceeded)
    );
}

#[test]
fn bounds_and_alignment_are_explicit_errors() {
    let mut memory = GuestMemory::new(1);
    assert_eq!(
        memory.map_zeroed(1, PAGE_SIZE, Permissions::READ),
        Err(MemoryError::UnalignedRange)
    );
    assert_eq!(
        memory.map_zeroed(0, 0, Permissions::READ),
        Err(MemoryError::UnalignedRange)
    );
    assert_eq!(
        memory.map_zeroed(u64::MAX - PAGE_SIZE + 1, PAGE_SIZE, Permissions::READ),
        Err(MemoryError::AddressOverflow)
    );
    assert_eq!(
        memory.read(u64::MAX, &mut [0; 2]),
        Err(MemoryError::AddressOverflow)
    );
    assert_eq!(
        memory.read(0, &mut [0]),
        Err(MemoryError::Unmapped { address: 0 })
    );
    memory.read(u64::MAX, &mut []).unwrap();
}
