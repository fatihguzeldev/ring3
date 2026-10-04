use ring3_engine::memory::{
    Access, AddressSpace, FaultReason, GuestAddress, MemoryError, MemoryFault, PAGE_SIZE,
    PageRange, Permissions,
};

const BASE: u32 = 0x1000_0000;

fn range(address: u32, pages: u32) -> PageRange {
    PageRange::new(GuestAddress(address), pages).unwrap()
}

#[test]
fn first_fit_checks_every_page_against_a_finite_occupied_set() {
    let probes = [0, 1, 15, 16, 17, 31, 32, 47];
    let mut memory = AddressSpace::new(64).unwrap();
    memory
        .map_zeroed(range(0x1000, 1), Permissions::ALL)
        .unwrap();
    memory
        .write(GuestAddress(0x1000), &[0x90, 0xeb, 0])
        .unwrap();
    let code = memory.snapshot_code(GuestAddress(0x1000), 3).unwrap();
    for mask in 0..256_u32 {
        let occupied: Vec<_> = probes
            .into_iter()
            .enumerate()
            .filter(|(index, _)| mask & (1 << index) != 0)
            .map(|(_, page)| page)
            .collect();
        for &page in &occupied {
            memory
                .map_zeroed(range(BASE + page * PAGE_SIZE, 1), Permissions::NONE)
                .unwrap();
        }
        for count in [1, 2, 16, 17] {
            let expected = [0, 16, 32]
                .into_iter()
                .find(|start| {
                    start + count <= 48
                        && occupied
                            .iter()
                            .all(|page| *page < *start || *page >= start + count)
                })
                .map(|start| range(BASE + start * PAGE_SIZE, count))
                .ok_or(MemoryError::Capacity);
            assert_eq!(
                memory.find_free_range(range(BASE, 48), count, 65536),
                expected,
                "mask={mask:08b}, count={count}"
            );
            assert_eq!(memory.mapped_pages(), 1 + occupied.len() as u32);
            assert!(memory.is_code_current(&code));
        }
        for &page in &occupied {
            memory.unmap(range(BASE + page * PAGE_SIZE, 1)).unwrap();
        }
    }
}

#[test]
fn planner_rounds_the_base_and_keeps_the_whole_fit_inside_bounds() {
    let memory = AddressSpace::new(32).unwrap();
    assert_eq!(
        memory.find_free_range(range(BASE + PAGE_SIZE, 32), 1, 65536),
        Ok(range(BASE + 65536, 1))
    );
    assert_eq!(
        memory.find_free_range(range(BASE + PAGE_SIZE, 15), 1, 65536),
        Err(MemoryError::Capacity)
    );
    assert_eq!(
        memory.find_free_range(range(BASE + PAGE_SIZE, 16), 2, 65536),
        Err(MemoryError::Capacity)
    );
    assert_eq!(
        memory.find_free_range(range(0xffff_f000, 1), 1, 65536),
        Err(MemoryError::Capacity)
    );
    assert_eq!(
        memory.find_free_range(range(0xffff_f000, 1), 1, PAGE_SIZE),
        Ok(range(0xffff_f000, 1))
    );
    assert_eq!(memory.mapped_pages(), 0);
}

#[test]
fn invalid_alignment_zero_count_and_backing_capacity_are_read_only_errors() {
    let memory = AddressSpace::new(1).unwrap();
    for alignment in [0, 1, 2048, 12288, u32::MAX] {
        assert_eq!(
            memory.find_free_range(range(BASE, 32), 1, alignment),
            Err(MemoryError::InvalidRange)
        );
    }
    assert_eq!(
        memory.find_free_range(range(BASE, 32), 0, PAGE_SIZE),
        Err(MemoryError::InvalidRange)
    );
    assert_eq!(
        memory.find_free_range(range(BASE, 32), 2, PAGE_SIZE),
        Err(MemoryError::Capacity)
    );
    assert_eq!(
        memory.find_free_range(range(BASE, 32), u32::MAX, PAGE_SIZE),
        Err(MemoryError::Capacity)
    );
    assert_eq!(memory.mapped_pages(), 0);
}

#[test]
fn planned_ranges_map_zeroed_rw_without_execute_and_preserve_old_code() {
    let mut memory = AddressSpace::new(4).unwrap();
    memory
        .map_zeroed(range(0x1000, 1), Permissions::ALL)
        .unwrap();
    memory.write(GuestAddress(0x1000), &[0x90]).unwrap();
    let code = memory.snapshot_code(GuestAddress(0x1000), 1).unwrap();
    let first = memory.find_free_range(range(BASE, 48), 1, 65536).unwrap();
    memory.map_zeroed(first, Permissions::READ_WRITE).unwrap();
    let second = memory.find_free_range(range(BASE, 48), 2, 65536).unwrap();
    assert_eq!(first, range(BASE, 1));
    assert_eq!(second, range(BASE + 65536, 2));
    memory.map_zeroed(second, Permissions::READ_WRITE).unwrap();
    for address in [BASE, BASE + 4095, BASE + 65536, BASE + 65536 + 8191] {
        let mut byte = [0xff];
        memory.read(GuestAddress(address), &mut byte).unwrap();
        assert_eq!(byte, [0]);
        memory.write(GuestAddress(address), &[0xa5]).unwrap();
        memory.read(GuestAddress(address), &mut byte).unwrap();
        assert_eq!(byte, [0xa5]);
        assert_eq!(
            memory.resolve(GuestAddress(address), Access::Execute),
            Err(MemoryError::Fault(MemoryFault {
                address: GuestAddress(address),
                access: Access::Execute,
                reason: FaultReason::Permission,
            }))
        );
    }
    assert!(memory.is_code_current(&code));
    assert_eq!(
        memory.find_free_range(range(BASE, 48), 1, 65536),
        Err(MemoryError::Capacity)
    );
}
