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

fn state(memory: &AddressSpace) -> impl std::fmt::Debug + PartialEq + use<> {
    (
        memory.identity,
        memory.version,
        memory.code_validation_version,
        memory.backing.clone(),
        memory.mappings.clone(),
        memory.permissions.clone(),
        memory.versions.clone(),
        memory.free.clone(),
    )
}

fn fault(address: u32, access: Access, reason: FaultReason) -> MemoryError {
    MemoryError::Fault(MemoryFault {
        address: GuestAddress(address),
        access,
        reason,
    })
}

#[test]
fn wrapped_write_uses_both_pages_one_version_and_keeps_host_contract_strict() {
    let mut memory = memory();
    let before = state(&memory);
    assert_eq!(
        memory.write(GuestAddress(0xffff_fffe), &[0xef, 0xcd, 0xab, 0x89]),
        Err(fault(
            0xffff_fffe,
            Access::Write,
            FaultReason::AddressOverflow
        ))
    );
    assert_eq!(state(&memory), before);
    let high = memory.snapshot_code(GuestAddress(0xffff_fffe), 2).unwrap();
    let low = memory.snapshot_code(GuestAddress(0), 2).unwrap();
    let version = memory.version;
    memory
        .cpu_write(GuestAddress(0xffff_fffe), &[0xef, 0xcd, 0xab, 0x89])
        .unwrap();
    let mut value = [0; 4];
    memory
        .cpu_read(GuestAddress(0xffff_fffe), &mut value)
        .unwrap();
    assert_eq!(value, [0xef, 0xcd, 0xab, 0x89]);
    assert_eq!(memory.version, version + 1);
    assert!(!memory.is_code_current(&high));
    assert!(!memory.is_code_current(&low));
    for address in [0xffff_fffe_u32, 0] {
        let slot = memory.mappings[address as usize / 4096] as usize - 1;
        assert_eq!(memory.versions[slot].content, version + 1);
    }
}

#[test]
fn all_mapping_and_permission_probes_precede_version_exhaustion_and_publication() {
    for (low_mapped, low_writable, exhausted, expected) in [
        (
            false,
            false,
            true,
            fault(0, Access::Write, FaultReason::Unmapped),
        ),
        (
            true,
            false,
            true,
            fault(0, Access::Write, FaultReason::Permission),
        ),
        (true, true, true, MemoryError::VersionExhausted),
    ] {
        let mut memory = memory();
        if !low_mapped {
            memory
                .unmap(PageRange::new(GuestAddress(0), 1).unwrap())
                .unwrap();
        } else if !low_writable {
            memory
                .protect(
                    PageRange::new(GuestAddress(0), 1).unwrap(),
                    Permissions::READ,
                )
                .unwrap();
        }
        if exhausted {
            memory.exhaust_versions_for_test();
        }
        let before = state(&memory);
        assert_eq!(
            memory.cpu_write(GuestAddress(0xffff_fffe), &[0xaa; 4]),
            Err(expected)
        );
        assert_eq!(state(&memory), before);
    }
}

#[test]
fn wrapped_scatter_preflights_all_spans_and_retains_overlap_order_with_one_version() {
    let mut memory = memory();
    let version = memory.version;
    memory
        .cpu_write_spans(&[
            (GuestAddress(0xffff_fffe), &[0xaa, 0xbb, 0xcc, 0xdd]),
            (GuestAddress(0), &[0xee, 0xff]),
        ])
        .unwrap();
    let mut value = [0; 4];
    memory
        .cpu_read(GuestAddress(0xffff_fffe), &mut value)
        .unwrap();
    assert_eq!(value, [0xaa, 0xbb, 0xee, 0xff]);
    assert_eq!(memory.version, version + 1);
    memory
        .protect(
            PageRange::new(GuestAddress(0), 1).unwrap(),
            Permissions::READ,
        )
        .unwrap();
    let before = state(&memory);
    assert_eq!(
        memory.cpu_write_spans(&[
            (GuestAddress(0xffff_fffc), &[1, 2]),
            (GuestAddress(0xffff_fffe), &[3, 4, 5, 6]),
        ]),
        Err(fault(0, Access::Write, FaultReason::Permission))
    );
    assert_eq!(state(&memory), before);
}

#[test]
fn compound_stack_transaction_wraps_each_slot_without_a_second_version() {
    let mut memory = memory();
    let version = memory.version;
    memory
        .cpu_write_stack(&[0xffff_fffe_u32, 0], &[0x1122_3344, 0xaabb_ccdd], 4)
        .unwrap();
    let mut value = [0; 6];
    memory
        .cpu_read(GuestAddress(0xffff_fffe), &mut value)
        .unwrap();
    assert_eq!(value, [0x44, 0x33, 0xdd, 0xcc, 0xbb, 0xaa]);
    assert_eq!(memory.version, version + 1);
    let before = state(&memory);
    assert_eq!(
        memory.cpu_write_stack(&[0xffff_fffe], &[], 4),
        Err(MemoryError::InvalidRange)
    );
    assert_eq!(state(&memory), before);
}

#[test]
fn selected_writes_probe_only_enabled_bytes_and_empty_masks_have_no_effect() {
    let mut memory = memory();
    memory
        .unmap(PageRange::new(GuestAddress(0), 1).unwrap())
        .unwrap();
    let version = memory.version;
    memory
        .cpu_write_masked(GuestAddress(0xffff_fffe), &[0xaa, 0xbb, 0xcc, 0xdd], 3)
        .unwrap();
    let mut high = [0; 2];
    memory.read(GuestAddress(0xffff_fffe), &mut high).unwrap();
    assert_eq!(high, [0xaa, 0xbb]);
    assert_eq!(memory.version, version + 1);
    let before = state(&memory);
    memory
        .cpu_write_masked(GuestAddress(0xffff_fffe), &[0; 4], 0)
        .unwrap();
    assert_eq!(state(&memory), before);
    assert_eq!(
        memory.cpu_write_masked(GuestAddress(0xffff_fffe), &[0; 4], 5),
        Err(fault(0, Access::Write, FaultReason::Unmapped))
    );
    assert_eq!(state(&memory), before);
}
