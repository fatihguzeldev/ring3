use super::*;

#[test]
fn alias_mapping_and_pointer_output_share_one_transaction_and_failed_output_is_neutral() {
    let mut memory = AddressSpace::new(4).unwrap();
    let pages = memory.allocate_shared_pages(1, &[7]).unwrap();
    memory
        .map_zeroed(range(0x1000, 1), Permissions::READ_WRITE)
        .unwrap();
    let version = memory.transaction_version();
    let mapped = memory.mapped_pages();
    assert!(
        memory
            .map_shared_page_rights_with_word_writes(
                range(0x5000, 1),
                &[(&pages, 0, Permissions::READ_WRITE)],
                true,
                &[WordWrite32 {
                    address: GuestAddress(0xdead0000),
                    value: 0x5004
                }],
            )
            .is_err()
    );
    assert_eq!(memory.transaction_version(), version);
    assert_eq!(memory.mapped_pages(), mapped);
    assert!(memory.private_slots.is_none());
    memory
        .map_shared_page_rights_with_word_writes(
            range(0x5000, 1),
            &[(&pages, 0, Permissions::READ_WRITE)],
            true,
            &[WordWrite32 {
                address: GuestAddress(0x1000),
                value: 0x5004,
            }],
        )
        .unwrap();
    assert_eq!(memory.transaction_version(), version + 1);
    let mut pointer = [0; 4];
    memory.read(GuestAddress(0x1000), &mut pointer).unwrap();
    assert_eq!(u32::from_le_bytes(pointer), 0x5004);
    let mut byte = [0];
    memory.read(GuestAddress(0x5000), &mut byte).unwrap();
    assert_eq!(byte, [7]);
    memory.write(GuestAddress(0x5000), &[9]).unwrap();
    memory.read_shared_pages(&pages, 0, &mut byte).unwrap();
    assert_eq!(byte, [7]);
}

fn range(address: u32, count: u32) -> PageRange {
    PageRange::new(GuestAddress(address), count).unwrap()
}

#[test]
fn shared_views_have_independent_rights_and_survive_owner_release() {
    let mut memory = AddressSpace::new(1).unwrap();
    let pages = memory.allocate_shared_pages(1, &[7]).unwrap();
    memory
        .map_shared_pages(range(0x1000, 1), &pages, 0, Permissions::READ_WRITE, false)
        .unwrap();
    memory
        .map_shared_pages(range(0x3000, 1), &pages, 0, Permissions::READ, false)
        .unwrap();
    memory.release_shared_pages(pages).unwrap();
    memory.write(GuestAddress(0x1000), &[9]).unwrap();
    let mut value = [0];
    memory.read(GuestAddress(0x3000), &mut value).unwrap();
    assert_eq!(value, [9]);
    assert!(memory.write(GuestAddress(0x3000), &[4]).is_err());
    memory.protect(range(0x1000, 1), Permissions::NONE).unwrap();
    assert_eq!(
        memory.permissions_at(GuestAddress(0x3000)),
        Ok(Permissions::READ)
    );
    memory.unmap(range(0x1000, 1)).unwrap();
    assert_eq!(memory.mapped_pages(), 1);
    memory.read(GuestAddress(0x3000), &mut value).unwrap();
    assert_eq!(value, [9]);
    memory.unmap(range(0x3000, 1)).unwrap();
    assert_eq!(memory.mapped_pages(), 0);
}

#[test]
fn writes_through_nonexecutable_alias_invalidate_executable_code() {
    let mut memory = AddressSpace::new(1).unwrap();
    let pages = memory.allocate_shared_pages(1, &[0x90]).unwrap();
    memory
        .map_shared_pages(range(0x1000, 1), &pages, 0, Permissions::READ_WRITE, false)
        .unwrap();
    memory
        .map_shared_pages(
            range(0x3000, 1),
            &pages,
            0,
            Permissions::READ_EXECUTE,
            false,
        )
        .unwrap();
    let code = memory.snapshot_code(GuestAddress(0x3000), 1).unwrap();
    let before = memory.code_validation_version();
    memory.write(GuestAddress(0x1000), &[0xc3]).unwrap();
    assert!(memory.code_validation_version() > before);
    assert!(!memory.is_code_current(&code));
    let mut instruction = [0];
    memory
        .fetch(GuestAddress(0x3000), &mut instruction)
        .unwrap();
    assert_eq!(instruction, [0xc3]);
}

#[test]
fn copy_on_write_tracks_shared_changes_until_each_page_first_write() {
    let mut memory = AddressSpace::new(4).unwrap();
    let pages = memory.allocate_shared_pages(2, &[1]).unwrap();
    memory
        .map_shared_pages(range(0x1000, 2), &pages, 0, Permissions::READ_WRITE, false)
        .unwrap();
    memory
        .map_shared_pages(range(0x5000, 2), &pages, 0, Permissions::ALL, true)
        .unwrap();
    assert_eq!(
        memory.mapped_pages(),
        4,
        "private commit capacity is reserved for every copy page"
    );
    memory.write(GuestAddress(0x1000), &[2]).unwrap();
    let mut value = [0];
    memory.read(GuestAddress(0x5000), &mut value).unwrap();
    assert_eq!(value, [2], "copy view remains coherent before first write");
    memory.write(GuestAddress(0x5001), &[3]).unwrap();
    memory.write(GuestAddress(0x1000), &[4]).unwrap();
    memory.read(GuestAddress(0x5000), &mut value).unwrap();
    assert_eq!(value, [2], "whole copy page is now private");
    memory.write(GuestAddress(0x2000), &[5]).unwrap();
    memory.read(GuestAddress(0x6000), &mut value).unwrap();
    assert_eq!(value, [5], "another copy page still aliases shared backing");
    let snapshot = memory.snapshot_code(GuestAddress(0x6000), 1).unwrap();
    memory.write(GuestAddress(0x6000), &[6]).unwrap();
    assert!(!memory.is_code_current(&snapshot));
    let mut shared = [0];
    memory
        .read_shared_pages(&pages, PAGE_SIZE as usize, &mut shared)
        .unwrap();
    assert_eq!(
        shared,
        [5],
        "private bytes never enter flushable shared backing"
    );
    memory.unmap(range(0x5000, 2)).unwrap();
    assert_eq!(memory.mapped_pages(), 2);
}

#[test]
fn failed_copy_map_and_failed_cross_page_write_are_failure_neutral() {
    let mut memory = AddressSpace::new(2).unwrap();
    let pages = memory.allocate_shared_pages(2, &[1]).unwrap();
    let version = memory.transaction_version();
    assert_eq!(
        memory.map_shared_pages(range(0x1000, 2), &pages, 0, Permissions::READ_WRITE, true),
        Err(MemoryError::Capacity)
    );
    assert_eq!(memory.transaction_version(), version);
    assert_eq!(memory.mappings[1], 0);
    assert_eq!(memory.references.to_vec(), vec![1, 1]);
    memory
        .map_shared_pages(range(0x1000, 1), &pages, 0, Permissions::READ_WRITE, false)
        .unwrap();
    let before = memory.transaction_version();
    assert!(memory.write(GuestAddress(0x1fff), &[8, 9]).is_err());
    assert_eq!(memory.transaction_version(), before);
    let mut value = [0];
    memory
        .read_shared_pages(&pages, PAGE_SIZE as usize - 1, &mut value)
        .unwrap();
    assert_eq!(value, [0]);
}

#[test]
fn alias_unmap_and_remap_never_revive_an_old_code_snapshot() {
    let mut memory = AddressSpace::new(1).unwrap();
    let pages = memory.allocate_shared_pages(1, &[0x90]).unwrap();
    memory
        .map_shared_pages(
            range(0x1000, 1),
            &pages,
            0,
            Permissions::READ_EXECUTE,
            false,
        )
        .unwrap();
    let old = memory.snapshot_code(GuestAddress(0x1000), 1).unwrap();
    let mapping = memory.mapping_version_at(GuestAddress(0x1000)).unwrap();
    memory.unmap(range(0x1000, 1)).unwrap();
    memory
        .map_shared_pages(
            range(0x1000, 1),
            &pages,
            0,
            Permissions::READ_EXECUTE,
            false,
        )
        .unwrap();
    assert!(!memory.is_code_current(&old));
    assert_ne!(
        memory.mapping_version_at(GuestAddress(0x1000)).unwrap(),
        mapping
    );
}

#[test]
fn heterogeneous_aliases_publish_together_and_keep_other_views_code_current() {
    let mut memory = AddressSpace::new(2).unwrap();
    let first = memory.allocate_shared_pages(1, &[0x90]).unwrap();
    let prepared = memory.prepare_shared_pages(1, &[7]).unwrap();
    let second = memory.commit_shared_pages(prepared);
    memory
        .map_shared_pages(
            range(0x1000, 1),
            &first,
            0,
            Permissions::READ_EXECUTE,
            false,
        )
        .unwrap();
    let code = memory.snapshot_code(GuestAddress(0x1000), 1).unwrap();
    let version = memory.transaction_version();
    memory
        .map_shared_page_list(
            range(0x4000, 2),
            &[(&first, 0), (&second, 0)],
            Permissions::READ_WRITE,
            false,
        )
        .unwrap();
    assert_eq!(memory.transaction_version(), version + 1);
    assert!(
        memory.is_code_current(&code),
        "adding another alias does not replace the existing view mapping identity"
    );
    memory.write(GuestAddress(0x4000), &[0xc3]).unwrap();
    assert!(!memory.is_code_current(&code));
}

#[test]
fn shared_protection_becomes_private_on_write_atomically_with_output_and_capacity() {
    for capacity in [2, 3] {
        let mut memory = AddressSpace::new(capacity).unwrap();
        memory
            .map_zeroed(range(0x8000, 1), Permissions::READ_WRITE)
            .unwrap();
        let pages = memory.allocate_shared_pages(1, &[1]).unwrap();
        memory
            .map_shared_pages(range(0x1000, 1), &pages, 0, Permissions::READ_WRITE, false)
            .unwrap();
        memory
            .map_shared_pages(range(0x3000, 1), &pages, 0, Permissions::READ, false)
            .unwrap();
        let before = memory.transaction_version();
        let output = super::super::WordWrite32 {
            address: GuestAddress(0x8000),
            value: 4,
        };
        let result = memory.protect_copy_with_word_write32(
            range(0x1000, 1),
            Permissions::READ_WRITE,
            output,
        );
        if capacity == 2 {
            assert_eq!(result, Err(MemoryError::Capacity));
            assert_eq!(memory.transaction_version(), before);
            let mut word = [0; 4];
            memory.read(GuestAddress(0x8000), &mut word).unwrap();
            assert_eq!(u32::from_le_bytes(word), 0);
            memory.write(GuestAddress(0x1000), &[2]).unwrap();
            let mut bytes = [0];
            memory.read(GuestAddress(0x3000), &mut bytes).unwrap();
            assert_eq!(bytes, [2]);
        } else {
            result.unwrap();
            let mut word = [0; 4];
            memory.read(GuestAddress(0x8000), &mut word).unwrap();
            assert_eq!(u32::from_le_bytes(word), 4);
            memory.write(GuestAddress(0x1000), &[3]).unwrap();
            let mut bytes = [0];
            memory.read(GuestAddress(0x3000), &mut bytes).unwrap();
            assert_eq!(bytes, [1]);
            memory.read(GuestAddress(0x1000), &mut bytes).unwrap();
            assert_eq!(bytes, [3]);
            memory
                .protect_copy_with_word_write32(range(0x1000, 1), Permissions::READ_WRITE, output)
                .unwrap();
            assert_eq!(
                memory.mapped_pages(),
                3,
                "already private pages require no extra commit capacity"
            );
        }
    }
}

#[test]
fn invalid_copy_protection_output_never_consumes_private_commit_capacity() {
    let mut memory = AddressSpace::new(2).unwrap();
    let pages = memory.allocate_shared_pages(1, &[7]).unwrap();
    memory
        .map_shared_pages(range(0x1000, 1), &pages, 0, Permissions::READ_WRITE, false)
        .unwrap();
    let before = memory.transaction_version();
    assert!(
        memory
            .protect_copy_with_word_write32(
                range(0x1000, 1),
                Permissions::READ_WRITE,
                super::super::WordWrite32 {
                    address: GuestAddress(0xdead0000),
                    value: 4
                }
            )
            .is_err()
    );
    assert_eq!(memory.transaction_version(), before);
    assert_eq!(memory.mapped_pages(), 1);
    assert!(memory.private_slots.is_none());
}
