use super::*;
use crate::memory::{FaultReason, PageRange, Permissions};

fn memory(low: bool) -> AddressSpace {
    let mut memory = AddressSpace::new(2).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0xffff_f000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory
        .write(GuestAddress(0xffff_fffe), &[0x11, 0x22])
        .unwrap();
    if low {
        memory
            .map_zeroed(
                PageRange::new(GuestAddress(0), 1).unwrap(),
                Permissions::ALL,
            )
            .unwrap();
        memory.write(GuestAddress(0), &[0x33, 0x44]).unwrap();
    }
    memory
}

#[test]
fn cpu_wrap_reads_preserve_host_contract_and_failure_neutrality() {
    let mapped = memory(true);
    let mut output = [0xa5; 4];
    mapped
        .cpu_read(GuestAddress(0xffff_fffe), &mut output)
        .unwrap();
    assert_eq!(output, [0x11, 0x22, 0x33, 0x44]);
    output.fill(0xa5);
    assert!(mapped.read(GuestAddress(0xffff_fffe), &mut output).is_err());
    assert_eq!(output, [0xa5; 4]);
    let memory = memory(false);
    let error = memory
        .cpu_read(GuestAddress(0xffff_fffe), &mut output)
        .unwrap_err();
    assert!(
        matches!(error, MemoryError::Fault(fault) if fault.address == GuestAddress(0) && fault.reason == FaultReason::Unmapped)
    );
    assert_eq!(output, [0xa5; 4]);
    assert_eq!(memory.mapped_pages(), 1);
}

#[test]
fn selected_cpu_wrap_reads_probe_only_selected_bytes() {
    let memory = memory(false);
    let mut output = [0xa5; 4];
    memory
        .cpu_read_masked(GuestAddress(0xffff_fffe), &mut output, 3)
        .unwrap();
    assert_eq!(output, [0x11, 0x22, 0xa5, 0xa5]);
    output.fill(0xa5);
    assert!(
        memory
            .cpu_read_masked(GuestAddress(0xffff_fffe), &mut output, 5)
            .is_err()
    );
    assert_eq!(output, [0xa5; 4]);
    memory
        .cpu_read_masked(GuestAddress(0xffff_fffe), &mut output, 0)
        .unwrap();
    assert_eq!(output, [0xa5; 4]);
}
