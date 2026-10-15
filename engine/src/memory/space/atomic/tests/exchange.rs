use super::*;

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

#[test]
fn byte_exchange_faults_precede_exhaustion_and_preserve_the_entire_memory_owner() {
    for version in [u64::MAX - 1, u64::MAX] {
        for address in [0x1fff, u32::MAX] {
            for bits in [0, 1, 2, 4, 5, 6] {
                let mut space = AddressSpace::new(2).unwrap();
                let range = PageRange::new(GuestAddress(address & !0xfff), 1).unwrap();
                space.map_zeroed(range, Permissions::ALL).unwrap();
                space.write(GuestAddress(address), &[0x5a]).unwrap();
                space
                    .protect(range, Permissions::from_bits(bits).unwrap())
                    .unwrap();
                space
                    .map_zeroed(
                        PageRange::new(GuestAddress(0x3000), 1).unwrap(),
                        Permissions::ALL,
                    )
                    .unwrap();
                let unrelated = space.snapshot_code(GuestAddress(0x3000), 1).unwrap();
                space.version = version;
                let before = exchange_state(&space);
                let access = if bits & 1 == 0 {
                    Access::Read
                } else {
                    Access::Write
                };
                assert_eq!(
                    space.exchange8(GuestAddress(address), 0xff),
                    Err(fault(
                        GuestAddress(address),
                        access,
                        FaultReason::Permission
                    ))
                );
                assert_eq!(exchange_state(&space), before);
                assert!(space.is_code_current(&unrelated));
            }
        }
        let mut space = AddressSpace::new(1).unwrap();
        space.version = version;
        let before = exchange_state(&space);
        assert_eq!(
            space.exchange8(GuestAddress(u32::MAX), 0xff),
            Err(fault(
                GuestAddress(u32::MAX),
                Access::Read,
                FaultReason::Unmapped
            ))
        );
        assert_eq!(exchange_state(&space), before);
    }
}

#[test]
fn final_byte_exchange_version_commits_once_for_identity_and_changed_values() {
    for permissions in [Permissions::READ_WRITE, Permissions::ALL] {
        for address in [0x1fff, u32::MAX] {
            for value in [0x5a, 0xa5] {
                let mut space = AddressSpace::new(2).unwrap();
                space
                    .map_zeroed(
                        PageRange::new(GuestAddress(address & !0xfff), 1).unwrap(),
                        permissions,
                    )
                    .unwrap();
                space
                    .map_zeroed(
                        PageRange::new(GuestAddress(0x3000), 1).unwrap(),
                        Permissions::ALL,
                    )
                    .unwrap();
                space.write(GuestAddress(address), &[0x5a]).unwrap();
                let consumed = permissions
                    .allows(Access::Execute)
                    .then(|| space.snapshot_code(GuestAddress(address), 1).unwrap());
                let unrelated = space.snapshot_code(GuestAddress(0x3000), 1).unwrap();
                space.version = u64::MAX - 1;
                let mut expected = exchange_state(&space);
                let offset = space
                    .resolve(GuestAddress(address), Access::Write)
                    .unwrap()
                    .0 as usize;
                expected.backing[offset] = value;
                let slot = space.mappings[(address / PAGE_SIZE) as usize] as usize - 1;
                expected.versions[slot].content = u64::MAX;
                expected.version = u64::MAX;
                if consumed.is_some() {
                    expected.code_version = u64::MAX;
                }
                assert_eq!(space.exchange8(GuestAddress(address), value), Ok(0x5a));
                assert_eq!(exchange_state(&space), expected);
                if let Some(snapshot) = consumed {
                    assert!(!space.is_code_current(&snapshot));
                }
                assert!(space.is_code_current(&unrelated));
                for _ in 0..2 {
                    assert_eq!(
                        space.exchange8(GuestAddress(address), 0x11),
                        Err(MemoryError::VersionExhausted)
                    );
                    assert_eq!(exchange_state(&space), expected);
                    assert!(space.is_code_current(&unrelated));
                }
            }
        }
    }
}

#[test]
fn failed_byte_exchange_keeps_the_last_version_available_for_a_valid_top_byte() {
    let mut space = AddressSpace::new(2).unwrap();
    space
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::READ,
        )
        .unwrap();
    space
        .map_zeroed(
            PageRange::new(GuestAddress(0xffff_f000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    let snapshot = space.snapshot_code(GuestAddress(u32::MAX), 1).unwrap();
    space.version = u64::MAX - 1;
    let before = exchange_state(&space);
    assert_eq!(
        space.exchange8(GuestAddress(0x1001), 0xff),
        Err(fault(
            GuestAddress(0x1001),
            Access::Write,
            FaultReason::Permission
        ))
    );
    assert_eq!(exchange_state(&space), before);
    assert_eq!(space.exchange8(GuestAddress(u32::MAX), 0xff), Ok(0));
    assert_eq!(space.version, u64::MAX);
    assert_eq!(space.code_validation_version(), u64::MAX);
    assert!(!space.is_code_current(&snapshot));
    let before = exchange_state(&space);
    assert_eq!(
        space.exchange8(GuestAddress(0x1001), 0xff),
        Err(fault(
            GuestAddress(0x1001),
            Access::Write,
            FaultReason::Permission
        ))
    );
    assert_eq!(exchange_state(&space), before);
}

#[test]
fn word_exchange_access_faults_preserve_all_owners_even_at_exhaustion() {
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
                0x1fff,
                Access::Read,
                0x2000,
                FaultReason::Permission,
            ),
            (1, None, 0x1fff, Access::Read, 0x2000, FaultReason::Unmapped),
            (
                7,
                Some(1),
                0x1fff,
                Access::Write,
                0x2000,
                FaultReason::Permission,
            ),
            (
                1,
                Some(3),
                0x1fff,
                Access::Write,
                0x1fff,
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
                    space.exchange16(GuestAddress(address), 0xbeef),
                    Err(fault(GuestAddress(at), access, reason))
                );
                assert_eq!(exchange_state(&space), before);
                assert!(space.is_code_current(&snapshot));
            }
        }
    }
}

#[test]
fn word_exchange_uses_one_final_version_for_both_pages_and_identity_writes() {
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
            let value = if identical { 0x3322_u16 } else { 0xaabb };
            for (index, byte) in value.to_le_bytes().into_iter().enumerate() {
                let offset = space
                    .resolve(GuestAddress(0x1fff + index as u32), Access::Write)
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
            assert_eq!(space.exchange16(GuestAddress(0x1fff), value), Ok(0x3322));
            assert_eq!(exchange_state(&space), expected);
            for snapshot in snapshots.into_iter().flatten() {
                assert!(!space.is_code_current(&snapshot));
            }
            assert!(space.is_code_current(&unrelated));
            for _ in 0..2 {
                assert_eq!(
                    space.exchange16(GuestAddress(0x1fff), 0xbeef),
                    Err(MemoryError::VersionExhausted)
                );
                assert_eq!(exchange_state(&space), expected);
                assert!(space.is_code_current(&unrelated));
            }
        }
    }
}

#[test]
fn word_exchange_faults_leave_the_final_version_for_a_valid_commit() {
    let mut space = AddressSpace::new(2).unwrap();
    space
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 2).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    space
        .write(GuestAddress(0x1fff), &[0x11, 0x22, 0x33, 0x44])
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
        space.exchange16(GuestAddress(0x1fff), 0xaabb),
        Err(fault(
            GuestAddress(0x2000),
            Access::Write,
            FaultReason::Permission
        ))
    );
    assert_eq!(exchange_state(&space), before);
    assert!(space.is_code_current(&snapshot));
    assert_eq!(space.exchange16(GuestAddress(0x1004), 0xaabb), Ok(0));
    assert_eq!(space.version, u64::MAX);
    assert_eq!(space.code_validation_version(), u64::MAX);
    assert!(!space.is_code_current(&snapshot));
    let before = exchange_state(&space);
    assert_eq!(
        space.exchange16(GuestAddress(0x1fff), 0),
        Err(fault(
            GuestAddress(0x2000),
            Access::Write,
            FaultReason::Permission
        ))
    );
    assert_eq!(exchange_state(&space), before);
}
