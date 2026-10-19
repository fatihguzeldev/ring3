use super::*;

#[test]
fn bit_test_set_changed_and_identity_use_one_final_version() {
    for (first, second) in [
        (Permissions::READ_WRITE, Permissions::READ_WRITE),
        (Permissions::ALL, Permissions::READ_WRITE),
        (Permissions::READ_WRITE, Permissions::ALL),
        (Permissions::ALL, Permissions::ALL),
    ] {
        for (old, bit, committed) in [
            (0x4433_2201_u32, 5, 0x4433_2221_u32),
            (0x4433_2221, 5, 0x4433_2221),
            (0x4433_2211, 31, 0xc433_2211),
            (0x4433_2210, 0xffff_ffe0, 0x4433_2211),
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
            let mut frame = [0xa5, 0x5a, 0, 0, 0, 0, 0x5a, 0xa5];
            frame[2..6].copy_from_slice(&old.to_le_bytes());
            space.write(GuestAddress(0x1ffc), &frame).unwrap();
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
                after.source_version = u64::MAX;
            }
            assert_eq!(space.bit_test_set32(GuestAddress(0x1ffe), bit), Ok(old));
            assert_eq!(exchange_state(&space), after);
            for snapshot in snapshots.into_iter().flatten() {
                assert!(!space.is_code_current(&snapshot));
            }
            assert!(space.is_code_current(&unrelated));
            for bit in [0, 31] {
                assert_eq!(
                    space.bit_test_set32(GuestAddress(0x1ffe), bit),
                    Err(MemoryError::VersionExhausted)
                );
                assert_eq!(exchange_state(&space), after);
                assert!(space.is_code_current(&unrelated));
            }
        }
    }
}

#[test]
fn bit_test_set_access_faults_precede_exhaustion_without_private_mutation() {
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
                0xffff_fffe,
                Access::Read,
                0xffff_fffe,
                FaultReason::AddressOverflow,
            ),
            (
                7,
                Some(3),
                0xffff_ffff,
                Access::Read,
                0xffff_ffff,
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
            for bit in [5, 37] {
                assert_eq!(
                    space.bit_test_set32(GuestAddress(address), bit),
                    Err(fault(GuestAddress(at), access, reason))
                );
                assert_eq!(exchange_state(&space), before);
                assert!(space.is_code_current(&snapshot));
            }
            if version == u64::MAX - 1 && first == 3 {
                assert_eq!(space.bit_test_set32(GuestAddress(0x1004), 5), Ok(0));
                assert_eq!(space.version, u64::MAX);
                assert!(space.is_code_current(&snapshot));
            }
        }
    }
}

#[test]
fn bit_test_reset_changed_and_identity_use_one_final_version() {
    for (first, second) in [
        (Permissions::READ_WRITE, Permissions::READ_WRITE),
        (Permissions::ALL, Permissions::READ_WRITE),
        (Permissions::READ_WRITE, Permissions::ALL),
        (Permissions::ALL, Permissions::ALL),
    ] {
        for (old, bit, committed) in [
            (0x4433_2221_u32, 5, 0x4433_2201_u32),
            (0x4433_2201, 5, 0x4433_2201),
            (0xc433_2211, 31, 0x4433_2211),
            (0x4433_2211, 0xffff_ffe0, 0x4433_2210),
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
            let mut frame = [0xa5, 0x5a, 0, 0, 0, 0, 0x5a, 0xa5];
            frame[2..6].copy_from_slice(&old.to_le_bytes());
            space.write(GuestAddress(0x1ffc), &frame).unwrap();
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
                after.source_version = u64::MAX;
            }
            assert_eq!(space.bit_test_reset32(GuestAddress(0x1ffe), bit), Ok(old));
            assert_eq!(exchange_state(&space), after);
            for snapshot in snapshots.into_iter().flatten() {
                assert!(!space.is_code_current(&snapshot));
            }
            assert!(space.is_code_current(&unrelated));
            for bit in [0, 31] {
                assert_eq!(
                    space.bit_test_reset32(GuestAddress(0x1ffe), bit),
                    Err(MemoryError::VersionExhausted)
                );
                assert_eq!(exchange_state(&space), after);
                assert!(space.is_code_current(&unrelated));
            }
        }
    }
}

#[test]
fn bit_test_reset_access_faults_precede_exhaustion_without_private_mutation() {
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
                0xffff_fffe,
                Access::Read,
                0xffff_fffe,
                FaultReason::AddressOverflow,
            ),
            (
                7,
                Some(3),
                0xffff_ffff,
                Access::Read,
                0xffff_ffff,
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
            for bit in [5, 37] {
                assert_eq!(
                    space.bit_test_reset32(GuestAddress(address), bit),
                    Err(fault(GuestAddress(at), access, reason))
                );
                assert_eq!(exchange_state(&space), before);
                assert!(space.is_code_current(&snapshot));
            }
            if version == u64::MAX - 1 && first == 3 {
                assert_eq!(space.bit_test_reset32(GuestAddress(0x1004), 5), Ok(0));
                assert_eq!(space.version, u64::MAX);
                assert!(space.is_code_current(&snapshot));
            }
        }
    }
}
