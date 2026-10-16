use super::*;

#[test]
fn low_fragmentation_policy_rounds_small_classes_reuses_exact_bins_and_cannot_be_disabled() {
    let mut memory = memory();
    let mut vm = virtual_memory();
    let mut heaps = HeapManager32::new(MemoryLimits32::default()).unwrap();
    let heap = heaps.ensure_default(&mut vm, &mut memory, &[]).unwrap();
    assert_eq!(heaps.compatibility(heap), Ok(0));
    heaps.enable_small_blocks(heap, 2).unwrap();
    assert_eq!(heaps.compatibility(heap), Ok(2));
    let first = heaps.allocate(&mut vm, &mut memory, heap, 0, 129).unwrap();
    let second = heaps.allocate(&mut vm, &mut memory, heap, 0, 129).unwrap();
    assert_eq!(second - first, 144);
    assert_eq!(heaps.size(heap, 0, first), Ok(129));
    heaps.free(heap, 0, first).unwrap();
    assert_eq!(
        heaps.allocate(&mut vm, &mut memory, heap, 0, 143),
        Ok(first)
    );
    assert_eq!(
        heaps.enable_small_blocks(heap, 0),
        Err(MemoryError32::InvalidParameter)
    );
    assert_eq!(heaps.compatibility(heap), Ok(2));
    for (flags, maximum) in [(HEAP_NO_SERIALIZE, 0), (0, 65536)] {
        let other = heaps
            .create(&mut vm, &mut memory, &[], flags, 0, maximum)
            .unwrap();
        assert_eq!(
            heaps.enable_small_blocks(other, 2),
            Err(MemoryError32::InvalidParameter)
        );
        assert_eq!(heaps.compatibility(other), Ok(0));
    }
}
#[test]
fn process_corruption_policy_distinguishes_double_free_from_a_foreign_pointer_without_mutating_heap()
 {
    let mut memory = memory();
    let mut vm = virtual_memory();
    let mut heaps = HeapManager32::new(MemoryLimits32::default()).unwrap();
    let heap = heaps.ensure_default(&mut vm, &mut memory, &[]).unwrap();
    let pointer = heaps.allocate(&mut vm, &mut memory, heap, 0, 16).unwrap();
    heaps.free(heap, 0, pointer).unwrap();
    assert_eq!(
        heaps.free(heap, 0, pointer),
        Err(MemoryError32::InvalidParameter)
    );
    heaps.enable_termination_on_corruption();
    let version = memory.transaction_version();
    assert_eq!(
        heaps.free(heap, 0, pointer),
        Err(MemoryError32::HeapCorruption)
    );
    assert_eq!(
        heaps.free(heap, 0, 0xdead),
        Err(MemoryError32::InvalidParameter)
    );
    assert!(heaps.validate(heap, 0, 0).unwrap());
    assert_eq!(memory.transaction_version(), version);
}
