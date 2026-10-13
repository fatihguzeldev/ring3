use super::super::{Access, AddressSpace, GuestAddress, MemoryError};
use crate::memory::{FaultReason, MemoryFault, PageRange, Permissions};

fn memory() -> AddressSpace {
    let mut memory = AddressSpace::new(2).unwrap();
    for address in [0xffff_f000, 0] {
        memory
            .map_zeroed(
                PageRange::new(GuestAddress(address), 1).unwrap(),
                Permissions::ALL,
            )
            .unwrap();
    }
    memory
        .write(GuestAddress(0xffff_fffe), &[0x11, 0x22])
        .unwrap();
    memory.write(GuestAddress(0), &[0x33, 0x44]).unwrap();
    memory
}

fn value(memory: &AddressSpace) -> u32 {
    let mut bytes = [0; 4];
    memory
        .cpu_read(GuestAddress(0xffff_fffe), &mut bytes)
        .unwrap();
    u32::from_le_bytes(bytes)
}

#[test]
fn wrapped_atomic_identity_mismatch_and_changed_writes_all_commit_exactly_once() {
    let mut memory = memory();
    let at = GuestAddress(0xffff_fffe);
    let mut version = memory.version;
    assert_eq!(memory.cpu_exchange32(at, 0x4433_2211), Ok(0x4433_2211));
    assert_eq!(memory.version, version + 1);
    version += 1;
    assert_eq!(
        memory.cpu_compare_exchange32(at, 0, 0xdead_beef),
        Ok(0x4433_2211)
    );
    assert_eq!(value(&memory), 0x4433_2211);
    assert_eq!(memory.version, version + 1);
    version += 1;
    assert_eq!(memory.cpu_exchange_add32(at, 0xbbcc_ddef), Ok(0x4433_2211));
    assert_eq!(value(&memory), 0);
    assert_eq!(memory.version, version + 1);
    version += 1;
    assert_eq!(memory.cpu_atomic_or_u32(at, 0x1234_5678), Ok(0));
    assert_eq!(value(&memory), 0x1234_5678);
    assert_eq!(memory.version, version + 1);
}

#[test]
fn wrapped_word_exchange_updates_low_address_but_public_exchange_stays_strict() {
    let mut memory = memory();
    assert!(matches!(
        memory.exchange16(GuestAddress(u32::MAX), 0xabcd),
        Err(MemoryError::Fault(MemoryFault {
            reason: FaultReason::AddressOverflow,
            ..
        }))
    ));
    let version = memory.version;
    assert_eq!(
        memory.cpu_exchange16(GuestAddress(u32::MAX), 0xabcd),
        Ok(0x3322)
    );
    let mut bytes = [0; 4];
    memory
        .cpu_read(GuestAddress(0xffff_fffe), &mut bytes)
        .unwrap();
    assert_eq!(bytes, [0x11, 0xcd, 0xab, 0x44]);
    assert_eq!(memory.version, version + 1);
}

#[test]
fn atomic_read_leg_fault_precedes_write_permission_and_exhaustion_without_any_mutation() {
    let mut memory = memory();
    memory
        .protect(
            PageRange::new(GuestAddress(0xffff_f000), 1).unwrap(),
            Permissions::READ,
        )
        .unwrap();
    memory
        .unmap(PageRange::new(GuestAddress(0), 1).unwrap())
        .unwrap();
    memory.exhaust_versions_for_test();
    let before = (
        memory.backing.clone(),
        memory.versions.clone(),
        memory.version,
        memory.code_validation_version,
    );
    assert_eq!(
        memory.cpu_exchange32(GuestAddress(0xffff_fffe), 0),
        Err(MemoryError::Fault(MemoryFault {
            address: GuestAddress(0),
            access: Access::Read,
            reason: FaultReason::Unmapped
        }))
    );
    assert_eq!(
        (
            memory.backing.clone(),
            memory.versions.clone(),
            memory.version,
            memory.code_validation_version
        ),
        before
    );
}
