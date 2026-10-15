use super::*;

#[test]
fn compare_exchange_match_mismatch_and_identity_use_one_final_version() {
    for (first, second) in [
        (Permissions::READ_WRITE, Permissions::READ_WRITE),
        (Permissions::ALL, Permissions::READ_WRITE),
        (Permissions::READ_WRITE, Permissions::ALL),
        (Permissions::ALL, Permissions::ALL),
    ] {
        for (expected, desired, committed) in [
            (0x4433_2211, 0xaabb_ccdd_u32, 0xaabb_ccdd_u32),
            (0x4433_2211, 0x4433_2211, 0x4433_2211),
            (0x4433_2210, 0xaabb_ccdd, 0x4433_2211),
        ] {
            let mut space = AddressSpace::new(3).unwrap();
            for (base, permissions) in [
                (0x1000, first),
                (0x3000, Permissions::ALL),
                (0x2000, second),
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
            let mut after = exchange_state(&space);
            for (index, byte) in committed.to_le_bytes().into_iter().enumerate() {
                let offset = space
                    .resolve(GuestAddress(0x1ffe + index as u32), Access::Write)
                    .unwrap()
                    .0;
                after.backing[offset as usize] = byte;
            }
            for base in [0x1000, 0x2000] {
                let slot = space.mappings[base as usize / PAGE_SIZE as usize] as usize - 1;
                after.versions[slot].content = u64::MAX;
            }
            after.version = u64::MAX;
            if snapshots.iter().any(Option::is_some) {
                after.code_version = u64::MAX;
            }
            assert_eq!(
                space.compare_exchange32(GuestAddress(0x1ffe), expected, desired),
                Ok(0x4433_2211)
            );
            assert_eq!(exchange_state(&space), after);
            for snapshot in snapshots.into_iter().flatten() {
                assert!(!space.is_code_current(&snapshot));
            }
            assert!(space.is_code_current(&unrelated));
            for expected in [committed, committed ^ 1] {
                assert_eq!(
                    space.compare_exchange32(GuestAddress(0x1ffe), expected, 0),
                    Err(MemoryError::VersionExhausted)
                );
                assert_eq!(exchange_state(&space), after);
                assert!(space.is_code_current(&unrelated));
            }
        }
    }
}

#[test]
fn compare_exchange_access_faults_precede_exhaustion_without_private_mutation() {
    for version in [u64::MAX - 1, u64::MAX] {
        for (first, second, address, access, at, reason) in [
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
                1,
                Some(3),
                0x1ffe,
                Access::Write,
                0x1ffe,
                FaultReason::Permission,
            ),
            (
                3,
                Some(1),
                0x1ffe,
                Access::Write,
                0x2000,
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
                7,
                Some(3),
                0xffff_fffd,
                Access::Read,
                0xffff_fffd,
                FaultReason::AddressOverflow,
            ),
            (
                7,
                Some(3),
                0x9001,
                Access::Read,
                0x9001,
                FaultReason::Unmapped,
            ),
        ] {
            let mut space = AddressSpace::new(4).unwrap();
            let range = |base| PageRange::new(GuestAddress(base), 1).unwrap();
            space.map_zeroed(range(0x1000), Permissions::ALL).unwrap();
            space
                .write(GuestAddress(0x1ffc), &[0xa5, 0x5a, 0x11, 0x22])
                .unwrap();
            if let Some(bits) = second {
                space.map_zeroed(range(0x2000), Permissions::ALL).unwrap();
                space
                    .write(GuestAddress(0x2000), &[0x33, 0x44, 0x5a, 0xa5])
                    .unwrap();
                space
                    .protect(range(0x2000), Permissions::from_bits(bits).unwrap())
                    .unwrap();
            }
            space
                .protect(range(0x1000), Permissions::from_bits(first).unwrap())
                .unwrap();
            for base in [0, 0x3000] {
                space.map_zeroed(range(base), Permissions::ALL).unwrap();
                space
                    .write(GuestAddress(base), &[0xaa, 0x55, 0x33, 0xcc])
                    .unwrap();
            }
            let snapshot = space.snapshot_code(GuestAddress(0x3000), 4).unwrap();
            space.version = version;
            let before = exchange_state(&space);
            for expected in [0x4433_2211, 0x4433_2210] {
                assert_eq!(
                    space.compare_exchange32(GuestAddress(address), expected, 0xdead_beef),
                    Err(fault(GuestAddress(at), access, reason))
                );
                assert_eq!(exchange_state(&space), before);
                assert!(space.is_code_current(&snapshot));
            }
            if version == u64::MAX - 1 && first == 3 {
                assert_eq!(
                    space.compare_exchange32(GuestAddress(0x1004), 0, 0x1122_3344),
                    Ok(0)
                );
                assert_eq!(space.version, u64::MAX);
                assert!(space.is_code_current(&snapshot));
            }
        }
    }
}

#[test]
fn byte_compare_exchange_match_mismatch_and_identity_use_one_final_version() {
    for permissions in [Permissions::READ_WRITE, Permissions::ALL] {
        for address in [0x1011, 0x1fff, u32::MAX] {
            for (expected, desired, committed) in
                [(0x5a, 0xa5, 0xa5), (0x5a, 0x5a, 0x5a), (0x5b, 0xa5, 0x5a)]
            {
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
                let mut after = exchange_state(&space);
                let offset = space
                    .resolve(GuestAddress(address), Access::Write)
                    .unwrap()
                    .0 as usize;
                after.backing[offset] = committed;
                let slot = space.mappings[(address / PAGE_SIZE) as usize] as usize - 1;
                after.versions[slot].content = u64::MAX;
                after.version = u64::MAX;
                if consumed.is_some() {
                    after.code_version = u64::MAX;
                }
                assert_eq!(
                    space.compare_exchange8(GuestAddress(address), expected, desired),
                    Ok(0x5a)
                );
                assert_eq!(exchange_state(&space), after);
                if let Some(snapshot) = consumed {
                    assert!(!space.is_code_current(&snapshot));
                }
                assert!(space.is_code_current(&unrelated));
                for expected in [committed, committed ^ 1] {
                    assert_eq!(
                        space.compare_exchange8(GuestAddress(address), expected, 0x11),
                        Err(MemoryError::VersionExhausted)
                    );
                    assert_eq!(exchange_state(&space), after);
                    assert!(space.is_code_current(&unrelated));
                }
            }
        }
    }
}

#[test]
fn byte_compare_exchange_access_faults_precede_exhaustion_without_private_mutation() {
    for version in [u64::MAX - 1, u64::MAX] {
        for address in [0x1fff, u32::MAX] {
            for bits in [None, Some(0), Some(1), Some(2), Some(4), Some(5), Some(6)] {
                let mut space = AddressSpace::new(2).unwrap();
                if let Some(bits) = bits {
                    let range = PageRange::new(GuestAddress(address & !0xfff), 1).unwrap();
                    space.map_zeroed(range, Permissions::ALL).unwrap();
                    space.write(GuestAddress(address), &[0x5a]).unwrap();
                    space
                        .protect(range, Permissions::from_bits(bits).unwrap())
                        .unwrap();
                }
                space
                    .map_zeroed(
                        PageRange::new(GuestAddress(0x3000), 1).unwrap(),
                        Permissions::ALL,
                    )
                    .unwrap();
                let snapshot = space.snapshot_code(GuestAddress(0x3000), 1).unwrap();
                space.version = version;
                let before = exchange_state(&space);
                let (access, reason) = match bits {
                    None => (Access::Read, FaultReason::Unmapped),
                    Some(bits) => (
                        if bits & 1 == 0 {
                            Access::Read
                        } else {
                            Access::Write
                        },
                        FaultReason::Permission,
                    ),
                };
                for expected in [0x5a, 0x5b] {
                    assert_eq!(
                        space.compare_exchange8(GuestAddress(address), expected, 0xa5),
                        Err(fault(GuestAddress(address), access, reason))
                    );
                    assert_eq!(exchange_state(&space), before);
                    assert!(space.is_code_current(&snapshot));
                }
                if version == u64::MAX - 1 {
                    assert_eq!(
                        space.compare_exchange8(GuestAddress(0x3000), 1, 0xff),
                        Ok(0)
                    );
                    assert_eq!(space.version, u64::MAX);
                    assert_eq!(space.code_validation_version(), u64::MAX);
                    assert!(!space.is_code_current(&snapshot));
                }
            }
        }
    }
}
#[test]
fn word_compare_exchange_match_mismatch_and_identity_use_one_final_version() {
    for (first, second) in [
        (Permissions::READ_WRITE, Permissions::READ_WRITE),
        (Permissions::ALL, Permissions::READ_WRITE),
        (Permissions::READ_WRITE, Permissions::ALL),
        (Permissions::ALL, Permissions::ALL),
    ] {
        for (expected, desired, committed) in [
            (0x3322, 0xaabb_u16, 0xaabb_u16),
            (0x3322, 0x3322, 0x3322),
            (0x3323, 0xaabb, 0x3322),
        ] {
            let mut space = AddressSpace::new(3).unwrap();
            for (base, permissions) in [
                (0x1000, first),
                (0x3000, Permissions::ALL),
                (0x2000, second),
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
            let mut after = exchange_state(&space);
            for (index, byte) in committed.to_le_bytes().into_iter().enumerate() {
                let offset = space
                    .resolve(GuestAddress(0x1fff + index as u32), Access::Write)
                    .unwrap()
                    .0;
                after.backing[offset as usize] = byte;
            }
            for base in [0x1000, 0x2000] {
                let slot = space.mappings[base as usize / PAGE_SIZE as usize] as usize - 1;
                after.versions[slot].content = u64::MAX;
            }
            after.version = u64::MAX;
            if snapshots.iter().any(Option::is_some) {
                after.code_version = u64::MAX;
            }
            assert_eq!(
                space.compare_exchange16(GuestAddress(0x1fff), expected, desired),
                Ok(0x3322)
            );
            assert_eq!(exchange_state(&space), after);
            for snapshot in snapshots.into_iter().flatten() {
                assert!(!space.is_code_current(&snapshot));
            }
            assert!(space.is_code_current(&unrelated));
            for expected in [committed, committed ^ 1] {
                assert_eq!(
                    space.compare_exchange16(GuestAddress(0x1fff), expected, 0),
                    Err(MemoryError::VersionExhausted)
                );
                assert_eq!(exchange_state(&space), after);
                assert!(space.is_code_current(&unrelated));
            }
        }
    }
}

#[test]
fn word_compare_exchange_access_faults_precede_exhaustion_without_private_mutation() {
    for version in [u64::MAX - 1, u64::MAX] {
        for (first, second, address, access, at, reason) in [
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
                1,
                Some(3),
                0x1fff,
                Access::Write,
                0x1fff,
                FaultReason::Permission,
            ),
            (
                3,
                Some(1),
                0x1fff,
                Access::Write,
                0x2000,
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
                7,
                Some(3),
                u32::MAX,
                Access::Read,
                u32::MAX,
                FaultReason::AddressOverflow,
            ),
            (
                7,
                Some(3),
                0x9001,
                Access::Read,
                0x9001,
                FaultReason::Unmapped,
            ),
        ] {
            let mut space = AddressSpace::new(4).unwrap();
            let range = |base| PageRange::new(GuestAddress(base), 1).unwrap();
            space.map_zeroed(range(0x1000), Permissions::ALL).unwrap();
            space
                .write(GuestAddress(0x1ffc), &[0xa5, 0x5a, 0x11, 0x22])
                .unwrap();
            if let Some(bits) = second {
                space.map_zeroed(range(0x2000), Permissions::ALL).unwrap();
                space
                    .write(GuestAddress(0x2000), &[0x33, 0x44, 0x5a, 0xa5])
                    .unwrap();
                space
                    .protect(range(0x2000), Permissions::from_bits(bits).unwrap())
                    .unwrap();
            }
            space
                .protect(range(0x1000), Permissions::from_bits(first).unwrap())
                .unwrap();
            for base in [0, 0x3000] {
                space.map_zeroed(range(base), Permissions::ALL).unwrap();
                space
                    .write(GuestAddress(base), &[0xaa, 0x55, 0x33, 0xcc])
                    .unwrap();
            }
            let snapshot = space.snapshot_code(GuestAddress(0x3000), 4).unwrap();
            space.version = version;
            let before = exchange_state(&space);
            for expected in [0x3322, 0x3323] {
                assert_eq!(
                    space.compare_exchange16(GuestAddress(address), expected, 0xbeef),
                    Err(fault(GuestAddress(at), access, reason))
                );
                assert_eq!(exchange_state(&space), before);
                assert!(space.is_code_current(&snapshot));
            }
            if version == u64::MAX - 1 {
                assert_eq!(
                    space.compare_exchange16(GuestAddress(0x3000), 0x55aa, 0x55aa),
                    Ok(0x55aa)
                );
                assert_eq!(space.version, u64::MAX);
                assert_eq!(space.code_validation_version(), u64::MAX);
                assert!(!space.is_code_current(&snapshot));
            }
        }
    }
}
