use super::*;
fn range(address: u32) -> PageRange {
    PageRange::new(GuestAddress(address), 1).unwrap()
}
#[test]
fn sparse_write_has_no_access_for_clear_mask_and_one_identity_commit() {
    let mut memory = AddressSpace::new(2).unwrap();
    memory
        .map_zeroed(range(0xffff_f000), Permissions::ALL)
        .unwrap();
    memory.write(GuestAddress(0xffff_ffff), &[0x5a]).unwrap();
    let snapshot = memory.snapshot_code(GuestAddress(0xffff_ffff), 1).unwrap();
    let version = memory.version;
    let epoch = memory.code_validation_version;
    assert_eq!(
        memory.write_masked(GuestAddress(0xffff_ffff), &[0x5a; 16], 0),
        Ok(())
    );
    assert_eq!(
        (memory.version, memory.code_validation_version),
        (version, epoch)
    );
    assert!(memory.is_code_current(&snapshot));
    assert_eq!(
        memory.write_masked(GuestAddress(0xffff_ffff), &[0x5a; 16], 1),
        Ok(())
    );
    assert_eq!(memory.version, version + 1);
    assert!(!memory.is_code_current(&snapshot));
    memory.version = u64::MAX;
    assert_eq!(memory.write_masked(GuestAddress(0), &[7; 16], 0), Ok(()));
    assert_eq!(
        memory.write_masked(GuestAddress(0xffff_ffff), &[0x5a; 16], 1),
        Err(MemoryError::VersionExhausted)
    );
    assert_eq!(
        memory.write_masked(GuestAddress(0xffff_ffff), &[0x5a; 16], 2),
        Err(fault(
            GuestAddress(0xffff_ffff),
            Access::Write,
            FaultReason::AddressOverflow
        ))
    );
}
#[test]
fn selected_write_preflight_is_complete_and_never_requires_read() {
    let mut memory = AddressSpace::new(2).unwrap();
    memory
        .map_zeroed(range(0x1000), Permissions::from_bits(2).unwrap())
        .unwrap();
    let version = memory.version;
    let before = memory.backing.clone();
    assert_eq!(
        memory.write_masked(GuestAddress(0x1ff8), &[0xa5; 16], 0x101),
        Err(fault(
            GuestAddress(0x2000),
            Access::Write,
            FaultReason::Unmapped
        ))
    );
    assert_eq!(memory.backing, before);
    assert_eq!(memory.version, version);
    assert_eq!(
        memory.write_masked(GuestAddress(0x1ff8), &[0xa5; 16], 0xff),
        Ok(())
    );
    assert_eq!(memory.version, version + 1);
    memory.map_zeroed(range(0x2000), Permissions::READ).unwrap();
    memory.version = u64::MAX;
    assert_eq!(
        memory.write_masked(GuestAddress(0x1ff8), &[0xa5; 16], 0x100),
        Err(fault(
            GuestAddress(0x2000),
            Access::Write,
            FaultReason::Permission
        ))
    );
}
