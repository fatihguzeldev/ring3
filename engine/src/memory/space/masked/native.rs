use super::*;
fn page(address: u32) -> PageRange {
    PageRange::new(GuestAddress(address), 1).unwrap()
}

#[test]
fn wide_selected_read_preserves_disabled_output_and_never_advances_versions() {
    let mut memory = AddressSpace::new(2).unwrap();
    for at in [0x1000, 0x2000] {
        memory.map_zeroed(page(at), Permissions::ALL).unwrap();
    }
    let input: [u8; 32] = std::array::from_fn(|i| (i * 17 + 3) as u8);
    memory.write(GuestAddress(0x1ff0), &input).unwrap();
    let snapshot = memory.snapshot_code(GuestAddress(0x1ff0), 32).unwrap();
    let version = memory.version;
    let epoch = memory.code_validation_version;
    for mask in [0, 1, 1 << 31, 0x80008001, 0xaaaa5555, u32::MAX] {
        let mut output = [0xa5; 32];
        let mut expected = output;
        for i in 0..32 {
            if mask & (1 << i) != 0 {
                expected[i] = input[i];
            }
        }
        assert_eq!(
            memory.read_masked(GuestAddress(0x1ff0), &mut output, mask),
            Ok(())
        );
        assert_eq!(output, expected);
        assert_eq!(
            (memory.version, memory.code_validation_version),
            (version, epoch)
        );
        assert!(memory.is_code_current(&snapshot));
    }
}
#[test]
fn wide_selected_store_uses_one_identity_commit_and_no_read_permission() {
    let mut memory = AddressSpace::new(2).unwrap();
    for at in [0x1000, 0x2000] {
        memory.map_zeroed(page(at), Permissions::ALL).unwrap();
    }
    let input: [u8; 32] = std::array::from_fn(|i| (i * 17 + 3) as u8);
    memory.write(GuestAddress(0x1ff0), &input).unwrap();
    let snapshot = memory.snapshot_code(GuestAddress(0x1ff0), 32).unwrap();
    let version = memory.version;
    memory
        .write_masked(GuestAddress(0x1ff0), &input, 0x80000001)
        .unwrap();
    assert_eq!(memory.version, version + 1);
    assert_eq!(memory.code_validation_version, version + 1);
    assert!(!memory.is_code_current(&snapshot));
    assert_eq!(
        memory.versions[memory.mappings[1] as usize - 1].content,
        version + 1
    );
    assert_eq!(
        memory.versions[memory.mappings[2] as usize - 1].content,
        version + 1
    );
    for at in [0x1000, 0x2000] {
        memory
            .protect(page(at), Permissions::from_bits(2).unwrap())
            .unwrap();
    }
    let version = memory.version;
    let before = memory.backing.clone();
    memory
        .write_masked(GuestAddress(0x1ff0), &[0x6b; 32], 0x80000001)
        .unwrap();
    assert_eq!(memory.version, version + 1);
    for i in 0..32 {
        let address = 0x1ff0 + i;
        let slot = memory.mappings[address / 4096] as usize - 1;
        let at = slot * 4096 + address % 4096;
        assert_eq!(
            memory.backing[at],
            if i == 0 || i == 31 { 0x6b } else { before[at] }
        );
    }
}
#[test]
fn all_selected_accesses_preflight_before_output_or_ram_publication() {
    let mut memory = AddressSpace::new(2).unwrap();
    memory.map_zeroed(page(0x1000), Permissions::ALL).unwrap();
    memory.write(GuestAddress(0x1ff0), &[0x6b; 16]).unwrap();
    let before = memory.backing.clone();
    let version = memory.version;
    let mut output = [0xa5; 32];
    for access in [Access::Read, Access::Write] {
        let error = Err(fault(GuestAddress(0x200f), access, FaultReason::Unmapped));
        let result = if access == Access::Read {
            memory.read_masked(GuestAddress(0x1ff0), &mut output, 0x80000001)
        } else {
            memory.write_masked(GuestAddress(0x1ff0), &[7; 32], 0x80000001)
        };
        assert_eq!(result, error);
        assert_eq!(output, [0xa5; 32]);
        assert_eq!(memory.backing, before);
        assert_eq!(memory.version, version);
    }
    memory.map_zeroed(page(0x2000), Permissions::READ).unwrap();
    let version = memory.version;
    let before = memory.backing.clone();
    assert_eq!(
        memory.write_masked(GuestAddress(0x1ff0), &[7; 32], 0x80000001),
        Err(fault(
            GuestAddress(0x200f),
            Access::Write,
            FaultReason::Permission
        ))
    );
    assert_eq!(memory.backing, before);
    assert_eq!(memory.version, version);
    memory
        .protect(page(0x1000), Permissions::from_bits(2).unwrap())
        .unwrap();
    assert_eq!(
        memory.read_masked(GuestAddress(0x1ff0), &mut output, 0x80000001),
        Err(fault(
            GuestAddress(0x1ff0),
            Access::Read,
            FaultReason::Permission
        ))
    );
    assert_eq!(output, [0xa5; 32]);
}
#[test]
fn zero_mask_avoids_nominal_span_overflow_and_version_exhaustion() {
    let mut memory = AddressSpace::new(1).unwrap();
    memory.version = u64::MAX;
    let mut output = [0xa5; 32];
    assert_eq!(
        memory.read_masked(GuestAddress(u32::MAX), &mut output, 0),
        Ok(())
    );
    assert_eq!(output, [0xa5; 32]);
    assert_eq!(
        memory.write_masked(GuestAddress(u32::MAX), &[7; 32], 0),
        Ok(())
    );
    assert_eq!(memory.version, u64::MAX);
    for access in [Access::Read, Access::Write] {
        let result = if access == Access::Read {
            memory.read_masked(GuestAddress(u32::MAX), &mut output, 0x80000001)
        } else {
            memory.write_masked(GuestAddress(u32::MAX), &[7; 32], 0x80000001)
        };
        assert_eq!(
            result,
            Err(fault(
                GuestAddress(u32::MAX),
                access,
                FaultReason::AddressOverflow
            ))
        );
        assert_eq!(output, [0xa5; 32]);
    }
    for (length, mask) in [(0, 0), (33, 0), (16, 1 << 16), (31, 1 << 31)] {
        let mut output = vec![0xa5; length];
        assert_eq!(
            memory.read_masked(GuestAddress(0), &mut output, mask),
            Err(MemoryError::InvalidRange)
        );
        assert_eq!(
            memory.write_masked(GuestAddress(0), &output, mask),
            Err(MemoryError::InvalidRange)
        );
        assert_eq!(output, vec![0xa5; length]);
    }
}
