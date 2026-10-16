use super::*;

fn arena(pages: u32) -> GuestComObjects32 {
    GuestComObjects32::new(
        PageRange::new(GuestAddress(FLOOR), pages).unwrap(),
        8,
        32,
        2,
    )
    .unwrap()
}

#[test]
fn released_objects_reuse_a_bounded_arena_across_many_more_lifetimes_than_pages() {
    let mut memory = AddressSpace::new(4).unwrap();
    let mut objects = arena(4);
    let mut previous_id = 0;
    let mut addresses = Vec::new();
    for payload in 0..64 {
        let identity = objects.create(&mut memory, payload, &[layout(IA)]).unwrap();
        assert_eq!(identity.id.get(), previous_id + 1);
        assert_eq!(
            objects.preflight(&memory, identity.identity_pointer),
            Ok(identity)
        );
        assert!((FLOOR + PAGE_SIZE..FLOOR + 4 * PAGE_SIZE).contains(&identity.identity_pointer.0));
        assert_eq!(read_word(&memory, identity.identity_pointer.0), FLOOR);
        assert_eq!(read_word(&memory, FLOOR), METHODS[0].0);
        assert_eq!(memory.mapped_pages(), 2);
        assert_eq!(objects.live_objects(), 1);
        addresses.push(identity.identity_pointer);
        previous_id = identity.id.get();
        let outcome = objects
            .release(&mut memory, identity.identity_pointer)
            .unwrap();
        assert_eq!(outcome.destroyed, Some(identity));
        assert_eq!(objects.live_objects(), 0);
        assert_eq!(objects.retained_pages(), 1);
        assert_eq!(memory.mapped_pages(), 1);
    }
    assert_eq!(addresses[0], addresses[3]);
}

#[test]
fn wrap_skips_live_objects_vtables_and_foreign_mappings_and_full_failure_is_neutral() {
    let mut memory = AddressSpace::new(8).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x8000), 1).unwrap(),
            Permissions::READ_WRITE,
        )
        .unwrap();
    memory.write(GuestAddress(0x8000), &[0xa5; 4]).unwrap();
    let mut objects = arena(6);
    let keeper = objects.create(&mut memory, 1, &[layout(IA)]).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(FLOOR + 4 * PAGE_SIZE), 1).unwrap(),
            Permissions::READ_WRITE,
        )
        .unwrap();
    memory
        .write(GuestAddress(FLOOR + 4 * PAGE_SIZE), &[0x5a; 4])
        .unwrap();
    for page in [2, 3, 5] {
        let temporary = objects.create(&mut memory, 2, &[layout(IA)]).unwrap();
        assert_eq!(temporary.identity_pointer.0, FLOOR + page * PAGE_SIZE);
        objects
            .release(&mut memory, temporary.identity_pointer)
            .unwrap();
    }
    let wrapped = objects.create(&mut memory, 3, &[layout(IA)]).unwrap();
    let middle = objects.create(&mut memory, 4, &[layout(IA)]).unwrap();
    let last = objects.create(&mut memory, 5, &[layout(IA)]).unwrap();
    assert_eq!(wrapped.identity_pointer.0, FLOOR + 2 * PAGE_SIZE);
    assert_eq!(middle.identity_pointer.0, FLOOR + 3 * PAGE_SIZE);
    assert_eq!(last.identity_pointer.0, FLOOR + 5 * PAGE_SIZE);
    let before = memory.transaction_version();
    let cursor = objects.high_water_address();
    assert!(matches!(
        objects.create_with_output(&mut memory, 6, &[layout(IA)], GuestAddress(0x8000), IA),
        Err(GuestComError32::Capacity | GuestComError32::Memory(MemoryError::Capacity))
    ));
    assert_eq!(memory.transaction_version(), before);
    assert_eq!(objects.high_water_address(), cursor);
    assert_eq!(objects.live_objects(), 4);
    assert_eq!(memory.mapped_pages(), 7);
    assert_eq!(read_word(&memory, 0x8000), 0xa5a5_a5a5);
    assert_eq!(read_word(&memory, FLOOR + 4 * PAGE_SIZE), 0x5a5a5a5a);
    assert_eq!(read_word(&memory, FLOOR), METHODS[0].0);
    for identity in [keeper, wrapped, middle, last] {
        assert_eq!(
            objects.preflight(&memory, identity.identity_pointer),
            Ok(identity)
        );
    }
}

#[test]
fn wrap_output_failure_keeps_the_cursor_id_and_reclaimed_page_unpublished() {
    let mut memory = AddressSpace::new(4).unwrap();
    let mut objects = arena(3);
    let first = objects.create(&mut memory, 1, &[layout(IA)]).unwrap();
    objects
        .release(&mut memory, first.identity_pointer)
        .unwrap();
    let last = objects.create(&mut memory, 2, &[layout(IA)]).unwrap();
    objects.release(&mut memory, last.identity_pointer).unwrap();
    let before = memory.transaction_version();
    let cursor = objects.high_water_address();
    assert!(matches!(
        objects.create_with_output(&mut memory, 3, &[layout(IA)], GuestAddress(0x9000), IA),
        Err(GuestComError32::Memory(MemoryError::Fault(_)))
    ));
    assert_eq!(memory.transaction_version(), before);
    assert_eq!(objects.high_water_address(), cursor);
    assert_eq!(objects.live_objects(), 0);
    assert_eq!(memory.mapped_pages(), 1);
    assert!(memory.permissions_at(first.identity_pointer).is_err());
    let replacement = objects.create(&mut memory, 4, &[layout(IA)]).unwrap();
    assert_eq!(replacement.identity_pointer, first.identity_pointer);
    assert_eq!(replacement.id.get(), last.id.get() + 1);
}

#[test]
fn wrapped_storage_rejects_stale_lifetime_and_retain_receipts_for_the_old_identity() {
    let mut memory = AddressSpace::new(4).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x8000), 1).unwrap(),
            Permissions::READ_WRITE,
        )
        .unwrap();
    memory.write(GuestAddress(0x8000), &[0xa5; 4]).unwrap();
    let mut objects = arena(3);
    let first = objects.create(&mut memory, 1, &[layout(IA)]).unwrap();
    let lifetime = objects
        .prepare_lifetime_changes(
            &memory,
            &[],
            &[first.identity_pointer],
            &[],
            &[],
            &[crate::memory::WordWrite32 {
                address: GuestAddress(0x8000),
                value: 7,
            }],
        )
        .unwrap();
    let retain = objects
        .prepare_lifetime_changes(&memory, &[first.identity_pointer], &[], &[], &[], &[])
        .unwrap();
    let old_mapping = memory.mapping_version_at(first.identity_pointer).unwrap();
    objects
        .release(&mut memory, first.identity_pointer)
        .unwrap();
    let last = objects.create(&mut memory, 2, &[layout(IA)]).unwrap();
    objects.release(&mut memory, last.identity_pointer).unwrap();
    let replacement = objects.create(&mut memory, 3, &[layout(IA)]).unwrap();
    assert_eq!(replacement.identity_pointer, first.identity_pointer);
    assert_ne!(replacement.id, first.id);
    assert!(
        memory
            .mapping_version_at(replacement.identity_pointer)
            .unwrap()
            > old_mapping
    );
    let before = memory.transaction_version();
    assert_eq!(
        objects.commit_lifetime_changes(&mut memory, lifetime),
        Err(GuestComError32::StaleMapping)
    );
    assert_eq!(
        objects.can_commit_lifetime_changes(&memory, &retain),
        Err(GuestComError32::StaleMapping)
    );
    assert_eq!(memory.transaction_version(), before);
    assert_eq!(
        objects.preflight(&memory, replacement.identity_pointer),
        Ok(replacement)
    );
    assert_eq!(objects.references(replacement.identity_pointer), Ok(1));
    assert_eq!(read_word(&memory, 0x8000), 0xa5a5_a5a5);
}

#[test]
fn externally_unmapped_live_object_storage_is_not_reclaimed_by_wrap() {
    let mut memory = AddressSpace::new(4).unwrap();
    let mut objects = arena(4);
    let live = objects.create(&mut memory, 1, &[layout(IA)]).unwrap();
    memory
        .unmap(PageRange::new(live.identity_pointer, 1).unwrap())
        .unwrap();
    for _ in 0..2 {
        let temporary = objects.create(&mut memory, 2, &[layout(IA)]).unwrap();
        objects
            .release(&mut memory, temporary.identity_pointer)
            .unwrap();
    }
    let replacement = objects.create(&mut memory, 3, &[layout(IA)]).unwrap();
    assert_eq!(replacement.identity_pointer.0, FLOOR + 2 * PAGE_SIZE);
    assert_eq!(objects.lookup(live.identity_pointer), Ok(live));
    assert!(objects.preflight(&memory, live.identity_pointer).is_err());
    assert!(memory.permissions_at(live.identity_pointer).is_err());
    assert_eq!(
        objects.preflight(&memory, replacement.identity_pointer),
        Ok(replacement)
    );
}

#[test]
fn externally_unmapped_retained_vtable_storage_is_not_reclaimed_by_wrap() {
    let mut memory = AddressSpace::new(4).unwrap();
    let mut objects = arena(4);
    for _ in 0..3 {
        let temporary = objects.create(&mut memory, 1, &[layout(IA)]).unwrap();
        objects
            .release(&mut memory, temporary.identity_pointer)
            .unwrap();
    }
    memory
        .unmap(PageRange::new(GuestAddress(FLOOR), 1).unwrap())
        .unwrap();
    let replacement = objects.create(&mut memory, 2, &[layout(IB)]).unwrap();
    assert_eq!(replacement.identity_pointer.0, FLOOR + 2 * PAGE_SIZE);
    assert_eq!(
        read_word(&memory, replacement.identity_pointer.0),
        FLOOR + PAGE_SIZE
    );
    assert!(memory.permissions_at(GuestAddress(FLOOR)).is_err());
    assert_eq!(
        objects.preflight(&memory, replacement.identity_pointer),
        Ok(replacement)
    );
    let before = memory.transaction_version();
    assert_eq!(
        objects.create(&mut memory, 3, &[layout(IA)]),
        Err(GuestComError32::Memory(MemoryError::NotMapped {
            address: GuestAddress(FLOOR)
        }))
    );
    assert_eq!(memory.transaction_version(), before);
}
