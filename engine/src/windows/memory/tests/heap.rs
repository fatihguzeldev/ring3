use super::*;

#[test]
fn heap_free_coalesces_and_realloc_preserves_data_and_zeroes_only_new_tail() {
    let mut memory = memory();
    let mut vm = virtual_memory();
    let mut heaps = HeapManager32::new(MemoryLimits32::default()).unwrap();
    let heap = heaps.ensure_default(&mut vm, &mut memory, &[]).unwrap();
    let first = heaps
        .allocate(&mut vm, &mut memory, heap, HEAP_ZERO_MEMORY, 16)
        .unwrap();
    let middle = heaps.allocate(&mut vm, &mut memory, heap, 0, 16).unwrap();
    let last = heaps.allocate(&mut vm, &mut memory, heap, 0, 16).unwrap();
    memory
        .write(GuestAddress(first), b"abcdefghijklmnop")
        .unwrap();
    assert_eq!(
        heaps.reallocate(
            &mut vm,
            &mut memory,
            heap,
            HEAP_REALLOC_IN_PLACE_ONLY,
            first,
            32
        ),
        Err(MemoryError32::Capacity)
    );
    let moved = heaps
        .reallocate(&mut vm, &mut memory, heap, HEAP_ZERO_MEMORY, first, 32)
        .unwrap();
    assert_ne!(moved, first);
    let mut bytes = [0; 32];
    memory.read(GuestAddress(moved), &mut bytes).unwrap();
    assert_eq!(&bytes[..16], b"abcdefghijklmnop");
    assert_eq!(&bytes[16..], &[0; 16]);
    heaps.free(heap, 0, middle).unwrap();
    heaps.free(heap, 0, last).unwrap();
    let joined = heaps.allocate(&mut vm, &mut memory, heap, 0, 48).unwrap();
    assert_eq!(joined, first);
    assert!(heaps.validate(heap, 0, 0).unwrap());
    assert_eq!(heaps.size(heap, 0, moved).unwrap(), 32);
}

#[test]
fn failed_heap_growth_keeps_original_block_and_other_heap_ownership() {
    let mut memory = AddressSpace::new(2).unwrap();
    let mut vm = virtual_memory();
    let mut heaps = HeapManager32::new(MemoryLimits32::default()).unwrap();
    let heap = heaps.ensure_default(&mut vm, &mut memory, &[]).unwrap();
    let pointer = heaps.allocate(&mut vm, &mut memory, heap, 0, 4096).unwrap();
    memory.write(GuestAddress(pointer), b"kept").unwrap();
    let other = heaps
        .create(&mut vm, &mut memory, &[], 0, 0, 65536)
        .unwrap();
    assert_eq!(
        heaps.free(other, 0, pointer),
        Err(MemoryError32::InvalidParameter)
    );
    assert_eq!(
        heaps.reallocate(
            &mut vm,
            &mut memory,
            heap,
            HEAP_ZERO_MEMORY,
            pointer,
            3 * 4096
        ),
        Err(MemoryError32::Capacity)
    );
    let mut bytes = [0; 4];
    memory.read(GuestAddress(pointer), &mut bytes).unwrap();
    assert_eq!(&bytes, b"kept");
    assert_eq!(heaps.size(heap, 0, pointer).unwrap(), 4096);
    assert_eq!(memory.mapped_pages(), 1);
    heaps.free(heap, 0, pointer).unwrap();
    assert_eq!(heaps.compact(&vm, &memory, heap, 0).unwrap(), 4096);
}

#[test]
fn movable_objects_keep_handle_identity_discard_state_and_distinct_fixed_unlock_results() {
    let mut memory = memory();
    let mut vm = virtual_memory();
    let mut heaps = HeapManager32::new(MemoryLimits32::default()).unwrap();
    let handle = heaps
        .local_allocate(&mut vm, &mut memory, &[], 2 | 0x40, 16)
        .unwrap();
    let pointer = heaps.local_lock(handle).unwrap();
    assert_ne!(pointer, handle);
    assert_eq!(heaps.local_handle(pointer).unwrap(), handle);
    assert_eq!(heaps.local_flags(handle).unwrap() & 0xff, 1);
    assert!(!heaps.local_unlock(handle, false).unwrap());
    assert_eq!(
        heaps.local_unlock(handle, false),
        Err(MemoryError32::NotLocked)
    );
    assert_eq!(
        heaps
            .local_reallocate(&mut vm, &mut memory, handle, 0, 2)
            .unwrap(),
        handle
    );
    assert_eq!(heaps.local_lock(handle), Err(MemoryError32::Discarded));
    assert_eq!(heaps.local_flags(handle).unwrap() & 0x4000, 0x4000);
    heaps
        .local_reallocate(&mut vm, &mut memory, handle, 32, 2 | 0x40)
        .unwrap();
    assert_ne!(heaps.local_lock(handle).unwrap(), 0);
    heaps.local_free(handle).unwrap();
    assert_eq!(
        heaps.local_lock(handle),
        Err(MemoryError32::InvalidParameter)
    );
    let fixed = heaps
        .local_allocate(&mut vm, &mut memory, &[], 0, 4)
        .unwrap();
    assert_eq!(
        heaps.local_unlock(fixed, false),
        Err(MemoryError32::NotLocked)
    );
    assert!(heaps.local_unlock(fixed, true).unwrap());
}
