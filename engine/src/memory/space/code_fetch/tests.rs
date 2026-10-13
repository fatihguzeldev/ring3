use super::*;

fn mapped(low: bool) -> AddressSpace {
    let mut memory = AddressSpace::new(3).unwrap();
    for page in [0xfffff000, 0x2000].into_iter().chain(low.then_some(0)) {
        memory
            .map_zeroed(
                PageRange::new(GuestAddress(page), 1).unwrap(),
                Permissions::ALL,
            )
            .unwrap();
    }
    memory
        .write(GuestAddress(0xfffffffd), &[0xb8, 0x78, 0x56])
        .unwrap();
    if low {
        memory.write(GuestAddress(0), &[0x34, 0x12]).unwrap();
    }
    memory
}

#[test]
fn code_only_linear_wrap_fetches_both_ranges_without_weakening_ordinary_access() {
    let memory = mapped(true);
    let mut bytes = [0xa5; 5];
    memory
        .fetch_code_wrapping(GuestAddress(0xfffffffd), &mut bytes)
        .unwrap();
    assert_eq!(bytes, [0xb8, 0x78, 0x56, 0x34, 0x12]);
    for access in [Access::Read, Access::Execute] {
        assert_eq!(
            memory.check_span(GuestAddress(0xfffffffd), 5, access),
            Err(MemoryError::Fault(MemoryFault {
                address: GuestAddress(0xfffffffd),
                access,
                reason: FaultReason::AddressOverflow,
            }))
        );
    }
    assert!(memory.snapshot_code(GuestAddress(0xfffffffd), 5).is_err());
}

#[test]
fn low_execute_fault_is_literal_and_keeps_fetch_output_atomic() {
    for permission in [None, Some(Permissions::READ)] {
        let mut memory = mapped(permission.is_some());
        if let Some(permission) = permission {
            memory
                .protect(PageRange::new(GuestAddress(0), 1).unwrap(), permission)
                .unwrap();
        }
        let mut bytes = [0xa5; 5];
        let expected = Err(MemoryError::Fault(MemoryFault {
            address: GuestAddress(0),
            access: Access::Execute,
            reason: if permission.is_some() {
                FaultReason::Permission
            } else {
                FaultReason::Unmapped
            },
        }));
        assert_eq!(
            memory.fetch_code_wrapping(GuestAddress(0xfffffffd), &mut bytes),
            expected
        );
        assert_eq!(bytes, [0xa5; 5]);
        assert!(
            matches!(memory.snapshot_code_wrapping(GuestAddress(0xfffffffd), 5), Err(MemoryError::Fault(fault)) if fault.address == GuestAddress(0) && fault.access == Access::Execute)
        );
    }
}

#[test]
fn wrapped_snapshots_track_both_ordered_pages_and_never_revive_equal_content() {
    for address in [0xfffffffd, 0] {
        let mut memory = mapped(true);
        let snapshot = memory
            .snapshot_code_wrapping(GuestAddress(0xfffffffd), 5)
            .unwrap();
        assert_eq!(
            (snapshot.first, snapshot.versions.len()),
            (GUEST_PAGES - 1, 2)
        );
        memory.write(GuestAddress(0x2000), &[0x90]).unwrap();
        assert!(memory.is_code_current(&snapshot));
        let byte = if address == 0 { 0x34 } else { 0xb8 };
        memory.write(GuestAddress(address), &[byte]).unwrap();
        assert!(!memory.is_code_current(&snapshot));
        memory.write(GuestAddress(address), &[byte]).unwrap();
        assert!(!memory.is_code_current(&snapshot));
    }
}

#[test]
fn wrapped_snapshot_preserves_mapping_permission_and_instance_identity() {
    let mut memory = mapped(true);
    let original = memory
        .snapshot_code_wrapping(GuestAddress(0xfffffffd), 5)
        .unwrap();
    assert!(!mapped(true).is_code_current(&original));
    let low = PageRange::new(GuestAddress(0), 1).unwrap();
    memory.protect(low, Permissions::READ).unwrap();
    assert!(!memory.is_code_current(&original));
    memory.protect(low, Permissions::ALL).unwrap();
    assert!(!memory.is_code_current(&original));
    let remap = memory
        .snapshot_code_wrapping(GuestAddress(0xfffffffd), 5)
        .unwrap();
    memory.unmap(low).unwrap();
    assert!(!memory.is_code_current(&remap));
    memory.map_zeroed(low, Permissions::ALL).unwrap();
    memory.write(GuestAddress(0), &[0x34, 0x12]).unwrap();
    assert!(!memory.is_code_current(&remap));
}

#[test]
fn snapshot_range_limits_do_not_turn_arbitrary_lengths_into_circular_reads() {
    let memory = mapped(true);
    assert!(matches!(
        memory.snapshot_code_wrapping(GuestAddress(0), 0),
        Err(MemoryError::InvalidRange)
    ));
    if let Ok(length) = usize::try_from(ADDRESS_LIMIT + 1) {
        assert!(matches!(
            memory.snapshot_code_wrapping(GuestAddress(0xfffffffd), length),
            Err(MemoryError::InvalidRange)
        ));
    }
}

#[test]
fn high_execute_denial_precedes_missing_low_tail_without_partial_fetch() {
    let mut memory = mapped(false);
    memory
        .protect(
            PageRange::new(GuestAddress(0xfffff000), 1).unwrap(),
            Permissions::READ,
        )
        .unwrap();
    let fault = MemoryError::Fault(MemoryFault {
        address: GuestAddress(0xfffffffd),
        access: Access::Execute,
        reason: FaultReason::Permission,
    });
    let mut output = [0xa5; 5];
    assert_eq!(
        memory.fetch_code_wrapping(GuestAddress(0xfffffffd), &mut output),
        Err(fault)
    );
    assert_eq!(output, [0xa5; 5]);
    assert!(matches!(
        memory.snapshot_code_wrapping(GuestAddress(0xfffffffd), 5),
        Err(error) if error == fault
    ));
}

#[test]
fn full_ring_length_is_admitted_but_execute_preflight_stops_at_first_missing_page() {
    let Ok(length) = usize::try_from(ADDRESS_LIMIT) else {
        return;
    };
    let memory = mapped(true);
    for (address, split) in [(0, length), (1, length - 1), (u32::MAX, 1)] {
        assert_eq!(code_split(GuestAddress(address), length), Ok(split));
        assert!(matches!(
            memory.snapshot_code_wrapping(GuestAddress(address), length),
            Err(MemoryError::Fault(MemoryFault {
                address: GuestAddress(0x1000),
                access: Access::Execute,
                reason: FaultReason::Unmapped,
            }))
        ));
    }
}
