use ring3_engine::memory::{
    Access, AddressSpace, FaultReason, GuestAddress, MAX_WORD_WRITES32, MemoryError, MemoryFault,
    PageRange, Permissions, WordWrite32,
};

fn map(memory: &mut AddressSpace, address: u32, pages: u32, permissions: Permissions) {
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(address), pages).unwrap(),
            permissions,
        )
        .unwrap();
}

fn word(address: u32, value: u32) -> WordWrite32 {
    WordWrite32 {
        address: GuestAddress(address),
        value,
    }
}

fn bytes(memory: &AddressSpace, address: u32, length: usize) -> Vec<u8> {
    let mut output = vec![0; length];
    memory.read(GuestAddress(address), &mut output).unwrap();
    output
}

fn fault(address: u32, reason: FaultReason) -> MemoryError {
    MemoryError::Fault(MemoryFault {
        address: GuestAddress(address),
        access: Access::Write,
        reason,
    })
}

#[test]
fn ordered_word_batch_commits_independent_little_endian_words() {
    let mut memory = AddressSpace::new(2).unwrap();
    map(&mut memory, 0x1000, 1, Permissions::ALL);
    map(&mut memory, 0x3000, 1, Permissions::ALL);
    assert_eq!(
        memory.write_words32(&[word(0x1001, 0x4433_2211), word(0x3000, 0xaabb_ccdd)]),
        Ok(())
    );
    assert_eq!(bytes(&memory, 0x1000, 6), [0, 0x11, 0x22, 0x33, 0x44, 0]);
    assert_eq!(bytes(&memory, 0x3000, 5), [0xdd, 0xcc, 0xbb, 0xaa, 0]);
    assert_eq!(memory.capacity_pages(), 2);
    assert_eq!(memory.mapped_pages(), 2);
}

#[test]
fn empty_and_oversized_batches_do_not_touch_memory_or_code_stamps() {
    assert_eq!(MAX_WORD_WRITES32, 17);
    let mut memory = AddressSpace::new(1).unwrap();
    assert_eq!(memory.write_words32(&[]), Ok(()));
    assert_eq!(memory.mapped_pages(), 0);
    map(&mut memory, 0x1000, 1, Permissions::ALL);
    memory.write(GuestAddress(0x1000), &[0x90; 8]).unwrap();
    let snapshot = memory.snapshot_code(GuestAddress(0x1000), 8).unwrap();
    let mapping = memory.resolve(GuestAddress(0x1000), Access::Write).unwrap();
    assert_eq!(memory.write_words32(&[]), Ok(()));
    assert_eq!(
        memory.write_words32(&[word(u32::MAX, 0); 18]),
        Err(MemoryError::InvalidRange)
    );
    assert_eq!(bytes(&memory, 0x1000, 8), [0x90; 8]);
    assert!(memory.is_code_current(&snapshot));
    assert_eq!(
        memory.resolve(GuestAddress(0x1000), Access::Write),
        Ok(mapping)
    );
    assert_eq!(memory.mapped_pages(), 1);
}

#[test]
fn one_and_seventeen_words_include_the_last_descriptor_across_address_wrap() {
    let mut memory = AddressSpace::new(2).unwrap();
    map(&mut memory, 0xffff_f000, 1, Permissions::ALL);
    map(&mut memory, 0, 1, Permissions::ALL);
    assert_eq!(
        memory.write_words32(&[word(0xffff_fffc, 0x4433_2211)]),
        Ok(())
    );
    assert_eq!(bytes(&memory, 0xffff_fffc, 4), [0x11, 0x22, 0x33, 0x44]);
    let high = memory.snapshot_code(GuestAddress(0xffff_fffc), 4).unwrap();
    let low = memory.snapshot_code(GuestAddress(0), 64).unwrap();
    let writes = std::array::from_fn::<_, 17, _>(|index| {
        word(
            if index == 0 {
                0xffff_fffc
            } else {
                (index as u32 - 1) * 4
            },
            0x1122_3300 + index as u32,
        )
    });
    assert_eq!(memory.write_words32(&writes), Ok(()));
    assert_eq!(bytes(&memory, 0xffff_fffc, 4), [0, 0x33, 0x22, 0x11]);
    for index in 1..=16u8 {
        assert_eq!(
            bytes(&memory, u32::from(index - 1) * 4, 4),
            [index, 0x33, 0x22, 0x11]
        );
    }
    assert!(!memory.is_code_current(&high));
    assert!(!memory.is_code_current(&low));
    assert_eq!(memory.mapped_pages(), 2);
}

#[test]
fn unaligned_cross_page_batches_need_only_write_permission() {
    let mut memory = AddressSpace::new(2).unwrap();
    map(&mut memory, 0x8000, 2, Permissions::from_bits(2).unwrap());
    assert_eq!(
        memory.write_words32(&[word(0x8fff, 0x0123_4567), word(0x8000, 0xdead_beef)]),
        Ok(())
    );
    for address in [0x8000, 0x8fff, 0x9000] {
        assert!(memory.resolve(GuestAddress(address), Access::Write).is_ok());
        for access in [Access::Read, Access::Execute] {
            assert_eq!(
                memory.resolve(GuestAddress(address), access),
                Err(MemoryError::Fault(MemoryFault {
                    address: GuestAddress(address),
                    access,
                    reason: FaultReason::Permission,
                }))
            );
        }
    }
    memory
        .protect(
            PageRange::new(GuestAddress(0x8000), 2).unwrap(),
            Permissions::READ,
        )
        .unwrap();
    assert_eq!(bytes(&memory, 0x8000, 4), [0xef, 0xbe, 0xad, 0xde]);
    assert_eq!(bytes(&memory, 0x8ffe, 6), [0, 0x67, 0x45, 0x23, 1, 0]);
    assert_eq!(memory.mapped_pages(), 2);
}

#[test]
fn exact_and_partial_aliases_commit_in_descriptor_order() {
    let mut memory = AddressSpace::new(1).unwrap();
    map(&mut memory, 0x1000, 1, Permissions::ALL);
    memory.write(GuestAddress(0x1000), &[0xee; 10]).unwrap();
    assert_eq!(
        memory.write_words32(&[
            word(0x1000, 0x4433_2211),
            word(0x1002, 0xaabb_ccdd),
            word(0x1000, 0x8877_6655),
            word(0x1005, 0x0403_0201),
        ]),
        Ok(())
    );
    assert_eq!(
        bytes(&memory, 0x1000, 10),
        [0x55, 0x66, 0x77, 0x88, 0xbb, 1, 2, 3, 4, 0xee]
    );
    memory.write(GuestAddress(0x1000), &[0xee; 10]).unwrap();
    assert_eq!(
        memory.write_words32(&[
            word(0x1005, 0x0403_0201),
            word(0x1000, 0x4433_2211),
            word(0x1002, 0xaabb_ccdd),
        ]),
        Ok(())
    );
    assert_eq!(
        bytes(&memory, 0x1000, 10),
        [0x11, 0x22, 0xdd, 0xcc, 0xbb, 0xaa, 2, 3, 4, 0xee]
    );
}

#[test]
fn later_descriptor_faults_keep_every_prefix_suffix_mapping_and_snapshot() {
    for (address, reason) in [
        (0x5000, FaultReason::Unmapped),
        (0x2001, FaultReason::Permission),
        (u32::MAX, FaultReason::AddressOverflow),
    ] {
        let mut memory = AddressSpace::new(3).unwrap();
        for (page, value) in [(0x1000, 0x11), (0x2000, 0x22), (0x3000, 0x33)] {
            map(&mut memory, page, 1, Permissions::ALL);
            memory.write(GuestAddress(page), &[value; 12]).unwrap();
        }
        memory
            .protect(
                PageRange::new(GuestAddress(0x2000), 1).unwrap(),
                Permissions::READ_EXECUTE,
            )
            .unwrap();
        let snapshots = [0x1000, 0x2000, 0x3000]
            .map(|page| memory.snapshot_code(GuestAddress(page), 12).unwrap());
        let mappings = [0x1000, 0x2000, 0x3000]
            .map(|page| memory.resolve(GuestAddress(page), Access::Read).unwrap());
        assert_eq!(
            memory.write_words32(&[
                word(0x1001, 0x4433_2211),
                word(0x1002, 0xaabb_ccdd),
                word(address, 0),
                word(0x3000, u32::MAX),
            ]),
            Err(fault(address, reason))
        );
        for (index, page) in [0x1000, 0x2000, 0x3000].into_iter().enumerate() {
            assert_eq!(bytes(&memory, page, 12), [0x11 * (index as u8 + 1); 12]);
            assert!(memory.is_code_current(&snapshots[index]));
            assert_eq!(
                memory.resolve(GuestAddress(page), Access::Read),
                Ok(mappings[index])
            );
        }
        assert_eq!(
            memory.resolve(GuestAddress(0x2000), Access::Write),
            Err(fault(0x2000, FaultReason::Permission))
        );
        assert_eq!(memory.capacity_pages(), 3);
        assert_eq!(memory.mapped_pages(), 3);
    }
}

#[test]
fn first_descriptor_error_precedes_later_overflow_or_mapping_errors() {
    let mut memory = AddressSpace::new(1).unwrap();
    map(&mut memory, 0x2000, 1, Permissions::READ_EXECUTE);
    for (writes, expected) in [
        (
            [word(0x5000, 1), word(u32::MAX, 2)],
            fault(0x5000, FaultReason::Unmapped),
        ),
        (
            [word(u32::MAX, 1), word(0x5000, 2)],
            fault(u32::MAX, FaultReason::AddressOverflow),
        ),
        (
            [word(0x2000, 1), word(0x5000, 2)],
            fault(0x2000, FaultReason::Permission),
        ),
    ] {
        assert_eq!(memory.write_words32(&writes), Err(expected));
    }
    assert_eq!(bytes(&memory, 0x2000, 4), [0; 4]);
}

#[test]
fn a_second_page_fault_cannot_publish_either_word_or_invalidate_code() {
    for denied in [false, true] {
        let mut memory = AddressSpace::new(3).unwrap();
        map(&mut memory, 0x1000, 1, Permissions::ALL);
        map(&mut memory, 0x3000, 1, Permissions::ALL);
        memory
            .write(GuestAddress(0x1ffc), &[0x11, 0x22, 0x33, 0x44])
            .unwrap();
        memory
            .write(GuestAddress(0x3000), &[0x55, 0x66, 0x77, 0x88])
            .unwrap();
        if denied {
            map(&mut memory, 0x2000, 1, Permissions::READ_EXECUTE);
        }
        let end = memory.snapshot_code(GuestAddress(0x1ffc), 4).unwrap();
        let prefix = memory.snapshot_code(GuestAddress(0x3000), 4).unwrap();
        assert_eq!(
            memory.write_words32(&[word(0x3000, 0), word(0x1ffe, u32::MAX), word(0x1000, 0)]),
            Err(fault(
                0x2000,
                if denied {
                    FaultReason::Permission
                } else {
                    FaultReason::Unmapped
                }
            ))
        );
        assert_eq!(bytes(&memory, 0x1ffc, 4), [0x11, 0x22, 0x33, 0x44]);
        assert_eq!(bytes(&memory, 0x3000, 4), [0x55, 0x66, 0x77, 0x88]);
        assert_eq!(bytes(&memory, 0x1000, 4), [0; 4]);
        if denied {
            assert_eq!(bytes(&memory, 0x2000, 4), [0; 4]);
        }
        assert!(memory.is_code_current(&end));
        assert!(memory.is_code_current(&prefix));
    }
}

#[test]
fn each_overflowing_word_reports_its_start_without_wrapping_or_prefix_commit() {
    let mut memory = AddressSpace::new(2).unwrap();
    map(&mut memory, 0xffff_f000, 1, Permissions::ALL);
    map(&mut memory, 0, 1, Permissions::ALL);
    memory
        .write(GuestAddress(0), &[0x11, 0x22, 0x33, 0x44])
        .unwrap();
    memory
        .write(GuestAddress(0xffff_fffc), &[0x55, 0x66, 0x77, 0x88])
        .unwrap();
    let high = memory.snapshot_code(GuestAddress(0xffff_fffc), 4).unwrap();
    let low = memory.snapshot_code(GuestAddress(0), 4).unwrap();
    for address in [0xffff_fffd, 0xffff_fffe, u32::MAX] {
        assert_eq!(
            memory.write_words32(&[word(0, 0), word(address, 0)]),
            Err(fault(address, FaultReason::AddressOverflow))
        );
        assert_eq!(bytes(&memory, 0, 4), [0x11, 0x22, 0x33, 0x44]);
        assert_eq!(bytes(&memory, 0xffff_fffc, 4), [0x55, 0x66, 0x77, 0x88]);
        assert!(memory.is_code_current(&high));
        assert!(memory.is_code_current(&low));
    }
}

#[test]
fn identical_writes_invalidate_touched_pages_and_preserve_unrelated_snapshots() {
    let mut memory = AddressSpace::new(3).unwrap();
    for page in [0x1000, 0x2000, 0x3000] {
        map(&mut memory, page, 1, Permissions::ALL);
    }
    let code = memory.snapshot_code(GuestAddress(0x1000), 1).unwrap();
    let data = memory.snapshot_code(GuestAddress(0x2000), 1).unwrap();
    let unrelated = memory.snapshot_code(GuestAddress(0x3000), 1).unwrap();
    let mappings = [0x1000, 0x2000, 0x3000]
        .map(|page| memory.resolve(GuestAddress(page), Access::Write).unwrap());
    memory
        .write_words32(&[word(0x2000, 0), word(0x2000, 0)])
        .unwrap();
    assert!(memory.is_code_current(&code));
    assert!(!memory.is_code_current(&data));
    assert!(memory.is_code_current(&unrelated));
    let data = memory.snapshot_code(GuestAddress(0x2000), 1).unwrap();
    memory
        .write_words32(&[word(0x1008, 0), word(0x1008, 0)])
        .unwrap();
    assert!(!memory.is_code_current(&code));
    assert!(memory.is_code_current(&data));
    assert!(memory.is_code_current(&unrelated));
    for (index, page) in [0x1000, 0x2000, 0x3000].into_iter().enumerate() {
        for access in [Access::Read, Access::Write, Access::Execute] {
            assert_eq!(
                memory.resolve(GuestAddress(page), access),
                Ok(mappings[index])
            );
        }
    }
    assert_eq!(memory.mapped_pages(), 3);
}
