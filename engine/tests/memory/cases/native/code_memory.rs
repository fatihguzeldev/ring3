use ring3_engine::memory::{
    Access, AddressSpace, FaultReason, GuestAddress, MemoryError, PageRange, Permissions,
};

fn range(address: u32, pages: u32) -> PageRange {
    PageRange::new(GuestAddress(address), pages).unwrap()
}

fn assert_fault<T>(
    result: Result<T, MemoryError>,
    address: u32,
    access: Access,
    reason: FaultReason,
) {
    let error = match result {
        Err(error) => error,
        Ok(_) => panic!("expected guest memory fault"),
    };
    let MemoryError::Fault(fault) = error else {
        panic!("expected guest memory fault, got {error:?}");
    };
    assert_eq!(fault.address.0, address);
    assert_eq!(fault.access, access);
    assert_eq!(fault.reason, reason);
}

#[test]
fn snapshot_requires_nonempty_executable_memory() {
    let mut space = AddressSpace::new(1).unwrap();
    space
        .map_zeroed(range(0x1000, 1), Permissions::READ_WRITE)
        .unwrap();
    assert!(matches!(
        space.snapshot_code(GuestAddress(0x1000), 0),
        Err(MemoryError::InvalidRange)
    ));
    assert!(matches!(
        space.snapshot_code(GuestAddress(u32::MAX), 0),
        Err(MemoryError::InvalidRange)
    ));
    assert_fault(
        space.snapshot_code(GuestAddress(0x1017), 1),
        0x1017,
        Access::Execute,
        FaultReason::Permission,
    );
    space
        .protect(range(0x1000, 1), Permissions::READ_EXECUTE)
        .unwrap();
    let snapshot = space.snapshot_code(GuestAddress(0x1017), 1).unwrap();
    assert!(space.is_code_current(&snapshot));
}

#[test]
fn snapshot_reports_first_unmapped_or_nonexecutable_byte() {
    let mut space = AddressSpace::new(2).unwrap();
    space
        .map_zeroed(range(0x1000, 1), Permissions::ALL)
        .unwrap();
    assert_fault(
        space.snapshot_code(GuestAddress(0x1ffe), 4),
        0x2000,
        Access::Execute,
        FaultReason::Unmapped,
    );
    space
        .map_zeroed(range(0x2000, 1), Permissions::READ_WRITE)
        .unwrap();
    assert_fault(
        space.snapshot_code(GuestAddress(0x1ffe), 4),
        0x2000,
        Access::Execute,
        FaultReason::Permission,
    );
    assert_fault(
        space.snapshot_code(GuestAddress(0x3017), 1),
        0x3017,
        Access::Execute,
        FaultReason::Unmapped,
    );
}

#[test]
fn final_byte_snapshot_is_valid_but_overflow_faults_at_start() {
    let mut space = AddressSpace::new(1).unwrap();
    space
        .map_zeroed(range(0xffff_f000, 1), Permissions::ALL)
        .unwrap();
    let snapshot = space.snapshot_code(GuestAddress(u32::MAX), 1).unwrap();
    assert!(space.is_code_current(&snapshot));
    assert_fault(
        space.snapshot_code(GuestAddress(u32::MAX), 2),
        u32::MAX,
        Access::Execute,
        FaultReason::AddressOverflow,
    );
    assert!(space.is_code_current(&snapshot));
}

#[test]
fn write_anywhere_on_a_covered_page_invalidates_snapshot() {
    let mut space = AddressSpace::new(1).unwrap();
    space
        .map_zeroed(range(0x1000, 1), Permissions::ALL)
        .unwrap();
    let snapshot = space.snapshot_code(GuestAddress(0x1800), 4).unwrap();
    space.write(GuestAddress(0x1fff), &[0x90]).unwrap();
    assert!(!space.is_code_current(&snapshot));
    let replacement = space.snapshot_code(GuestAddress(0x1800), 4).unwrap();
    assert!(space.is_code_current(&replacement));
    assert!(!space.is_code_current(&snapshot));
}

#[test]
fn writing_identical_bytes_invalidates_snapshot() {
    let mut space = AddressSpace::new(1).unwrap();
    space
        .map_zeroed(range(0x1000, 1), Permissions::ALL)
        .unwrap();
    space.write(GuestAddress(0x1800), &[0x90, 0xc3]).unwrap();
    let snapshot = space.snapshot_code(GuestAddress(0x1800), 2).unwrap();
    space.write(GuestAddress(0x1800), &[0x90, 0xc3]).unwrap();
    assert!(!space.is_code_current(&snapshot));
}

#[test]
fn each_page_in_a_cross_page_snapshot_contributes_to_validity() {
    for changed_address in [0x1000, 0x2fff] {
        let mut space = AddressSpace::new(2).unwrap();
        space
            .map_zeroed(range(0x1000, 2), Permissions::ALL)
            .unwrap();
        let snapshot = space.snapshot_code(GuestAddress(0x1fff), 2).unwrap();
        assert!(space.is_code_current(&snapshot));
        space.write(GuestAddress(changed_address), &[0x90]).unwrap();
        assert!(!space.is_code_current(&snapshot));
    }
}

#[test]
fn setting_same_permissions_invalidates_snapshot() {
    let mut space = AddressSpace::new(1).unwrap();
    space
        .map_zeroed(range(0x1000, 1), Permissions::READ_EXECUTE)
        .unwrap();
    let snapshot = space.snapshot_code(GuestAddress(0x1800), 1).unwrap();
    space
        .protect(range(0x1000, 1), Permissions::READ_EXECUTE)
        .unwrap();
    assert!(!space.is_code_current(&snapshot));
}

#[test]
fn temporarily_removing_execute_permission_never_revives_snapshot() {
    let mut space = AddressSpace::new(1).unwrap();
    space
        .map_zeroed(range(0x1000, 1), Permissions::ALL)
        .unwrap();
    let snapshot = space.snapshot_code(GuestAddress(0x1800), 1).unwrap();
    space
        .protect(range(0x1000, 1), Permissions::READ_WRITE)
        .unwrap();
    assert!(!space.is_code_current(&snapshot));
    space.protect(range(0x1000, 1), Permissions::ALL).unwrap();
    assert!(!space.is_code_current(&snapshot));
}

#[test]
fn writing_while_nonexecutable_then_restoring_execute_never_revives_snapshot() {
    let mut space = AddressSpace::new(1).unwrap();
    space
        .map_zeroed(range(0x1000, 1), Permissions::ALL)
        .unwrap();
    space.write(GuestAddress(0x1800), &[0x90]).unwrap();
    let snapshot = space.snapshot_code(GuestAddress(0x1800), 1).unwrap();
    space
        .protect(range(0x1000, 1), Permissions::READ_WRITE)
        .unwrap();
    space.write(GuestAddress(0x1800), &[0xc3]).unwrap();
    space.write(GuestAddress(0x1800), &[0x90]).unwrap();
    space.protect(range(0x1000, 1), Permissions::ALL).unwrap();
    assert!(!space.is_code_current(&snapshot));
}

#[test]
fn unmapping_and_remapping_same_address_and_bytes_never_revives_snapshot() {
    let mut space = AddressSpace::new(1).unwrap();
    space
        .map_zeroed(range(0x1000, 1), Permissions::ALL)
        .unwrap();
    space.write(GuestAddress(0x1800), &[0x90]).unwrap();
    let snapshot = space.snapshot_code(GuestAddress(0x1800), 1).unwrap();
    space.unmap(range(0x1000, 1)).unwrap();
    assert!(!space.is_code_current(&snapshot));
    space
        .map_zeroed(range(0x1000, 1), Permissions::ALL)
        .unwrap();
    space.write(GuestAddress(0x1800), &[0x90]).unwrap();
    assert!(!space.is_code_current(&snapshot));
    let replacement = space.snapshot_code(GuestAddress(0x1800), 1).unwrap();
    assert!(space.is_code_current(&replacement));
}

#[test]
fn reusing_a_backing_slot_for_another_guest_page_never_revives_snapshot() {
    let mut space = AddressSpace::new(1).unwrap();
    space
        .map_zeroed(range(0x1000, 1), Permissions::ALL)
        .unwrap();
    let snapshot = space.snapshot_code(GuestAddress(0x1800), 1).unwrap();
    space.unmap(range(0x1000, 1)).unwrap();
    space
        .map_zeroed(range(0x9000, 1), Permissions::ALL)
        .unwrap();
    assert!(!space.is_code_current(&snapshot));
    space.unmap(range(0x9000, 1)).unwrap();
    space
        .map_zeroed(range(0x1000, 1), Permissions::ALL)
        .unwrap();
    assert!(!space.is_code_current(&snapshot));
}

#[test]
fn unrelated_page_mutations_leave_snapshot_current() {
    let mut space = AddressSpace::new(3).unwrap();
    space
        .map_zeroed(range(0x1000, 1), Permissions::READ_EXECUTE)
        .unwrap();
    space
        .map_zeroed(range(0x8000, 1), Permissions::READ_WRITE)
        .unwrap();
    let snapshot = space.snapshot_code(GuestAddress(0x1800), 1).unwrap();
    space.write(GuestAddress(0x8000), &[0x90]).unwrap();
    assert!(space.is_code_current(&snapshot));
    space
        .map_zeroed(range(0xf000, 1), Permissions::ALL)
        .unwrap();
    assert!(space.is_code_current(&snapshot));
    space.protect(range(0xf000, 1), Permissions::READ).unwrap();
    assert!(space.is_code_current(&snapshot));
    space.unmap(range(0xf000, 1)).unwrap();
    assert!(space.is_code_current(&snapshot));
}

#[test]
fn empty_and_rejected_writes_leave_snapshot_current() {
    let mut space = AddressSpace::new(2).unwrap();
    space
        .map_zeroed(range(0x1000, 1), Permissions::ALL)
        .unwrap();
    space
        .map_zeroed(range(0x2000, 1), Permissions::READ_EXECUTE)
        .unwrap();
    let snapshot = space.snapshot_code(GuestAddress(0x1fff), 1).unwrap();
    space.write(GuestAddress(0x1fff), &[]).unwrap();
    assert!(space.is_code_current(&snapshot));
    assert_fault(
        space.write(GuestAddress(0x1fff), &[0x90, 0xc3]),
        0x2000,
        Access::Write,
        FaultReason::Permission,
    );
    assert!(space.is_code_current(&snapshot));
    assert_fault(
        space.write(GuestAddress(0x3000), &[0x90]),
        0x3000,
        Access::Write,
        FaultReason::Unmapped,
    );
    assert!(space.is_code_current(&snapshot));
}

#[test]
fn rejected_overflow_write_leaves_final_page_snapshot_current() {
    let mut space = AddressSpace::new(1).unwrap();
    space
        .map_zeroed(range(0xffff_f000, 1), Permissions::ALL)
        .unwrap();
    let snapshot = space.snapshot_code(GuestAddress(u32::MAX), 1).unwrap();
    assert_fault(
        space.write(GuestAddress(u32::MAX), &[0x90, 0xc3]),
        u32::MAX,
        Access::Write,
        FaultReason::AddressOverflow,
    );
    assert!(space.is_code_current(&snapshot));
}

#[test]
fn rejected_mapping_and_management_operations_leave_snapshot_current() {
    let mut space = AddressSpace::new(1).unwrap();
    space
        .map_zeroed(range(0x1000, 1), Permissions::ALL)
        .unwrap();
    let snapshot = space.snapshot_code(GuestAddress(0x1800), 1).unwrap();
    assert!(matches!(
        space.map_zeroed(range(0x1000, 2), Permissions::READ),
        Err(MemoryError::AlreadyMapped {
            address: GuestAddress(0x1000)
        })
    ));
    assert!(space.is_code_current(&snapshot));
    assert!(matches!(
        space.map_zeroed(range(0x4000, 1), Permissions::ALL),
        Err(MemoryError::Capacity)
    ));
    assert!(space.is_code_current(&snapshot));
    assert!(matches!(
        space.protect(range(0x1000, 2), Permissions::READ),
        Err(MemoryError::NotMapped {
            address: GuestAddress(0x2000)
        })
    ));
    assert!(space.is_code_current(&snapshot));
    assert!(matches!(
        space.unmap(range(0x1000, 2)),
        Err(MemoryError::NotMapped {
            address: GuestAddress(0x2000)
        })
    ));
    assert!(space.is_code_current(&snapshot));
}

#[test]
fn snapshots_belong_to_one_address_space_despite_matching_operations() {
    let mut first = AddressSpace::new(1).unwrap();
    let mut second = AddressSpace::new(1).unwrap();
    for space in [&mut first, &mut second] {
        space
            .map_zeroed(range(0x1000, 1), Permissions::ALL)
            .unwrap();
        space.write(GuestAddress(0x1800), &[0x90, 0xc3]).unwrap();
    }
    let first_snapshot = first.snapshot_code(GuestAddress(0x1800), 2).unwrap();
    let second_snapshot = second.snapshot_code(GuestAddress(0x1800), 2).unwrap();
    assert!(first.is_code_current(&first_snapshot));
    assert!(second.is_code_current(&second_snapshot));
    assert!(!first.is_code_current(&second_snapshot));
    assert!(!second.is_code_current(&first_snapshot));
}
#[test]
fn dropping_the_original_space_does_not_make_its_snapshot_reusable() {
    let snapshot = {
        let mut original = AddressSpace::new(1).unwrap();
        original
            .map_zeroed(range(0x1000, 1), Permissions::ALL)
            .unwrap();
        original.snapshot_code(GuestAddress(0x1000), 1).unwrap()
    };
    let mut replacement = AddressSpace::new(1).unwrap();
    replacement
        .map_zeroed(range(0x1000, 1), Permissions::ALL)
        .unwrap();
    assert!(!replacement.is_code_current(&snapshot));
}
