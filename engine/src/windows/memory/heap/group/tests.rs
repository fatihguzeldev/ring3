use super::*;
use crate::memory::{Access, Permissions};
fn runtime(pages: u32) -> (AddressSpace, VirtualMemory32, HeapManager32, u32) {
    let mut memory = AddressSpace::new(pages).unwrap();
    let mut vm = VirtualMemory32::new(MemoryLimits32::default()).unwrap();
    let mut heaps = HeapManager32::new(MemoryLimits32::default()).unwrap();
    let heap = heaps.ensure_default(&mut vm, &mut memory, &[]).unwrap();
    (memory, vm, heaps, heap)
}
fn blocks(heaps: &HeapManager32, handle: u32) -> Vec<(u32, u32, Option<u32>)> {
    heaps.heaps[heaps.index(handle).unwrap()]
        .blocks
        .iter()
        .map(|block| (block.address, block.extent, block.size))
        .collect()
}
#[test]
fn group_payloads_and_output_publish_once_with_independent_local_free_ownership() {
    let (mut memory, mut vm, mut heaps, heap) = runtime(64);
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::READ_WRITE,
        )
        .unwrap();
    let prepared = heaps
        .prepare_initialized_allocations(&memory, heap, &[8, 16], &[])
        .unwrap();
    let pointers = prepared.addresses().to_vec();
    let version = memory.transaction_version();
    let mut output = [0; 8];
    output[..4].copy_from_slice(&pointers[0].to_le_bytes());
    output[4..].copy_from_slice(&pointers[1].to_le_bytes());
    heaps
        .commit_initialized_allocations(
            &mut vm,
            &mut memory,
            prepared,
            &[b"abcdefgh", b"1234567890abcdef"],
            &[(GuestAddress(0x1000), &output)],
        )
        .unwrap();
    assert_eq!(memory.transaction_version(), version + 1);
    let mut actual = [0; 8];
    memory.read(GuestAddress(0x1000), &mut actual).unwrap();
    assert_eq!(actual, output);
    assert_eq!(heaps.size(heap, 0, pointers[0]).unwrap(), 8);
    assert_eq!(heaps.size(heap, 0, pointers[1]).unwrap(), 16);
    heaps.local_free(pointers[0]).unwrap();
    assert_eq!(heaps.size(heap, 0, pointers[1]).unwrap(), 16);
    heaps.local_free(pointers[1]).unwrap();
    assert_eq!(blocks(&heaps, heap).len(), 1);
}
#[test]
fn failed_group_publication_keeps_retired_source_blocks_and_all_virtual_memory_unchanged() {
    let (mut memory, mut vm, mut heaps, heap) = runtime(64);
    let old = heaps
        .allocate_initialized(&mut vm, &mut memory, heap, b"oldvalue")
        .unwrap();
    let before = blocks(&heaps, heap);
    let version = memory.transaction_version();
    let pages = memory.mapped_pages();
    let info = vm.query(&memory, heap).unwrap();
    let prepared = heaps
        .prepare_initialized_allocations(&memory, heap, &[8, 4096], &[old])
        .unwrap();
    assert_eq!(prepared.addresses()[0], old);
    assert_eq!(
        heaps.commit_initialized_allocations(
            &mut vm,
            &mut memory,
            prepared,
            &[b"newvalue", &[0; 4096]],
            &[(GuestAddress(0xfffffff0), b"bad!")]
        ),
        Err(MemoryError32::NoAccess)
    );
    assert_eq!(blocks(&heaps, heap), before);
    assert_eq!(memory.transaction_version(), version);
    assert_eq!(memory.mapped_pages(), pages);
    assert_eq!(vm.query(&memory, heap).unwrap(), info);
    let mut bytes = [0; 8];
    memory.read(GuestAddress(old), &mut bytes).unwrap();
    assert_eq!(&bytes, b"oldvalue");
}
#[test]
fn stale_free_without_a_memory_write_cannot_restore_old_heap_metadata() {
    let (mut memory, mut vm, mut heaps, heap) = runtime(64);
    let old = heaps
        .allocate_initialized(&mut vm, &mut memory, heap, b"oldvalue")
        .unwrap();
    let prepared = heaps
        .prepare_initialized_allocations(&memory, heap, &[8, 8], &[])
        .unwrap();
    let version = memory.transaction_version();
    heaps.local_free(old).unwrap();
    assert_eq!(memory.transaction_version(), version);
    let after_free = blocks(&heaps, heap);
    assert_eq!(
        heaps.commit_initialized_allocations(
            &mut vm,
            &mut memory,
            prepared,
            &[b"abcdefgh", b"12345678"],
            &[]
        ),
        Err(MemoryError32::InvalidParameter)
    );
    assert_eq!(blocks(&heaps, heap), after_free);
    assert_eq!(memory.transaction_version(), version);
}
#[test]
fn disjoint_group_commit_preserves_intervening_readonly_and_uncommitted_pages() {
    let (mut memory, mut vm, mut heaps, heap) = runtime(64);
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::READ_WRITE,
        )
        .unwrap();
    let a = heaps.allocate(&mut vm, &mut memory, heap, 0, 4096).unwrap();
    let b = heaps.allocate(&mut vm, &mut memory, heap, 0, 4096).unwrap();
    let c = heaps.allocate(&mut vm, &mut memory, heap, 0, 4096).unwrap();
    let d = heaps.allocate(&mut vm, &mut memory, heap, 0, 4096).unwrap();
    let e = heaps.allocate(&mut vm, &mut memory, heap, 0, 4096).unwrap();
    heaps.local_free(a).unwrap();
    heaps.local_free(c).unwrap();
    heaps.local_free(e).unwrap();
    vm.protect(&mut memory, b, 4096, 2, GuestAddress(0x1000))
        .unwrap();
    vm.free(&mut memory, d, 4096, MEM_DECOMMIT).unwrap();
    vm.free(&mut memory, a, 4096, MEM_DECOMMIT).unwrap();
    vm.free(&mut memory, c, 4096, MEM_DECOMMIT).unwrap();
    let before_b = vm.query(&memory, b).unwrap();
    let before_d = vm.query(&memory, d).unwrap();
    let prepared = heaps
        .prepare_initialized_allocations(&memory, heap, &[4096, 4096], &[])
        .unwrap();
    assert_eq!(prepared.addresses(), [a, c]);
    heaps
        .commit_initialized_allocations(
            &mut vm,
            &mut memory,
            prepared,
            &[&[1; 4096], &[2; 4096]],
            &[],
        )
        .unwrap();
    assert_eq!(vm.query(&memory, b).unwrap(), before_b);
    assert_eq!(vm.query(&memory, d).unwrap(), before_d);
    assert!(
        memory
            .check_span(GuestAddress(b), 1, Access::Write)
            .is_err()
    );
    assert!(memory.check_span(GuestAddress(d), 1, Access::Read).is_err());
}
#[test]
fn group_capacity_and_version_exhaustion_do_not_partially_commit_pages_or_blocks() {
    let (mut memory, mut vm, mut heaps, heap) = runtime(2);
    let before = blocks(&heaps, heap);
    let version = memory.transaction_version();
    let prepared = heaps
        .prepare_initialized_allocations(&memory, heap, &[4096, 4096, 4096], &[])
        .unwrap();
    assert_eq!(
        heaps.commit_initialized_allocations(
            &mut vm,
            &mut memory,
            prepared,
            &[&[0; 4096], &[0; 4096], &[0; 4096]],
            &[]
        ),
        Err(MemoryError32::Capacity)
    );
    assert_eq!(blocks(&heaps, heap), before);
    assert_eq!(memory.mapped_pages(), 0);
    assert_eq!(memory.transaction_version(), version);
    memory.exhaust_versions_for_test();
    let prepared = heaps
        .prepare_initialized_allocations(&memory, heap, &[8, 8], &[])
        .unwrap();
    assert_eq!(
        heaps.commit_initialized_allocations(
            &mut vm,
            &mut memory,
            prepared,
            &[b"abcdefgh", b"12345678"],
            &[]
        ),
        Err(MemoryError32::Exhausted)
    );
    assert_eq!(blocks(&heaps, heap), before);
    assert_eq!(memory.mapped_pages(), 0);
}
