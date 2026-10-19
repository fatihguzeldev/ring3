use super::*;

#[test]
fn fresh_publication_preflight_and_exhaustion_leave_both_stamps_and_ram_unchanged() {
    let mut memory = AddressSpace::new(4).unwrap();
    memory.map_zeroed(page(0x1000), Permissions::ALL).unwrap();
    memory
        .map_zeroed(page(0x2000), Permissions::READ_WRITE)
        .unwrap();
    memory
        .write(GuestAddress(0x1000), &[0x90, 0x90, 0x90, 0xc3])
        .unwrap();
    memory.write(GuestAddress(0x2000), &[1, 2, 3, 4]).unwrap();
    let snapshot = memory.snapshot_code(GuestAddress(0x1000), 4).unwrap();
    let before = stamps(&memory);
    let mapped = memory.mapped_pages();
    let code = read::<4>(&memory, 0x1000);
    let data = read::<4>(&memory, 0x2000);
    let fresh = [PageInitialization {
        address: GuestAddress(0x3000),
        permissions: Permissions::READ_WRITE,
        bytes: &[7],
    }];
    let change = [PrivatePageChange32::Commit {
        address: GuestAddress(0x3000),
        permissions: Permissions::READ_WRITE,
    }];
    assert!(
        memory
            .map_zeroed(page(0x1000), Permissions::READ_WRITE)
            .is_err()
    );
    assert_eq!(
        memory.map_zeroed(
            PageRange::new(GuestAddress(0x3000), 3).unwrap(),
            Permissions::READ_WRITE
        ),
        Err(MemoryError::Capacity)
    );
    assert!(
        memory
            .install_pages_with_word_writes(
                &fresh,
                &[WordWrite32 {
                    address: GuestAddress(0xdead_0000),
                    value: 1
                }]
            )
            .is_err()
    );
    assert!(
        memory
            .apply_private_pages(
                &change,
                &[
                    (GuestAddress(0x1000), &[0xcc]),
                    (GuestAddress(0xdead_0000), &[1])
                ]
            )
            .is_err()
    );
    assert_eq!(stamps(&memory), before);
    assert_eq!(memory.mapped_pages(), mapped);
    assert_eq!(read::<4>(&memory, 0x1000), code);
    assert_eq!(read::<4>(&memory, 0x2000), data);
    assert!(memory.is_code_current(&snapshot));

    memory.exhaust_versions_for_test();
    let exhausted = stamps(&memory);
    assert_eq!(
        memory.map_zeroed(page(0x3000), Permissions::READ_WRITE),
        Err(MemoryError::VersionExhausted)
    );
    assert_eq!(
        memory.install_pages_with_word_writes(&fresh, &[]),
        Err(MemoryError::VersionExhausted)
    );
    assert_eq!(
        memory.apply_private_pages(&change, &[(GuestAddress(0x3000), &[7])]),
        Err(MemoryError::VersionExhausted)
    );
    assert_eq!(stamps(&memory), exhausted);
    assert_eq!(memory.mapped_pages(), mapped);
    assert_eq!(read::<4>(&memory, 0x1000), code);
    assert_eq!(read::<4>(&memory, 0x2000), data);
    assert!(memory.is_code_current(&snapshot));
}
