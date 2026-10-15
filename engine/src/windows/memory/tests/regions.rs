use super::*;

#[test]
fn sparse_reservation_commit_decommit_and_recommit_preserve_correct_page_states() {
    let mut memory = AddressSpace::new(8).unwrap();
    let mut vm = virtual_memory();
    let base = vm
        .allocate(&mut memory, &[], 0, 256 * 1024 * 1024, MEM_RESERVE, 4)
        .unwrap();
    assert_eq!(memory.mapped_pages(), 0);
    assert_eq!(vm.query(&memory, base).unwrap().state, MEM_RESERVE);
    assert_eq!(
        vm.allocate(&mut memory, &[], base, 9 * 4096, MEM_COMMIT, 4),
        Err(MemoryError32::Capacity)
    );
    assert_eq!(memory.mapped_pages(), 0);
    vm.allocate(&mut memory, &[], base + 4095, 2, MEM_COMMIT, 4)
        .unwrap();
    assert_eq!(memory.mapped_pages(), 2);
    memory.write(GuestAddress(base), &[0x5a]).unwrap();
    vm.allocate(&mut memory, &[], base, 4096, MEM_COMMIT, 2)
        .unwrap();
    assert_eq!(
        memory.permissions_at(GuestAddress(base)).unwrap(),
        Permissions::READ_WRITE
    );
    let mut bytes = [0];
    memory.read(GuestAddress(base), &mut bytes).unwrap();
    assert_eq!(bytes, [0x5a]);
    vm.free(&mut memory, base, 4096, MEM_DECOMMIT).unwrap();
    assert_eq!(vm.query(&memory, base).unwrap().state, MEM_RESERVE);
    assert_eq!(vm.query(&memory, base + 4096).unwrap().state, MEM_COMMIT);
    vm.allocate(&mut memory, &[], base, 4096, MEM_COMMIT, 4)
        .unwrap();
    memory.read(GuestAddress(base), &mut bytes).unwrap();
    assert_eq!(bytes, [0]);
    vm.free(&mut memory, base, 0, MEM_RELEASE).unwrap();
    assert!(vm.is_empty());
    assert_eq!(memory.mapped_pages(), 0);
}

#[test]
fn reserved_addresses_external_regions_and_top_down_share_one_placement_authority() {
    let mut memory = memory();
    let mut vm = virtual_memory();
    let external = PageRange::new(GuestAddress(0x1000_0000), 65536 / 4096).unwrap();
    let base = vm
        .allocate(&mut memory, &[external], 0, 65536, MEM_RESERVE, 4)
        .unwrap();
    assert_eq!(base, 0x1001_0000);
    assert_eq!(
        vm.allocate(&mut memory, &[], base + 1, 4096, MEM_RESERVE, 4),
        Err(MemoryError32::InvalidAddress)
    );
    let next = vm
        .allocate(&mut memory, &[], 0, 65536, MEM_RESERVE | MEM_TOP_DOWN, 4)
        .unwrap();
    assert_eq!(next, 0x6fff_0000);
    assert_eq!(
        vm.query(&memory, next + 4096).unwrap().allocation_base,
        next
    );
    vm.free(&mut memory, base + 4096, 0, MEM_RELEASE)
        .unwrap_err();
    assert_eq!(vm.reservation_count(), 2);
}

#[test]
fn failed_protection_output_and_exhausted_commit_leave_metadata_and_ram_unchanged() {
    let mut memory = memory();
    let mut vm = virtual_memory();
    let base = vm
        .allocate(&mut memory, &[], 0, 8192, MEM_RESERVE | MEM_COMMIT, 4)
        .unwrap();
    memory.write(GuestAddress(base), &[0x33]).unwrap();
    assert_eq!(
        vm.protect(&mut memory, base, 4096, 2, GuestAddress(0x2222)),
        Err(MemoryError32::NoAccess)
    );
    assert_eq!(vm.query(&memory, base).unwrap().protection, 4);
    vm.protect(&mut memory, base, 4096, 2, GuestAddress(base + 4096))
        .unwrap();
    let mut output = [0; 4];
    memory.read(GuestAddress(base + 4096), &mut output).unwrap();
    assert_eq!(u32::from_le_bytes(output), 4);
    let version = memory.transaction_version();
    memory.exhaust_versions_for_test();
    assert_eq!(
        vm.free(&mut memory, base, 4096, MEM_DECOMMIT),
        Err(MemoryError32::Exhausted)
    );
    assert_eq!(vm.query(&memory, base).unwrap().state, MEM_COMMIT);
    memory.restore_version_for_test(version);
    let mut byte = [0];
    memory.read(GuestAddress(base), &mut byte).unwrap();
    assert_eq!(byte, [0x33]);
}

#[test]
fn host_protection_changes_remain_authoritative_for_private_queries_and_old_protect() {
    let mut memory = memory();
    let mut vm = virtual_memory();
    let base = vm
        .allocate(&mut memory, &[], 0, 8192, MEM_RESERVE | MEM_COMMIT, 4)
        .unwrap();
    memory
        .protect(
            PageRange::new(GuestAddress(base), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    let first = vm.query(&memory, base).unwrap();
    assert_eq!(first.protection, 0x40);
    assert_eq!(first.size, 4096);
    assert_eq!(vm.query(&memory, base + 4096).unwrap().protection, 4);
    vm.protect(&mut memory, base, 4096, 2, GuestAddress(base + 4096))
        .unwrap();
    let mut bytes = [0; 4];
    memory.read(GuestAddress(base + 4096), &mut bytes).unwrap();
    assert_eq!(u32::from_le_bytes(bytes), 0x40);
}
