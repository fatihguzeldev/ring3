use super::*;

#[derive(Debug, PartialEq, Eq)]
struct Snapshot {
    backing: Vec<u8>,
    mappings: Vec<u32>,
    permissions: Vec<Permissions>,
    free: Vec<u32>,
    versions: Vec<PageVersion>,
    version: u64,
    code_version: u64,
}

fn snapshot(memory: &AddressSpace) -> Snapshot {
    Snapshot {
        backing: memory.backing.to_vec(),
        mappings: memory.mappings.to_vec(),
        permissions: memory.permissions.to_vec(),
        free: memory.free.clone(),
        versions: memory.versions.to_vec(),
        version: memory.version,
        code_version: memory.code_validation_version,
    }
}

fn page(address: u32, bytes: &[u8]) -> PageInitialization<'_> {
    PageInitialization {
        address: GuestAddress(address),
        permissions: Permissions::READ_EXECUTE,
        bytes,
    }
}

#[test]
fn installs_final_pages_and_existing_tls_words_in_one_version() {
    let mut memory = AddressSpace::new(4).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::READ_WRITE,
        )
        .unwrap();
    let identity = memory.identity();
    let version = memory.version;
    let words = [WordWrite32 {
        address: GuestAddress(0x1004),
        value: 0x3000,
    }];
    memory
        .install_pages_with_word_writes(
            &[page(0x3000, &[0xc3]), page(0xfffff000, &[0x90, 0xc3])],
            &words,
        )
        .unwrap();
    assert_eq!(memory.identity(), identity);
    assert_eq!(memory.version, version + 1);
    assert_eq!(memory.code_validation_version, version + 1);
    assert_eq!(memory.mapped_pages(), 3);
    let mut bytes = [0xff; 4096];
    memory.fetch(GuestAddress(0x3000), &mut bytes).unwrap();
    assert_eq!(bytes[0], 0xc3);
    assert!(bytes[1..].iter().all(|byte| *byte == 0));
    assert!(matches!(
        memory.write(GuestAddress(0x3000), &[0]),
        Err(MemoryError::Fault(_))
    ));
    let mut value = [0; 4];
    memory.read(GuestAddress(0x1004), &mut value).unwrap();
    assert_eq!(u32::from_le_bytes(value), 0x3000);
    let old = memory.snapshot_code(GuestAddress(0x3000), 1).unwrap();
    memory
        .unmap_ranges_with_word_writes(
            &[
                PageRange::new(GuestAddress(0x3000), 1).unwrap(),
                PageRange::new(GuestAddress(0xfffff000), 1).unwrap(),
            ],
            &[WordWrite32 {
                address: GuestAddress(0x1004),
                value: 0,
            }],
        )
        .unwrap();
    assert!(!memory.is_code_current(&old));
    assert_eq!(memory.version, version + 2);
    memory.read(GuestAddress(0x1004), &mut value).unwrap();
    assert_eq!(value, [0; 4]);
    assert_eq!(memory.mapped_pages(), 1);
}

#[test]
fn rejected_install_preserves_all_memory_and_version_state() {
    let mut memory = AddressSpace::new(3).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::READ,
        )
        .unwrap();
    let before = snapshot(&memory);
    let word = WordWrite32 {
        address: GuestAddress(0x1000),
        value: 7,
    };
    assert!(matches!(
        memory.install_pages_with_word_writes(&[page(0x3000, &[0xc3])], &[word]),
        Err(MemoryError::Fault(_))
    ));
    assert_eq!(snapshot(&memory), before);
    for pages in [
        vec![page(0x3000, &[]), page(0x3000, &[])],
        vec![page(0x3001, &[])],
        vec![page(0x3000, &[]), page(0x2000, &[])],
        vec![page(0x3000, &[]), page(0x1000, &[])],
    ] {
        assert!(memory.install_pages(&pages).is_err());
        assert_eq!(snapshot(&memory), before);
    }
    assert_eq!(
        memory.install_pages(&[page(0x2000, &[0; 4097])]),
        Err(MemoryError::InvalidRange)
    );
    assert_eq!(snapshot(&memory), before);
    assert_eq!(
        memory.install_pages(&[page(0x2000, &[]), page(0x3000, &[]), page(0x4000, &[])]),
        Err(MemoryError::Capacity)
    );
    assert_eq!(snapshot(&memory), before);
}

#[test]
fn exhausted_mapping_transactions_do_not_publish_pages_or_words() {
    let mut memory = AddressSpace::new(3).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::READ_WRITE,
        )
        .unwrap();
    memory.install_pages(&[page(0x3000, &[0xc3])]).unwrap();
    memory.exhaust_versions_for_test();
    let before = snapshot(&memory);
    let word = WordWrite32 {
        address: GuestAddress(0x1000),
        value: 7,
    };
    assert_eq!(
        memory.install_pages_with_word_writes(&[page(0x4000, &[])], &[word]),
        Err(MemoryError::VersionExhausted)
    );
    assert_eq!(snapshot(&memory), before);
    assert_eq!(
        memory.unmap_ranges_with_word_writes(
            &[PageRange::new(GuestAddress(0x3000), 1).unwrap()],
            &[word]
        ),
        Err(MemoryError::VersionExhausted)
    );
    assert_eq!(snapshot(&memory), before);
}

#[test]
fn unmap_validates_every_range_and_keeps_live_code_on_failure() {
    let mut memory = AddressSpace::new(3).unwrap();
    memory
        .install_pages(&[page(0x1000, &[0xc3]), page(0x3000, &[0xc3])])
        .unwrap();
    let code = memory.snapshot_code(GuestAddress(0x1000), 1).unwrap();
    let before = snapshot(&memory);
    let range = PageRange::new(GuestAddress(0x1000), 1).unwrap();
    assert_eq!(
        memory.unmap_ranges(&[range, PageRange::new(GuestAddress(0x5000), 1).unwrap()]),
        Err(MemoryError::NotMapped {
            address: GuestAddress(0x5000)
        })
    );
    assert_eq!(snapshot(&memory), before);
    assert_eq!(
        memory.unmap_ranges(&[range, range]),
        Err(MemoryError::InvalidRange)
    );
    assert_eq!(snapshot(&memory), before);
    assert!(memory.is_code_current(&code));
}

#[test]
fn write_overlaps_cannot_hide_inside_install_or_unmap() {
    let mut memory = AddressSpace::new(4).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 2).unwrap(),
            Permissions::READ_WRITE,
        )
        .unwrap();
    let before = snapshot(&memory);
    let writes = [
        WordWrite32 {
            address: GuestAddress(0x1000),
            value: 1,
        },
        WordWrite32 {
            address: GuestAddress(0x1002),
            value: 2,
        },
    ];
    assert_eq!(
        memory.install_pages_with_word_writes(&[page(0x3000, &[])], &writes),
        Err(MemoryError::InvalidRange)
    );
    assert_eq!(snapshot(&memory), before);
    let cross = [WordWrite32 {
        address: GuestAddress(0x1ffe),
        value: 1,
    }];
    assert_eq!(
        memory.unmap_ranges_with_word_writes(
            &[PageRange::new(GuestAddress(0x2000), 1).unwrap()],
            &cross
        ),
        Err(MemoryError::InvalidRange)
    );
    assert_eq!(snapshot(&memory), before);
    assert!(
        memory
            .install_pages_with_word_writes(
                &[page(0x3000, &[])],
                &[WordWrite32 {
                    address: GuestAddress(0x3000),
                    value: 1
                }]
            )
            .is_err()
    );
    assert_eq!(snapshot(&memory), before);
}

#[test]
fn reused_physical_slots_zero_only_the_uninitialized_tail() {
    let mut memory = AddressSpace::new(1).unwrap();
    memory
        .install_pages(&[page(0x1000, &[0xa5; 4096])])
        .unwrap();
    memory
        .unmap_ranges(&[PageRange::new(GuestAddress(0x1000), 1).unwrap()])
        .unwrap();
    memory.install_pages(&[page(0x2000, &[0xc3])]).unwrap();
    let mut bytes = [0xff; 4096];
    memory.fetch(GuestAddress(0x2000), &mut bytes).unwrap();
    assert_eq!(bytes[0], 0xc3);
    assert!(bytes[1..].iter().all(|byte| *byte == 0));
}
