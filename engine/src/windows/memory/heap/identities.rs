use super::*;

fn fixture() -> (HeapManager32, VirtualMemory32, AddressSpace, u32) {
    let mut memory = AddressSpace::new(64).unwrap();
    let mut vm = VirtualMemory32::new(MemoryLimits32::default()).unwrap();
    let mut heaps = HeapManager32::new(MemoryLimits32::default()).unwrap();
    let handle = heaps.ensure_default(&mut vm, &mut memory, &[]).unwrap();
    (heaps, vm, memory, handle)
}

#[test]
fn reuse_and_in_place_reallocation_renew_only_the_owned_block_identity() {
    let (mut heaps, mut vm, mut memory, heap) = fixture();
    let first = heaps.allocate(&mut vm, &mut memory, heap, 0, 8).unwrap();
    let neighbor = heaps.allocate(&mut vm, &mut memory, heap, 0, 8).unwrap();
    assert_eq!(first / PAGE_SIZE, neighbor / PAGE_SIZE);
    let identity = heaps.allocation_identity(heap, first).unwrap();
    let neighbor_identity = heaps.allocation_identity(heap, neighbor).unwrap();
    assert_ne!(identity, neighbor_identity);
    heaps.free(heap, 0, first).unwrap();
    assert!(heaps.allocation_identity(heap, first).is_err());
    assert_eq!(
        heaps.allocate(&mut vm, &mut memory, heap, 0, 8).unwrap(),
        first
    );
    let reused = heaps.allocation_identity(heap, first).unwrap();
    assert_ne!(reused, identity);
    assert_eq!(
        heaps
            .reallocate(
                &mut vm,
                &mut memory,
                heap,
                HEAP_REALLOC_IN_PLACE_ONLY,
                first,
                8
            )
            .unwrap(),
        first
    );
    assert_ne!(heaps.allocation_identity(heap, first).unwrap(), reused);
    assert_eq!(
        heaps.allocation_identity(heap, neighbor).unwrap(),
        neighbor_identity
    );
}

#[test]
fn grouped_publication_renews_reused_blocks_and_output_fault_consumes_no_identity() {
    let (mut heaps, mut vm, mut memory, heap) = fixture();
    let first = heaps
        .allocate_initialized(&mut vm, &mut memory, heap, b"old")
        .unwrap();
    let identity = heaps.allocation_identity(heap, first).unwrap();
    let prepared = heaps
        .prepare_initialized_allocations(&memory, heap, &[3, 3], &[first])
        .unwrap();
    let addresses = prepared.addresses().to_vec();
    let version = memory.transaction_version();
    assert!(
        heaps
            .commit_initialized_allocations(
                &mut vm,
                &mut memory,
                prepared,
                &[b"new", b"two"],
                &[(GuestAddress(0xfffffff0), &[1])],
            )
            .is_err()
    );
    assert_eq!(memory.transaction_version(), version);
    assert_eq!(heaps.next_allocation, identity);
    assert_eq!(heaps.allocation_identity(heap, first).unwrap(), identity);
    let prepared = heaps
        .prepare_initialized_allocations(&memory, heap, &[3, 3], &[first])
        .unwrap();
    assert_eq!(prepared.addresses(), addresses);
    heaps
        .commit_initialized_allocations(&mut vm, &mut memory, prepared, &[b"new", b"two"], &[])
        .unwrap();
    let renewed = heaps.allocation_identity(heap, addresses[0]).unwrap();
    let second = heaps.allocation_identity(heap, addresses[1]).unwrap();
    assert_ne!(renewed, identity);
    assert_ne!(second, renewed);
    assert!(renewed != 0 && second != 0);
}

#[test]
fn identity_exhaustion_rejects_single_group_and_realloc_before_memory_publication() {
    let (mut heaps, mut vm, mut memory, heap) = fixture();
    let pointer = heaps
        .allocate_initialized(&mut vm, &mut memory, heap, b"old")
        .unwrap();
    let identity = heaps.allocation_identity(heap, pointer).unwrap();
    heaps.next_allocation = u64::MAX;
    let version = memory.transaction_version();
    assert!(matches!(
        heaps.prepare_initialized_allocation(&memory, heap, 8),
        Err(MemoryError32::Capacity)
    ));
    assert!(matches!(
        heaps.prepare_initialized_allocations(&memory, heap, &[8], &[pointer]),
        Err(MemoryError32::Capacity)
    ));
    assert_eq!(
        heaps.reallocate(&mut vm, &mut memory, heap, 0, pointer, 8),
        Err(MemoryError32::Capacity)
    );
    assert_eq!(memory.transaction_version(), version);
    assert_eq!(heaps.allocation_identity(heap, pointer).unwrap(), identity);
    let mut bytes = [0; 3];
    memory.read(GuestAddress(pointer), &mut bytes).unwrap();
    assert_eq!(&bytes, b"old");
}
