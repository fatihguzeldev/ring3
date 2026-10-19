use super::*;

fn range(address: u32, pages: u32) -> PageRange {
    PageRange::new(GuestAddress(address), pages).unwrap()
}

#[derive(PartialEq)]
struct Saved {
    backing: Vec<u8>,
    mappings: Vec<u32>,
    permissions: Vec<Permissions>,
    free: Vec<u32>,
    versions: Vec<PageVersion>,
    identity: u64,
    version: u64,
    code_version: u64,
    source_version: u64,
}

fn saved(space: &AddressSpace) -> Saved {
    Saved {
        backing: space.backing.to_vec(),
        mappings: space.mappings.to_vec(),
        permissions: space.permissions.to_vec(),
        free: space.free.clone(),
        versions: space.versions.to_vec(),
        identity: space.identity,
        version: space.version,
        code_version: space.code_validation_version(),
        source_version: space.code_source_version(),
    }
}

#[test]
fn permission_query_observes_mapped_pages_without_requiring_access() {
    let mut space = AddressSpace::new(3).unwrap();
    for (address, permissions) in [
        (0x1000, Permissions::NONE),
        (0x2000, Permissions::EXECUTE),
        (0x3000, Permissions::from_bits(2).unwrap()),
    ] {
        space.map_zeroed(range(address, 1), permissions).unwrap();
    }
    let before = saved(&space);
    for (address, permissions) in [
        (0x1fff, Permissions::NONE),
        (0x2001, Permissions::EXECUTE),
        (0x3fff, Permissions::from_bits(2).unwrap()),
    ] {
        assert_eq!(space.permissions_at(GuestAddress(address)), Ok(permissions));
    }
    assert_eq!(
        space.permissions_at(GuestAddress(0x4123)),
        Err(MemoryError::NotMapped {
            address: GuestAddress(0x4000),
        })
    );
    assert!(saved(&space) == before, "permission query mutated memory");
}

#[test]
fn mixed_pages_and_cross_page_output_commit_using_the_last_version_once() {
    let mut space = AddressSpace::new(5).unwrap();
    space
        .map_zeroed(range(0x1000, 1), Permissions::EXECUTE)
        .unwrap();
    space
        .map_zeroed(range(0x2000, 1), Permissions::NONE)
        .unwrap();
    space
        .map_zeroed(range(0x3000, 1), Permissions::ALL)
        .unwrap();
    space
        .map_zeroed(range(0x4000, 1), Permissions::READ_WRITE)
        .unwrap();
    space
        .map_zeroed(range(0x5000, 1), Permissions::EXECUTE)
        .unwrap();
    let target = space.snapshot_code(GuestAddress(0x1000), 1).unwrap();
    let output = space.snapshot_code(GuestAddress(0x3ffe), 2).unwrap();
    let unrelated = space.snapshot_code(GuestAddress(0x5000), 1).unwrap();
    let mappings = space.mappings.to_vec();
    let free = space.free.clone();
    let old_content = [0x1000, 0x2000].map(|address| {
        let slot = space.mappings[address / PAGE_SIZE as usize] as usize - 1;
        space.versions[slot].content
    });
    space.version = u64::MAX - 1;
    space
        .protect_with_word_write32(
            range(0x1000, 2),
            Permissions::READ,
            WordWrite32 {
                address: GuestAddress(0x3ffe),
                value: 0x4433_2211,
            },
        )
        .unwrap();
    assert_eq!(space.version, u64::MAX);
    assert_eq!(space.code_validation_version(), u64::MAX);
    assert_eq!(space.mappings.as_ref(), mappings);
    assert_eq!(space.free, free);
    assert_eq!(space.mapped_pages(), 5);
    for (index, address) in [0x1000, 0x2000].into_iter().enumerate() {
        assert_eq!(
            space.permissions_at(GuestAddress(address as u32)),
            Ok(Permissions::READ)
        );
        let slot = space.mappings[address / PAGE_SIZE as usize] as usize - 1;
        assert_eq!(
            space.versions[slot],
            PageVersion {
                mapping: u64::MAX,
                content: old_content[index]
            }
        );
    }
    for address in [0x3000, 0x4000] {
        let slot = space.mappings[address / PAGE_SIZE as usize] as usize - 1;
        assert_eq!(space.versions[slot].content, u64::MAX);
    }
    let mut bytes = [0; 8];
    space.read(GuestAddress(0x3ffc), &mut bytes).unwrap();
    assert_eq!(bytes, [0, 0, 0x11, 0x22, 0x33, 0x44, 0, 0]);
    assert!(!space.is_code_current(&target));
    assert!(!space.is_code_current(&output));
    assert!(space.is_code_current(&unrelated));
    let before = saved(&space);
    assert_eq!(
        space.protect_with_word_write32(
            range(0x1000, 2),
            Permissions::NONE,
            WordWrite32 {
                address: GuestAddress(0x3ffe),
                value: 0
            },
        ),
        Err(MemoryError::VersionExhausted)
    );
    assert!(
        saved(&space) == before,
        "exhausted transaction mutated memory"
    );
    assert!(space.is_code_current(&unrelated));
}

#[test]
fn transaction_preflights_every_target_and_output_page_before_mutation() {
    for failure in 0..3 {
        let mut space = AddressSpace::new(4).unwrap();
        space
            .map_zeroed(range(0x1000, 1), Permissions::ALL)
            .unwrap();
        space
            .map_zeroed(range(0x3000, 1), Permissions::ALL)
            .unwrap();
        space.write(GuestAddress(0x3ffc), &[0x55; 4]).unwrap();
        if failure != 0 {
            space
                .map_zeroed(range(0x2000, 1), Permissions::NONE)
                .unwrap();
        }
        if failure == 2 {
            space
                .map_zeroed(range(0x4000, 1), Permissions::READ)
                .unwrap();
        }
        let target = space.snapshot_code(GuestAddress(0x1000), 1).unwrap();
        let output = space.snapshot_code(GuestAddress(0x3ffc), 4).unwrap();
        space.version = u64::MAX - 1;
        let before = saved(&space);
        let error = match failure {
            0 => MemoryError::NotMapped {
                address: GuestAddress(0x2000),
            },
            1 => fault(GuestAddress(0x4000), Access::Write, FaultReason::Unmapped),
            _ => fault(GuestAddress(0x4000), Access::Write, FaultReason::Permission),
        };
        assert_eq!(
            space.protect_with_word_write32(
                range(0x1000, 2),
                Permissions::NONE,
                WordWrite32 {
                    address: GuestAddress(0x3ffe),
                    value: 0x4433_2211
                },
            ),
            Err(error)
        );
        assert!(saved(&space) == before, "preflight failure mutated memory");
        assert!(space.is_code_current(&target));
        assert!(space.is_code_current(&output));
    }
}

#[test]
fn output_overflow_precedes_output_mapping_and_permissions_without_mutation() {
    for map_top in [false, true] {
        let mut space = AddressSpace::new(3).unwrap();
        space
            .map_zeroed(range(0x1000, 1), Permissions::ALL)
            .unwrap();
        space
            .map_zeroed(range(0, 1), Permissions::READ_WRITE)
            .unwrap();
        space.write(GuestAddress(0), &[0x5a, 0xa5]).unwrap();
        if map_top {
            space
                .map_zeroed(range(0xffff_f000, 1), Permissions::NONE)
                .unwrap();
        }
        space.version = u64::MAX - 1;
        let before = saved(&space);
        assert_eq!(
            space.protect_with_word_write32(
                range(0x1000, 1),
                Permissions::NONE,
                WordWrite32 {
                    address: GuestAddress(0xffff_fffe),
                    value: 0x4433_2211
                },
            ),
            Err(fault(
                GuestAddress(0xffff_fffe),
                Access::Write,
                FaultReason::AddressOverflow
            ))
        );
        assert!(saved(&space) == before, "overflow failure mutated memory");
    }
}

#[test]
fn identical_protection_invalidates_target_and_executable_output_only() {
    let mut space = AddressSpace::new(3).unwrap();
    space
        .map_zeroed(range(0x1000, 1), Permissions::READ_EXECUTE)
        .unwrap();
    space
        .map_zeroed(range(0x3000, 1), Permissions::ALL)
        .unwrap();
    space
        .map_zeroed(range(0x5000, 1), Permissions::EXECUTE)
        .unwrap();
    let target = space.snapshot_code(GuestAddress(0x1000), 1).unwrap();
    let output = space.snapshot_code(GuestAddress(0x3000), 4).unwrap();
    let unrelated = space.snapshot_code(GuestAddress(0x5000), 1).unwrap();
    let version = space.version;
    space
        .protect_with_word_write32(
            range(0x1000, 1),
            Permissions::READ_EXECUTE,
            WordWrite32 {
                address: GuestAddress(0x3000),
                value: 0,
            },
        )
        .unwrap();
    assert_eq!(space.version, version + 1);
    assert!(!space.is_code_current(&target));
    assert!(!space.is_code_current(&output));
    assert!(space.is_code_current(&unrelated));
    assert_eq!(
        space.permissions_at(GuestAddress(0x1000)),
        Ok(Permissions::READ_EXECUTE)
    );
    let mut bytes = [0xa5; 4];
    space.read(GuestAddress(0x3000), &mut bytes).unwrap();
    assert_eq!(bytes, [0; 4]);
}
