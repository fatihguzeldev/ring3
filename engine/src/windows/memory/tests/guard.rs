use super::*;

#[test]
fn virtual_guard_is_queryable_once_and_restores_the_exact_underlying_rights() {
    let mut memory = memory();
    let mut vm = virtual_memory();
    let base = vm
        .allocate(
            &mut memory,
            &[],
            0,
            8192,
            MEM_RESERVE | MEM_COMMIT,
            2 | PAGE_GUARD,
        )
        .unwrap();
    assert_eq!(vm.query(&memory, base).unwrap().protection, 0x102);
    let guard = memory.guard_page(GuestAddress(base)).unwrap();
    memory.apply_private_pages(&[guard.clear()], &[]).unwrap();
    assert_eq!(vm.query(&memory, base).unwrap().protection, 2);
    assert_eq!(vm.query(&memory, base).unwrap().size, 4096);
    assert_eq!(vm.query(&memory, base + 4096).unwrap().protection, 0x102);
    assert!(memory.read(GuestAddress(base), &mut [0]).is_ok());
    assert!(memory.write(GuestAddress(base), &[1]).is_err());
    assert_eq!(
        vm.allocate(
            &mut memory,
            &[],
            0,
            4096,
            MEM_RESERVE | MEM_COMMIT,
            1 | PAGE_GUARD
        ),
        Err(MemoryError32::InvalidParameter)
    );
}

#[test]
fn heap_generate_flag_is_inherited_without_replacing_failure_or_allocating_on_failure() {
    let mut memory = memory();
    let mut vm = virtual_memory();
    let mut heaps = HeapManager32::new(MemoryLimits32::default()).unwrap();
    let heap = heaps
        .create(
            &mut vm,
            &mut memory,
            &[],
            HEAP_GENERATE_EXCEPTIONS,
            0,
            65536,
        )
        .unwrap();
    assert!(heaps.generates_exceptions(heap, 0));
    let version = memory.transaction_version();
    assert_eq!(
        heaps.allocate(&mut vm, &mut memory, heap, 0, 65537),
        Err(MemoryError32::Capacity)
    );
    assert_eq!(memory.transaction_version(), version);
    assert!(heaps.generates_exceptions(0xbad, HEAP_GENERATE_EXCEPTIONS));
    assert!(!heaps.generates_exceptions(0xbad, 0));
}
