use super::*;

#[test]
fn fresh_private_rw_publications_preserve_only_the_executable_source_stamp() {
    let mut memory = AddressSpace::new(4).unwrap();
    memory.map_zeroed(page(0x1000), Permissions::ALL).unwrap();
    memory.write(GuestAddress(0x1000), &[0x90]).unwrap();
    paired(&memory);
    let snapshot = memory.snapshot_code(GuestAddress(0x1000), 1).unwrap();
    let source = memory.code_source_version();
    let mut version = memory.transaction_version();

    memory
        .map_zeroed(page(0x2000), Permissions::READ_WRITE)
        .unwrap();
    version += 1;
    assert_eq!(stamps(&memory), (version, version, source));
    assert_eq!(read::<4>(&memory, 0x2000), [0; 4]);
    assert_eq!(memory.mapping_version_at(GuestAddress(0x2000)), Ok(version));

    memory
        .install_pages_with_word_writes(
            &[PageInitialization {
                address: GuestAddress(0x3000),
                permissions: Permissions::READ_WRITE,
                bytes: &[7, 8],
            }],
            &[WordWrite32 {
                address: GuestAddress(0x2004),
                value: 0x4433_2211,
            }],
        )
        .unwrap();
    version += 1;
    assert_eq!(stamps(&memory), (version, version, source));
    assert_eq!(read::<4>(&memory, 0x3000), [7, 8, 0, 0]);
    assert_eq!(read::<4>(&memory, 0x2004), [0x11, 0x22, 0x33, 0x44]);
    assert_eq!(memory.mapping_version_at(GuestAddress(0x3000)), Ok(version));

    memory
        .apply_private_pages(
            &[PrivatePageChange32::Commit {
                address: GuestAddress(0x4000),
                permissions: Permissions::READ_WRITE,
            }],
            &[(GuestAddress(0x4000), &[9]), (GuestAddress(0x2008), &[10])],
        )
        .unwrap();
    version += 1;
    assert_eq!(stamps(&memory), (version, version, source));
    assert_eq!(read::<4>(&memory, 0x4000), [9, 0, 0, 0]);
    assert_eq!(read::<1>(&memory, 0x2008), [10]);
    assert_eq!(memory.mapping_version_at(GuestAddress(0x4000)), Ok(version));
    assert!(memory.is_code_current(&snapshot));

    memory.write(GuestAddress(0x2000), &[11]).unwrap();
    assert_eq!(stamps(&memory), (version + 1, version, source));
    assert!(memory.is_code_current(&snapshot));
}

#[test]
fn executable_outputs_in_fresh_rw_transactions_stamp_both_versions() {
    let mut memory = AddressSpace::new(3).unwrap();
    memory.map_zeroed(page(0x1000), Permissions::ALL).unwrap();
    let first = memory.snapshot_code(GuestAddress(0x1000), 4).unwrap();
    memory
        .install_pages_with_word_writes(
            &[PageInitialization {
                address: GuestAddress(0x2000),
                permissions: Permissions::READ_WRITE,
                bytes: &[7],
            }],
            &[WordWrite32 {
                address: GuestAddress(0x1000),
                value: 0x4433_2211,
            }],
        )
        .unwrap();
    paired(&memory);
    assert!(!memory.is_code_current(&first));
    assert_eq!(read::<4>(&memory, 0x1000), [0x11, 0x22, 0x33, 0x44]);

    let second = memory.snapshot_code(GuestAddress(0x1000), 4).unwrap();
    memory
        .apply_private_pages(
            &[PrivatePageChange32::Commit {
                address: GuestAddress(0x3000),
                permissions: Permissions::READ_WRITE,
            }],
            &[(GuestAddress(0x1001), &[0x90])],
        )
        .unwrap();
    paired(&memory);
    assert!(!memory.is_code_current(&second));
    assert_eq!(read::<4>(&memory, 0x1000), [0x11, 0x90, 0x33, 0x44]);
}

#[test]
fn protect_unmap_and_non_rw_mapping_keep_the_conservative_source_stamp() {
    let mut memory = AddressSpace::new(3).unwrap();
    memory
        .map_zeroed(page(0x1000), Permissions::READ_WRITE)
        .unwrap();
    assert_ne!(
        memory.code_validation_version(),
        memory.code_source_version()
    );
    memory
        .protect(page(0x1000), Permissions::READ_WRITE)
        .unwrap();
    paired(&memory);
    memory
        .protect_with_word_write32(
            page(0x1000),
            Permissions::READ_WRITE,
            WordWrite32 {
                address: GuestAddress(0x1000),
                value: 1,
            },
        )
        .unwrap();
    paired(&memory);
    memory
        .protect_copy_with_word_write32(
            page(0x1000),
            Permissions::READ_WRITE,
            WordWrite32 {
                address: GuestAddress(0x1000),
                value: 2,
            },
        )
        .unwrap();
    paired(&memory);
    memory
        .protect_alias_pages(&[(GuestAddress(0x1000), Permissions::READ_WRITE)])
        .unwrap();
    paired(&memory);
    memory
        .unmap_ranges_with_word_writes(&[page(0x1000)], &[])
        .unwrap();
    paired(&memory);
    memory.map_zeroed(page(0x1000), Permissions::READ).unwrap();
    paired(&memory);
    memory.unmap(page(0x1000)).unwrap();
    paired(&memory);
    memory
        .apply_private_pages(
            &[PrivatePageChange32::Commit {
                address: GuestAddress(0x1000),
                permissions: Permissions::READ_WRITE,
            }],
            &[],
        )
        .unwrap();
    memory
        .apply_private_pages(
            &[PrivatePageChange32::Protect {
                address: GuestAddress(0x1000),
                permissions: Permissions::READ_WRITE,
            }],
            &[],
        )
        .unwrap();
    paired(&memory);
    memory
        .apply_private_pages(
            &[PrivatePageChange32::Decommit {
                address: GuestAddress(0x1000),
            }],
            &[],
        )
        .unwrap();
    paired(&memory);
}

#[test]
fn shared_executable_alias_writes_and_private_cow_keep_their_distinct_source_effects() {
    for copy_on_write in [false, true] {
        let mut memory = AddressSpace::new(2).unwrap();
        let pages = memory.allocate_shared_pages(1, &[0x90]).unwrap();
        memory
            .map_shared_pages(page(0x1000), &pages, 0, Permissions::READ_EXECUTE, false)
            .unwrap();
        paired(&memory);
        memory
            .map_shared_page_rights_with_word_writes(
                page(0x3000),
                &[(&pages, 0, Permissions::READ_WRITE)],
                copy_on_write,
                &[],
            )
            .unwrap();
        paired(&memory);
        let snapshot = memory.snapshot_code(GuestAddress(0x1000), 1).unwrap();
        let (_, authority, source) = stamps(&memory);
        memory.write(GuestAddress(0x3000), &[0xcc]).unwrap();
        if copy_on_write {
            assert_eq!(stamps(&memory), (authority + 1, authority, source));
            assert!(memory.is_code_current(&snapshot));
            assert_eq!(read::<1>(&memory, 0x1000), [0x90]);
        } else {
            paired(&memory);
            assert!(!memory.is_code_current(&snapshot));
            assert_eq!(read::<1>(&memory, 0x1000), [0xcc]);
        }
        assert_eq!(read::<1>(&memory, 0x3000), [0xcc]);
        memory.release_shared_pages(pages).unwrap();
    }
}
