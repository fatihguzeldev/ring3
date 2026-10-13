use super::*;
fn range(address: u32) -> PageRange {
    PageRange::new(GuestAddress(address), 1).unwrap()
}
#[test]
fn bounded_scatter_uses_one_version_and_preflights_every_span() {
    let mut memory = AddressSpace::new(2).unwrap();
    memory.map_zeroed(range(0x1000), Permissions::ALL).unwrap();
    memory.map_zeroed(range(0x2000), Permissions::ALL).unwrap();
    let snapshot = memory.snapshot_code(GuestAddress(0x1000), 8).unwrap();
    let version = memory.version;
    memory
        .write_spans(&[
            (GuestAddress(0x1000), &[1, 2]),
            (GuestAddress(0x2000), &[3, 4]),
        ])
        .unwrap();
    assert_eq!(memory.version, version + 1);
    assert!(!memory.is_code_current(&snapshot));
    let mut bytes = [0; 2];
    memory.read(GuestAddress(0x1000), &mut bytes).unwrap();
    assert_eq!(bytes, [1, 2]);
    memory.read(GuestAddress(0x2000), &mut bytes).unwrap();
    assert_eq!(bytes, [3, 4]);
    memory.protect(range(0x2000), Permissions::READ).unwrap();
    let before = memory.backing.clone();
    let version = memory.version;
    assert_eq!(
        memory.write_spans(&[
            (GuestAddress(0x1000), &[5, 6]),
            (GuestAddress(0x2000), &[7, 8])
        ]),
        Err(fault(
            GuestAddress(0x2000),
            Access::Write,
            FaultReason::Permission
        ))
    );
    assert_eq!(memory.backing, before);
    assert_eq!(memory.version, version);
    memory.version = u64::MAX;
    assert_eq!(
        memory.write_spans(&[(GuestAddress(0x1000), &[5, 6])]),
        Err(MemoryError::VersionExhausted)
    );
    assert_eq!(memory.backing, before);
    assert_eq!(
        memory.write_spans(&[(GuestAddress(0), &[] as &[u8]); 6]),
        Err(MemoryError::InvalidRange)
    );
    assert_eq!(memory.write_spans(&[]), Ok(()));
}
