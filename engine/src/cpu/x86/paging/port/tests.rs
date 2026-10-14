use super::*;
use crate::memory::{AddressSpace, PageRange, Permissions};

fn range(address: u32) -> PageRange {
    PageRange::new(GuestAddress(address), 1).unwrap()
}

fn page_fault(error: CpuPagingError32, linear: u32, access: PagingAccess32, code: u32) {
    let CpuPagingError32::PageFault(fault) = error else {
        panic!("expected application access fault: {error:?}");
    };
    assert_eq!(fault.linear(), GuestAddress(linear));
    assert_eq!(fault.access(), access);
    assert_eq!(fault.error_code(), code);
}

#[test]
fn mappings_and_permissions_always_authorize_cpu_access_without_configuration() {
    let mut memory = AddressSpace::new(2).unwrap();
    let mut output = [0xa5; 4];
    page_fault(
        DefaultCpuReadPort32::new(&memory)
            .read(GuestAddress(0x1010), &mut output)
            .unwrap_err(),
        0x1010,
        PagingAccess32::Read,
        4,
    );
    assert_eq!(output, [0xa5; 4]);
    memory
        .map_zeroed(range(0x1000), Permissions::READ_WRITE)
        .unwrap();
    DefaultCpuMemoryPort32::new(&mut memory)
        .write(GuestAddress(0x1010), &[1, 2, 3, 4])
        .unwrap();
    memory.protect(range(0x1000), Permissions::READ).unwrap();
    page_fault(
        DefaultCpuReadPort32::new(&memory)
            .read_modify(GuestAddress(0x1010), &mut output)
            .unwrap_err(),
        0x1010,
        PagingAccess32::Write,
        7,
    );
    assert_eq!(output, [0xa5; 4]);
    DefaultCpuReadPort32::new(&memory)
        .read(GuestAddress(0x1010), &mut output)
        .unwrap();
    assert_eq!(output, [1, 2, 3, 4]);
    memory.unmap(range(0x1000)).unwrap();
    page_fault(
        DefaultCpuReadPort32::new(&memory)
            .check(GuestAddress(0x1010), 4, PagingAccess32::Execute)
            .unwrap_err(),
        0x1010,
        PagingAccess32::Execute,
        20,
    );
}

#[test]
fn wraparound_and_late_write_failures_publish_no_partial_state() {
    let mut memory = AddressSpace::new(2).unwrap();
    memory
        .map_zeroed(range(0xfffff000), Permissions::ALL)
        .unwrap();
    memory.map_zeroed(range(0), Permissions::ALL).unwrap();
    DefaultCpuMemoryPort32::new(&mut memory)
        .write(GuestAddress(0xfffffffe), &[1, 2, 3, 4])
        .unwrap();
    memory.protect(range(0), Permissions::READ).unwrap();
    let version = memory.code_validation_version();
    page_fault(
        DefaultCpuMemoryPort32::new(&mut memory)
            .write(GuestAddress(0xfffffffe), &[9; 4])
            .unwrap_err(),
        0,
        PagingAccess32::Write,
        7,
    );
    let mut output = [0; 4];
    DefaultCpuReadPort32::new(&memory)
        .read(GuestAddress(0xfffffffe), &mut output)
        .unwrap();
    assert_eq!(output, [1, 2, 3, 4]);
    assert_eq!(memory.code_validation_version(), version);
}

#[test]
fn disabled_mask_and_empty_transaction_do_not_access_memory_or_consume_versions() {
    let mut memory = AddressSpace::new(1).unwrap();
    let version = memory.code_validation_version();
    let mut output = [0xa5; 16];
    DefaultCpuReadPort32::new(&memory)
        .read_masked(GuestAddress(0), &mut output, 0)
        .unwrap();
    assert_eq!(output, [0xa5; 16]);
    let mut port = DefaultCpuMemoryPort32::new(&mut memory);
    port.write_masked(GuestAddress(0), &[0; 16], 0).unwrap();
    port.write_spans(&[]).unwrap();
    port.write_stack(&[], &[], 4).unwrap();
    assert_eq!(memory.code_validation_version(), version);
}
