use super::*;
use crate::process::EngineInstance;

fn engine() -> EngineInstance {
    let mut e = EngineInstance::new(3, 7).unwrap();
    for page in [0x1000, 0x7000] {
        e.map(page, 1, 3).unwrap();
    }
    e
}

#[test]
fn inspection_returns_actual_typed_fault_without_cr2_until_runtime_records_it() {
    let mut e = engine();
    e.protect(0x1000, 1, 1).unwrap();
    let before = e.arena().to_vec();
    let mut output = [0xa5; 4];
    let error = e
        .inspect_cpu_memory_port32()
        .read_modify(GuestAddress(0x1000), &mut output)
        .unwrap_err();
    let CpuPagingError32::PageFault(fault) = error else {
        panic!("expected certified PF")
    };
    assert_eq!(
        (fault.error_code(), fault.access()),
        (7, PagingAccess32::Write)
    );
    assert_eq!(e.cr2(), None);
    assert_eq!(e.arena(), before);
    assert_eq!(output, [0xa5; 4]);
    e.record_cpu_paging_error32(error);
    assert_eq!(e.cr2(), Some(GuestAddress(0x1000)));
}

#[test]
fn detecting_port_updates_cr2_and_preserves_full_atomic_stack_or_scatter_transaction() {
    let mut e = engine();
    let before = e.arena().to_vec();
    let mut original = [0; 16];
    e.memory()
        .unwrap()
        .read(GuestAddress(0x7000), &mut original)
        .unwrap();
    let version = e.memory().unwrap().code_validation_version();
    let error = e
        .cpu_memory_port32()
        .write_stack(&[0x7008, 0x7004, 0x7000, 0x6ffc], &[1, 2, 3, 4], 4)
        .unwrap_err();
    let CpuPagingError32::PageFault(fault) = error else {
        panic!("expected certified PF")
    };
    assert_eq!(
        (fault.linear(), fault.error_code()),
        (GuestAddress(0x6ffc), 6)
    );
    assert_eq!(e.cr2(), Some(GuestAddress(0x6ffc)));
    assert_eq!(e.arena(), before);
    let mut after = [0; 16];
    e.memory()
        .unwrap()
        .read(GuestAddress(0x7000), &mut after)
        .unwrap();
    assert_eq!(after, original);
    assert_eq!(e.memory().unwrap().code_validation_version(), version);
    let error = e
        .cpu_memory_port32()
        .write_spans(&[(GuestAddress(0x7000), &[1; 4]), (GuestAddress(0), &[2; 4])])
        .unwrap_err();
    assert!(matches!(error, CpuPagingError32::PageFault(_)));
    assert_eq!(e.cr2(), Some(GuestAddress(0)));
    assert_eq!(e.memory().unwrap().code_validation_version(), version);
    e.memory()
        .unwrap()
        .read(GuestAddress(0x7000), &mut after)
        .unwrap();
    assert_eq!(after, original);
}

#[test]
fn map_and_protect_changes_apply_immediately_to_inspection_and_detection() {
    let mut e = engine();
    e.protect(0x1000, 1, 4).unwrap();
    let mut output = [0xa5; 4];
    assert!(matches!(
        e.inspect_cpu_memory_port32()
            .read(GuestAddress(0x1000), &mut output),
        Err(CpuPagingError32::PageFault(_))
    ));
    assert_eq!(e.cr2(), None);
    assert_eq!(output, [0xa5; 4]);
    e.protect(0x1000, 1, 3).unwrap();
    e.inspect_cpu_memory_port32()
        .read(GuestAddress(0x1000), &mut output)
        .unwrap();
    assert_eq!(output, [0; 4]);
    output.fill(0xa5);
    assert!(matches!(
        e.cpu_memory_port32()
            .read(GuestAddress(0x4000), &mut output),
        Err(CpuPagingError32::PageFault(_))
    ));
    assert_eq!(output, [0xa5; 4]);
    assert_eq!(e.cr2(), Some(GuestAddress(0x4000)));
}

#[test]
fn combined_read_keeps_wrapped_rmw_fault_atomic_and_records_the_first_linear_address() {
    let mut e = EngineInstance::new(2, 7).unwrap();
    for page in [0xffff_f000, 0] {
        e.map(page, 1, 3).unwrap();
    }
    e.memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0xffff_fffe), &[0x11, 0x22])
        .unwrap();
    e.memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0), &[0x33, 0x44])
        .unwrap();
    e.protect(0, 1, 2).unwrap();
    let before = e.arena().to_vec();
    let version = e.memory().unwrap().code_validation_version();
    let mut output = [0xa5; 4];
    let error = e
        .cpu_memory_port32()
        .read_modify(GuestAddress(0xffff_fffe), &mut output)
        .unwrap_err();
    let CpuPagingError32::PageFault(fault) = error else {
        panic!("expected application page fault")
    };
    assert_eq!(
        (fault.linear(), fault.error_code(), fault.access()),
        (GuestAddress(0), 7, PagingAccess32::Write)
    );
    assert_eq!(output, [0xa5; 4]);
    assert_eq!(e.arena(), before);
    assert_eq!(e.cr2(), Some(GuestAddress(0)));
    assert_eq!(e.memory().unwrap().code_validation_version(), version);

    e.protect(0, 1, 3).unwrap();
    e.cpu_memory_port32()
        .read_modify(GuestAddress(0xffff_fffe), &mut output)
        .unwrap();
    assert_eq!(output, [0x11, 0x22, 0x33, 0x44]);
    assert_eq!(e.cr2(), Some(GuestAddress(0)));
}
