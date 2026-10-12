use ring3_engine::memory::{
    Access, AddressSpace, FaultReason, GuestAddress, MemoryError, MemoryFault, PAGE_SIZE,
    PageRange, Permissions,
};

fn range(address: u32) -> PageRange {
    PageRange::new(GuestAddress(address), 1).unwrap()
}

fn page(space: &AddressSpace, address: u32) -> Vec<u8> {
    let mut bytes = vec![0; PAGE_SIZE as usize];
    space.read(GuestAddress(address), &mut bytes).unwrap();
    bytes
}

fn fault(address: u32, access: Access, reason: FaultReason) -> MemoryError {
    MemoryError::Fault(MemoryFault {
        address: GuestAddress(address),
        access,
        reason,
    })
}

#[test]
fn exchange_returns_old_little_endian_word_and_changes_only_the_selected_four_bytes() {
    for address in [0, 0x1001, 0x1ffd, 0x1ffe, 0x1fff, 0xffff_fffc] {
        let mut space = AddressSpace::new(4).unwrap();
        let pages = [0xffff_f000, 0x2000, 0, 0x1000];
        for (index, base) in pages.into_iter().enumerate() {
            space.map_zeroed(range(base), Permissions::ALL).unwrap();
            space
                .write(
                    GuestAddress(base),
                    &vec![0x90 + index as u8; PAGE_SIZE as usize],
                )
                .unwrap();
        }
        let first_slot = space.resolve(GuestAddress(0x1000), Access::Read).unwrap().0;
        let second_slot = space.resolve(GuestAddress(0x2000), Access::Read).unwrap().0;
        assert_ne!(second_slot, first_slot + PAGE_SIZE);
        space
            .write(GuestAddress(address), &[0x11, 0x22, 0x33, 0x44])
            .unwrap();
        let mut expected = pages.map(|base| page(&space, base));
        let snapshots = pages.map(|base| space.snapshot_code(GuestAddress(base), 1).unwrap());
        for (offset, value) in [0xdd, 0xcc, 0xbb, 0xaa].into_iter().enumerate() {
            let at = (u64::from(address) + offset as u64) as u32;
            let index = pages.iter().position(|base| *base == at & !0xfff).unwrap();
            expected[index][(at % PAGE_SIZE) as usize] = value;
        }

        assert_eq!(
            space.exchange32(GuestAddress(address), 0xaabb_ccdd),
            Ok(0x4433_2211)
        );
        assert_eq!(space.mapped_pages(), 4);
        for (index, base) in pages.into_iter().enumerate() {
            assert_eq!(
                page(&space, base),
                expected[index],
                "{address:08x}/{base:08x}"
            );
            let touched = u64::from(base) <= u64::from(address) + 3
                && u64::from(address) < u64::from(base) + u64::from(PAGE_SIZE);
            assert_eq!(space.is_code_current(&snapshots[index]), !touched);
        }
    }
}

#[test]
fn exchange_requires_read_then_write_and_preserves_every_denied_permission_mode() {
    for bits in 0..8 {
        let mut space = AddressSpace::new(1).unwrap();
        space.map_zeroed(range(0x1000), Permissions::ALL).unwrap();
        space
            .write(GuestAddress(0x1011), &[0x11, 0x22, 0x33, 0x44])
            .unwrap();
        let before = page(&space, 0x1000);
        space
            .protect(range(0x1000), Permissions::from_bits(bits).unwrap())
            .unwrap();
        let accesses = [Access::Read, Access::Write, Access::Execute];
        let resolutions = accesses.map(|access| space.resolve(GuestAddress(0x1011), access));
        let snapshot =
            (bits & 4 != 0).then(|| space.snapshot_code(GuestAddress(0x1011), 4).unwrap());
        let expected = if bits & 3 == 3 {
            Ok(0x4433_2211)
        } else {
            Err(fault(
                0x1011,
                if bits & 1 == 0 {
                    Access::Read
                } else {
                    Access::Write
                },
                FaultReason::Permission,
            ))
        };
        assert_eq!(
            space.exchange32(GuestAddress(0x1011), 0x8877_6655),
            expected,
            "{bits}"
        );
        assert_eq!(space.mapped_pages(), 1);
        assert_eq!(
            accesses.map(|access| space.resolve(GuestAddress(0x1011), access)),
            resolutions
        );
        if let Some(snapshot) = snapshot {
            assert_eq!(space.is_code_current(&snapshot), bits & 3 != 3);
        }
        space.protect(range(0x1000), Permissions::READ).unwrap();
        let mut expected_page = before;
        if bits & 3 == 3 {
            expected_page[0x11..0x15].copy_from_slice(&[0x55, 0x66, 0x77, 0x88]);
        }
        assert_eq!(page(&space, 0x1000), expected_page, "{bits}");
    }
}

#[test]
fn exchange_finishes_read_preflight_before_any_write_denial_and_never_changes_a_prefix() {
    for (first_bits, second_bits, access, at, reason) in [
        (1, Some(2), Access::Read, 0x2000, FaultReason::Permission),
        (1, None, Access::Read, 0x2000, FaultReason::Unmapped),
        (3, Some(1), Access::Write, 0x2000, FaultReason::Permission),
        (1, Some(3), Access::Write, 0x1ffe, FaultReason::Permission),
    ] {
        let mut space = AddressSpace::new(2).unwrap();
        space.map_zeroed(range(0x1000), Permissions::ALL).unwrap();
        space
            .write(GuestAddress(0x1ffc), &[0xa5, 0x5a, 0x11, 0x22])
            .unwrap();
        let before_first = page(&space, 0x1000);
        let before_second = second_bits.map(|_| {
            space.map_zeroed(range(0x2000), Permissions::ALL).unwrap();
            space
                .write(GuestAddress(0x2000), &[0x33, 0x44, 0x5a, 0xa5])
                .unwrap();
            page(&space, 0x2000)
        });
        space
            .protect(range(0x1000), Permissions::from_bits(first_bits).unwrap())
            .unwrap();
        if let Some(bits) = second_bits {
            space
                .protect(range(0x2000), Permissions::from_bits(bits).unwrap())
                .unwrap();
        }
        for _ in 0..2 {
            assert_eq!(
                space.exchange32(GuestAddress(0x1ffe), 0xdead_beef),
                Err(fault(at, access, reason))
            );
            assert_eq!(page(&space, 0x1000), before_first);
        }
        if let Some(before) = before_second {
            space.protect(range(0x2000), Permissions::READ).unwrap();
            assert_eq!(page(&space, 0x2000), before);
        }
        assert_eq!(
            space.mapped_pages(),
            if second_bits.is_some() { 2 } else { 1 }
        );
    }
}

#[test]
fn exchange_overflow_precedes_unmapped_or_denied_pages_and_never_wraps_to_zero() {
    for top_mapped in [false, true] {
        let mut space = AddressSpace::new(2).unwrap();
        space.map_zeroed(range(0), Permissions::READ_WRITE).unwrap();
        space
            .write(GuestAddress(0), &[0xa5, 0x5a, 0x33, 0x44])
            .unwrap();
        let low = page(&space, 0);
        let top = top_mapped.then(|| {
            space
                .map_zeroed(range(0xffff_f000), Permissions::ALL)
                .unwrap();
            space
                .write(GuestAddress(0xffff_fffc), &[0x11, 0x22, 0x55, 0xaa])
                .unwrap();
            let before = page(&space, 0xffff_f000);
            space
                .protect(range(0xffff_f000), Permissions::NONE)
                .unwrap();
            before
        });
        for address in [0xffff_fffd, 0xffff_fffe, 0xffff_ffff] {
            assert_eq!(
                space.exchange32(GuestAddress(address), 0xdead_beef),
                Err(fault(address, Access::Read, FaultReason::AddressOverflow))
            );
            assert_eq!(page(&space, 0), low);
        }
        if let Some(before) = top {
            space
                .protect(range(0xffff_f000), Permissions::READ)
                .unwrap();
            assert_eq!(page(&space, 0xffff_f000), before);
        }
    }
}
