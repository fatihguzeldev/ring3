use super::*;
fn range(address: u32) -> PageRange {
    PageRange::new(GuestAddress(address), 1).unwrap()
}
#[test]
fn wrapped_stack_slots_commit_once_and_max_failure_preserves_every_slot() {
    let mut memory = AddressSpace::new(2).unwrap();
    memory.map_zeroed(range(0), Permissions::ALL).unwrap();
    memory
        .map_zeroed(range(0xffff_f000), Permissions::ALL)
        .unwrap();
    let snapshot = memory.snapshot_code(GuestAddress(0), 2).unwrap();
    let version = memory.version;
    memory
        .write_stack(&[0, 0xffff_fffe], &[0x1234, 0x5678], 2)
        .unwrap();
    assert_eq!(memory.version, version + 1);
    assert!(!memory.is_code_current(&snapshot));
    let mut bytes = [0; 2];
    memory.read(GuestAddress(0), &mut bytes).unwrap();
    assert_eq!(bytes, [0x34, 0x12]);
    memory.read(GuestAddress(0xffff_fffe), &mut bytes).unwrap();
    assert_eq!(bytes, [0x78, 0x56]);
    memory.version = u64::MAX;
    let before = memory.backing.clone();
    assert_eq!(
        memory.write_stack(&[0, 0xffff_fffe], &[0xaaaa, 0xbbbb], 2),
        Err(MemoryError::VersionExhausted)
    );
    assert_eq!(memory.backing, before);
    assert_eq!(memory.version, u64::MAX);
}
#[test]
fn late_stack_permission_or_overflow_preflight_has_no_earlier_write() {
    let mut memory = AddressSpace::new(2).unwrap();
    memory.map_zeroed(range(0x1000), Permissions::ALL).unwrap();
    memory
        .map_zeroed(range(0xffff_f000), Permissions::READ)
        .unwrap();
    let before = memory.backing.clone();
    let version = memory.version;
    assert_eq!(
        memory.write_stack(&[0x1000, 0xffff_fffc], &[1, 2], 4),
        Err(fault(
            GuestAddress(0xffff_fffc),
            Access::Write,
            FaultReason::Permission
        ))
    );
    assert_eq!(
        memory.write_stack(&[0x1000, 0xffff_fffe], &[1, 2], 4),
        Err(fault(
            GuestAddress(0xffff_fffe),
            Access::Write,
            FaultReason::AddressOverflow
        ))
    );
    assert_eq!(memory.backing, before);
    assert_eq!(memory.version, version);
}
