use super::Backing;
use crate::memory::{AddressSpace, GuestAddress, MemoryError, PAGE_SIZE, PageRange, Permissions};

#[test]
fn logical_capacity_is_zeroed_and_page_aligned() {
    for length in [1, 31, 4095, 4096, 4097, 8192, 3 * PAGE_SIZE as usize] {
        let backing = Backing::zeroed(length).unwrap();
        assert_eq!(backing.len(), length);
        assert_eq!(backing.as_ptr() as usize % PAGE_SIZE as usize, 0);
        assert!(backing.iter().all(|&byte| byte == 0));
        assert_eq!(backing.storage.len(), length + PAGE_SIZE as usize - 1);
        assert!(backing.offset < PAGE_SIZE as usize);
        assert!(backing.offset + length <= backing.storage.len());
    }
}

#[test]
fn only_the_logical_slice_is_exposed_or_mutated() {
    let mut backing = Backing::zeroed(17).unwrap();
    backing.storage.fill(0xcc);
    backing.fill(0x5a);
    assert_eq!(backing.as_ref(), &[0x5a; 17]);
    assert!(backing.get(17).is_none());
    assert!(backing.get(17..18).is_none());
    assert!(
        backing.storage[..backing.offset]
            .iter()
            .all(|&byte| byte == 0xcc)
    );
    assert!(
        backing.storage[backing.offset + backing.length..]
            .iter()
            .all(|&byte| byte == 0xcc)
    );
}

#[test]
fn moves_preserve_the_aligned_allocation_and_slice_operations() {
    let mut backing = Backing::zeroed(32).unwrap();
    backing[..8].copy_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
    backing.copy_within(..8, 8);
    let address = backing.as_ptr();
    let moved = Some(backing).unwrap();
    let mut moved_again = vec![moved].pop().unwrap();
    assert_eq!(moved_again.as_ptr(), address);
    assert_eq!(&moved_again[8..16], &[1, 2, 3, 4, 5, 6, 7, 8]);
    moved_again[31] = 0xff;
    assert_eq!(moved_again[31], 0xff);
}

#[test]
fn invalid_capacity_is_rejected_before_allocation() {
    for length in [0, usize::MAX, isize::MAX as usize] {
        assert!(matches!(
            Backing::zeroed(length),
            Err(MemoryError::Capacity)
        ));
    }
}

#[test]
fn test_snapshots_clone_logical_bytes_into_independent_aligned_storage() {
    let mut backing = Backing::zeroed(4097).unwrap();
    backing[0] = 7;
    backing[4096] = 9;
    let mut copy = backing.clone();
    assert_eq!(copy, backing);
    assert_ne!(copy.as_ptr(), backing.as_ptr());
    assert_eq!(copy.as_ptr() as usize % PAGE_SIZE as usize, 0);
    copy[0] = 1;
    assert_eq!(backing[0], 7);
}

#[test]
fn address_space_publishes_only_requested_capacity_and_keeps_guest_behavior() {
    let mut space = AddressSpace::new(2).unwrap();
    let (_, _, bytes) = space.cpu_read_arrays();
    let address = bytes.as_ptr();
    assert_eq!(bytes.len(), 2 * PAGE_SIZE as usize);
    assert_eq!(address as usize % PAGE_SIZE as usize, 0);
    assert_eq!(space.capacity_pages(), 2);
    let range = PageRange::new(GuestAddress(0x1000), 1).unwrap();
    space.map_zeroed(range, Permissions::READ_WRITE).unwrap();
    space.write(GuestAddress(0x1ffc), &[1, 2, 3, 4]).unwrap();
    let mut actual = [0; 4];
    space.read(GuestAddress(0x1ffc), &mut actual).unwrap();
    assert_eq!(actual, [1, 2, 3, 4]);
    space.protect(range, Permissions::READ).unwrap();
    assert!(space.write(GuestAddress(0x1ffc), &[0; 4]).is_err());
    space.unmap(range).unwrap();
    let (_, _, bytes) = space.cpu_read_arrays();
    assert_eq!(bytes.as_ptr(), address);
    assert_eq!(bytes.len(), 2 * PAGE_SIZE as usize);
}
