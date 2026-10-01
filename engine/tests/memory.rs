use ring3_engine::memory::{
    Access, AddressSpace, BackingOffset, FaultReason, GuestAddress, MemoryError, PAGE_SIZE,
    PageRange, Permissions,
};

fn range(address: u32, pages: u32) -> PageRange {
    PageRange::new(GuestAddress(address), pages).unwrap()
}

fn assert_fault(
    result: Result<(), MemoryError>,
    address: u32,
    access: Access,
    reason: FaultReason,
) {
    let Err(MemoryError::Fault(fault)) = result else {
        panic!("expected guest memory fault, got {result:?}");
    };
    assert_eq!(fault.address.0, address);
    assert_eq!(fault.access, access);
    assert_eq!(fault.reason, reason);
}

#[test]
fn page_ranges_require_aligned_nonempty_nonoverflowing_bounds() {
    assert!(matches!(
        PageRange::new(GuestAddress(1), 1),
        Err(MemoryError::InvalidRange)
    ));
    assert!(matches!(
        PageRange::new(GuestAddress(0), 0),
        Err(MemoryError::InvalidRange)
    ));
    assert!(matches!(
        PageRange::new(GuestAddress(0xffff_f000), 2),
        Err(MemoryError::InvalidRange)
    ));
    assert!(PageRange::new(GuestAddress(0xffff_f000), 1).is_ok());
    assert!(PageRange::new(GuestAddress(0), 1 << 20).is_ok());
}

#[test]
fn mapped_pages_are_zeroed_and_cross_page_data_round_trips() {
    let mut space = AddressSpace::new(2).unwrap();
    assert_eq!(space.capacity_pages(), 2);
    assert_eq!(space.mapped_pages(), 0);
    space
        .map_zeroed(range(0x1000, 2), Permissions::READ_WRITE)
        .unwrap();
    assert_eq!(space.mapped_pages(), 2);

    let mut bytes = [0xa5; 6];
    space.read(GuestAddress(0x1ffd), &mut bytes).unwrap();
    assert_eq!(bytes, [0; 6]);
    space
        .write(GuestAddress(0x1ffd), &[1, 2, 3, 4, 5, 6])
        .unwrap();
    space.read(GuestAddress(0x1ffd), &mut bytes).unwrap();
    assert_eq!(bytes, [1, 2, 3, 4, 5, 6]);
}

#[test]
fn high_guest_addresses_resolve_to_distinct_bounded_backing_pages() {
    let mut space = AddressSpace::new(3).unwrap();
    for address in [0, 0x8000_0000, 0xffff_f000] {
        space
            .map_zeroed(range(address, 1), Permissions::ALL)
            .unwrap();
    }

    let offsets: Vec<BackingOffset> = [0, 0x8000_0000, 0xffff_f000]
        .into_iter()
        .map(|address| space.resolve(GuestAddress(address), Access::Read).unwrap())
        .collect();
    for (index, offset) in offsets.iter().enumerate() {
        assert_eq!(offset.0 % PAGE_SIZE, 0);
        assert!(offset.0 < 3 * PAGE_SIZE);
        assert!(offsets[..index].iter().all(|earlier| earlier.0 != offset.0));
    }
    let final_byte = space
        .resolve(GuestAddress(u32::MAX), Access::Write)
        .unwrap();
    assert_eq!(final_byte.0, offsets[2].0 + PAGE_SIZE - 1);
    space.write(GuestAddress(u32::MAX), &[0x91]).unwrap();
    let mut last = [0];
    space.read(GuestAddress(u32::MAX), &mut last).unwrap();
    assert_eq!(last, [0x91]);
    space.fetch(GuestAddress(u32::MAX), &mut last).unwrap();
    assert_eq!(last, [0x91]);
}

#[test]
fn permissions_are_checked_separately_for_each_access_kind() {
    let mut space = AddressSpace::new(5).unwrap();
    let cases = [
        (Permissions::NONE, false, false, false),
        (Permissions::READ, true, false, false),
        (Permissions::READ_WRITE, true, true, false),
        (Permissions::READ_EXECUTE, true, false, true),
        (Permissions::ALL, true, true, true),
    ];
    for (index, (permissions, read, write, execute)) in cases.into_iter().enumerate() {
        let address = index as u32 * PAGE_SIZE;
        space.map_zeroed(range(address, 1), permissions).unwrap();
        for (access, allowed) in [
            (Access::Read, read),
            (Access::Write, write),
            (Access::Execute, execute),
        ] {
            let result = space.resolve(GuestAddress(address + 17), access);
            if allowed {
                assert!(result.is_ok());
            } else {
                assert_fault(
                    result.map(|_| ()),
                    address + 17,
                    access,
                    FaultReason::Permission,
                );
            }
        }
    }
}

#[test]
fn unmapped_access_reports_the_requested_address_and_preserves_output() {
    let mut space = AddressSpace::new(1).unwrap();
    let mut output = [0x5a; 3];
    assert_fault(
        space.read(GuestAddress(0x2007), &mut output),
        0x2007,
        Access::Read,
        FaultReason::Unmapped,
    );
    assert_eq!(output, [0x5a; 3]);
    assert_fault(
        space.fetch(GuestAddress(0x2007), &mut output),
        0x2007,
        Access::Execute,
        FaultReason::Unmapped,
    );
    assert_eq!(output, [0x5a; 3]);
    assert_fault(
        space.write(GuestAddress(0x2007), &[1, 2, 3]),
        0x2007,
        Access::Write,
        FaultReason::Unmapped,
    );
}

#[test]
fn cross_page_reads_and_fetches_preserve_output_when_next_page_is_unmapped() {
    let mut space = AddressSpace::new(1).unwrap();
    space
        .map_zeroed(range(0x1000, 1), Permissions::ALL)
        .unwrap();
    space.write(GuestAddress(0x1ffe), &[1, 2]).unwrap();

    let mut output = [0x5a; 4];
    assert_fault(
        space.read(GuestAddress(0x1ffe), &mut output),
        0x2000,
        Access::Read,
        FaultReason::Unmapped,
    );
    assert_eq!(output, [0x5a; 4]);
    assert_fault(
        space.fetch(GuestAddress(0x1ffe), &mut output),
        0x2000,
        Access::Execute,
        FaultReason::Unmapped,
    );
    assert_eq!(output, [0x5a; 4]);
}

#[test]
fn cross_page_reads_and_fetches_preserve_output_when_next_page_denies_access() {
    let mut space = AddressSpace::new(2).unwrap();
    space
        .map_zeroed(range(0x1000, 1), Permissions::ALL)
        .unwrap();
    space
        .map_zeroed(range(0x2000, 1), Permissions::NONE)
        .unwrap();
    space.write(GuestAddress(0x1ffe), &[1, 2]).unwrap();

    let mut output = [0x5a; 4];
    assert_fault(
        space.read(GuestAddress(0x1ffe), &mut output),
        0x2000,
        Access::Read,
        FaultReason::Permission,
    );
    assert_eq!(output, [0x5a; 4]);
    assert_fault(
        space.fetch(GuestAddress(0x1ffe), &mut output),
        0x2000,
        Access::Execute,
        FaultReason::Permission,
    );
    assert_eq!(output, [0x5a; 4]);
}

#[test]
fn cross_page_write_does_not_change_prefix_when_next_page_is_unmapped() {
    let mut space = AddressSpace::new(1).unwrap();
    space
        .map_zeroed(range(0x1000, 1), Permissions::READ_WRITE)
        .unwrap();
    space.write(GuestAddress(0x1ffe), &[7, 8]).unwrap();
    assert_fault(
        space.write(GuestAddress(0x1ffe), &[1, 2, 3, 4]),
        0x2000,
        Access::Write,
        FaultReason::Unmapped,
    );
    let mut prefix = [0; 2];
    space.read(GuestAddress(0x1ffe), &mut prefix).unwrap();
    assert_eq!(prefix, [7, 8]);
}

#[test]
fn cross_page_write_does_not_change_either_page_when_next_page_is_read_only() {
    let mut space = AddressSpace::new(2).unwrap();
    space
        .map_zeroed(range(0x1000, 2), Permissions::READ_WRITE)
        .unwrap();
    space.write(GuestAddress(0x1ffe), &[7, 8, 9, 10]).unwrap();
    space.protect(range(0x2000, 1), Permissions::READ).unwrap();
    assert_fault(
        space.write(GuestAddress(0x1ffe), &[1, 2, 3, 4]),
        0x2000,
        Access::Write,
        FaultReason::Permission,
    );
    let mut output = [0; 4];
    space.read(GuestAddress(0x1ffe), &mut output).unwrap();
    assert_eq!(output, [7, 8, 9, 10]);
}

#[test]
fn address_overflow_faults_at_start_without_wrapping_or_mutation() {
    let mut space = AddressSpace::new(2).unwrap();
    space.map_zeroed(range(0, 1), Permissions::ALL).unwrap();
    space
        .map_zeroed(range(0xffff_f000, 1), Permissions::ALL)
        .unwrap();
    space.write(GuestAddress(0), &[0x41]).unwrap();
    space.write(GuestAddress(u32::MAX), &[0x42]).unwrap();

    let mut output = [0x5a; 2];
    assert_fault(
        space.read(GuestAddress(u32::MAX), &mut output),
        u32::MAX,
        Access::Read,
        FaultReason::AddressOverflow,
    );
    assert_eq!(output, [0x5a; 2]);
    assert_fault(
        space.fetch(GuestAddress(u32::MAX), &mut output),
        u32::MAX,
        Access::Execute,
        FaultReason::AddressOverflow,
    );
    assert_eq!(output, [0x5a; 2]);
    assert_fault(
        space.write(GuestAddress(u32::MAX), &[1, 2]),
        u32::MAX,
        Access::Write,
        FaultReason::AddressOverflow,
    );
    let mut first = [0];
    let mut last = [0];
    space.read(GuestAddress(0), &mut first).unwrap();
    space.read(GuestAddress(u32::MAX), &mut last).unwrap();
    assert_eq!(first, [0x41]);
    assert_eq!(last, [0x42]);
}

#[test]
fn empty_accesses_are_noops_even_at_unmapped_final_address() {
    let mut space = AddressSpace::new(1).unwrap();
    space.read(GuestAddress(u32::MAX), &mut []).unwrap();
    space.fetch(GuestAddress(u32::MAX), &mut []).unwrap();
    space.write(GuestAddress(u32::MAX), &[]).unwrap();
    assert_eq!(space.mapped_pages(), 0);
}

#[test]
fn overlapping_map_is_atomic_and_reports_first_existing_page() {
    let mut space = AddressSpace::new(3).unwrap();
    space
        .map_zeroed(range(0x2000, 1), Permissions::READ_WRITE)
        .unwrap();
    space.write(GuestAddress(0x2000), &[0x71]).unwrap();
    assert!(matches!(
        space.map_zeroed(range(0x1000, 3), Permissions::ALL),
        Err(MemoryError::AlreadyMapped {
            address: GuestAddress(0x2000)
        })
    ));
    assert_eq!(space.mapped_pages(), 1);
    assert_fault(
        space
            .resolve(GuestAddress(0x1000), Access::Read)
            .map(|_| ()),
        0x1000,
        Access::Read,
        FaultReason::Unmapped,
    );
    assert_fault(
        space
            .resolve(GuestAddress(0x3000), Access::Read)
            .map(|_| ()),
        0x3000,
        Access::Read,
        FaultReason::Unmapped,
    );
    let mut existing = [0];
    space.read(GuestAddress(0x2000), &mut existing).unwrap();
    assert_eq!(existing, [0x71]);
    assert_fault(
        space
            .resolve(GuestAddress(0x2000), Access::Execute)
            .map(|_| ()),
        0x2000,
        Access::Execute,
        FaultReason::Permission,
    );
}

#[test]
fn insufficient_capacity_does_not_map_prefix_or_consume_slots() {
    let mut space = AddressSpace::new(2).unwrap();
    space
        .map_zeroed(range(0x1000, 1), Permissions::READ_WRITE)
        .unwrap();
    assert!(matches!(
        space.map_zeroed(range(0x4000, 2), Permissions::READ_WRITE),
        Err(MemoryError::Capacity)
    ));
    assert_eq!(space.mapped_pages(), 1);
    for address in [0x4000, 0x5000] {
        assert_fault(
            space
                .resolve(GuestAddress(address), Access::Read)
                .map(|_| ()),
            address,
            Access::Read,
            FaultReason::Unmapped,
        );
    }
    space
        .map_zeroed(range(0x7000, 1), Permissions::READ_WRITE)
        .unwrap();
    assert_eq!(space.mapped_pages(), 2);
}

#[test]
fn overlapping_map_precedes_capacity_failure() {
    let mut space = AddressSpace::new(1).unwrap();
    space
        .map_zeroed(range(0x2000, 1), Permissions::READ)
        .unwrap();
    assert!(matches!(
        space.map_zeroed(range(0x1000, 3), Permissions::READ),
        Err(MemoryError::AlreadyMapped {
            address: GuestAddress(0x2000)
        })
    ));
    assert_eq!(space.mapped_pages(), 1);
}

#[test]
fn protect_of_partially_mapped_range_does_not_change_existing_permissions() {
    let mut space = AddressSpace::new(1).unwrap();
    space
        .map_zeroed(range(0x1000, 1), Permissions::READ_WRITE)
        .unwrap();
    assert!(matches!(
        space.protect(range(0x1000, 2), Permissions::NONE),
        Err(MemoryError::NotMapped {
            address: GuestAddress(0x2000)
        })
    ));
    space.write(GuestAddress(0x1000), &[0x61]).unwrap();
    let mut output = [0];
    space.read(GuestAddress(0x1000), &mut output).unwrap();
    assert_eq!(output, [0x61]);
}

#[test]
fn unmap_of_partially_mapped_range_does_not_remove_existing_page() {
    let mut space = AddressSpace::new(1).unwrap();
    space
        .map_zeroed(range(0x1000, 1), Permissions::READ_WRITE)
        .unwrap();
    space.write(GuestAddress(0x1000), &[0x61]).unwrap();
    let offset = space.resolve(GuestAddress(0x1000), Access::Read).unwrap();
    assert!(matches!(
        space.unmap(range(0x1000, 2)),
        Err(MemoryError::NotMapped {
            address: GuestAddress(0x2000)
        })
    ));
    assert_eq!(space.mapped_pages(), 1);
    assert_eq!(
        space.resolve(GuestAddress(0x1000), Access::Read).unwrap().0,
        offset.0
    );
    let mut output = [0];
    space.read(GuestAddress(0x1000), &mut output).unwrap();
    assert_eq!(output, [0x61]);
}

#[test]
fn protection_changes_access_without_moving_or_erasing_backing() {
    let mut space = AddressSpace::new(1).unwrap();
    space
        .map_zeroed(range(0x1000, 1), Permissions::READ_WRITE)
        .unwrap();
    space.write(GuestAddress(0x1000), &[0x61]).unwrap();
    let offset = space.resolve(GuestAddress(0x1000), Access::Read).unwrap();
    space
        .protect(range(0x1000, 1), Permissions::READ_EXECUTE)
        .unwrap();
    assert_eq!(
        space
            .resolve(GuestAddress(0x1000), Access::Execute)
            .unwrap()
            .0,
        offset.0
    );
    assert_fault(
        space.write(GuestAddress(0x1000), &[0x71]),
        0x1000,
        Access::Write,
        FaultReason::Permission,
    );
    let mut output = [0];
    space.fetch(GuestAddress(0x1000), &mut output).unwrap();
    assert_eq!(output, [0x61]);
}

#[test]
fn reused_backing_is_zeroed_and_old_guest_mapping_is_removed() {
    let mut space = AddressSpace::new(1).unwrap();
    space
        .map_zeroed(range(0x1000, 1), Permissions::READ_WRITE)
        .unwrap();
    space
        .write(GuestAddress(0x1000), &vec![0x91; PAGE_SIZE as usize])
        .unwrap();
    let offset = space.resolve(GuestAddress(0x1000), Access::Read).unwrap();
    space.unmap(range(0x1000, 1)).unwrap();
    assert_eq!(space.mapped_pages(), 0);
    space
        .map_zeroed(range(0x8000_0000, 1), Permissions::READ_WRITE)
        .unwrap();
    assert_eq!(
        space
            .resolve(GuestAddress(0x8000_0000), Access::Read)
            .unwrap()
            .0,
        offset.0
    );
    let mut output = vec![0xa5; PAGE_SIZE as usize];
    space.read(GuestAddress(0x8000_0000), &mut output).unwrap();
    assert!(output.iter().all(|byte| *byte == 0));
    assert_fault(
        space
            .resolve(GuestAddress(0x1000), Access::Read)
            .map(|_| ()),
        0x1000,
        Access::Read,
        FaultReason::Unmapped,
    );
}
