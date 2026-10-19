use super::*;

#[test]
fn logical_backing_is_page_aligned_zeroed_and_exactly_requested_capacity() {
    for pages in [1, 2, 3, 17] {
        let space = AddressSpace::new(pages).unwrap();
        let (_, _, backing) = space.cpu_read_arrays();
        assert_eq!(backing.len(), pages as usize * PAGE_SIZE as usize);
        assert_eq!(space.capacity_pages(), pages);
        assert_eq!(backing.as_ptr() as usize % PAGE_SIZE as usize, 0);
        assert!(backing.iter().all(|&byte| byte == 0));
        assert!(backing.get(backing.len()).is_none());
    }
    assert!(matches!(AddressSpace::new(0), Err(MemoryError::Capacity)));
    assert!(matches!(
        AddressSpace::new(u32::MAX),
        Err(MemoryError::Capacity)
    ));
}

#[test]
fn backing_pointer_survives_mapping_writes_protection_and_address_space_moves() {
    let mut space = AddressSpace::new(2).unwrap();
    let (_, _, bytes) = space.cpu_read_arrays();
    let address = bytes.as_ptr();
    let range = PageRange::new(GuestAddress(0x1000), 1).unwrap();
    space.map_zeroed(range, Permissions::READ_WRITE).unwrap();
    space.write(GuestAddress(0x1ffc), &[1, 2, 3, 4]).unwrap();
    let mut actual = [0; 4];
    space.read(GuestAddress(0x1ffc), &mut actual).unwrap();
    assert_eq!(actual, [1, 2, 3, 4]);
    space.protect(range, Permissions::READ).unwrap();
    assert!(space.write(GuestAddress(0x1ffc), &[0; 4]).is_err());
    space.unmap(range).unwrap();
    let moved = Some(space).unwrap();
    let moved_again = vec![moved].pop().unwrap();
    let (_, _, bytes) = moved_again.cpu_read_arrays();
    assert_eq!(bytes.as_ptr(), address);
    assert_eq!(bytes.len(), 2 * PAGE_SIZE as usize);
}
