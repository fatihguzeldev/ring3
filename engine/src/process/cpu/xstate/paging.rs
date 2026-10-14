use super::*;
use crate::cpu::x86::{CpuPagingError32, PagingAccess32};

fn engine() -> EngineInstance {
    let mut e = EngineInstance::new(3, 7).unwrap();

    for page in [0x5000, 0x6000] {
        e.map(page, 1, 3).unwrap();
    }
    e
}
#[test]
fn xsave_header_is_architectural_write_before_backing_read_and_restore_is_read() {
    let mut e = engine();
    e.unmap(0x5000, 1).unwrap();
    let before = e.arena().to_vec();
    let error = save(
        &mut e.cpu_memory_port32(),
        0x5000,
        0,
        &X87State::default(),
        &SimdState::default(),
    )
    .unwrap_err();
    let CpuMemoryError::Paging(CpuPagingError32::PageFault(fault)) = error else {
        panic!("expected certified XSAVE header PF");
    };
    assert_eq!(
        (fault.linear(), fault.error_code(), fault.access()),
        (GuestAddress(0x5200), 6, PagingAccess32::Write)
    );
    assert_eq!(e.cr2(), Some(GuestAddress(0x5200)));
    assert_eq!(e.arena(), before);
    let error = restore(
        &mut e.cpu_memory_port32(),
        0x5000,
        0,
        &X87State::default(),
        &SimdState::default(),
    )
    .unwrap_err();
    let CpuMemoryError::Paging(CpuPagingError32::PageFault(fault)) = error else {
        panic!("expected certified XRSTOR header PF");
    };
    assert_eq!(
        (fault.error_code(), fault.access()),
        (4, PagingAccess32::Read)
    );
}
#[test]
fn selected_xsave_batch_preflight_precedes_all_ram_and_versions() {
    let mut e = engine();
    // header is on6000 and succeeds, but selected x87 state spans begin in5000.
    e.protect(0x5000, 1, 1).unwrap();
    let address = 0x5e00;
    let before = e.arena().to_vec();
    let version = e.memory().unwrap().code_validation_version();
    let mut ram = [0; 4096];
    e.memory()
        .unwrap()
        .read(GuestAddress(0x6000), &mut ram)
        .unwrap();
    let error = save(
        &mut e.cpu_memory_port32(),
        address,
        1,
        &X87State::default(),
        &SimdState::default(),
    )
    .unwrap_err();
    let CpuMemoryError::Paging(CpuPagingError32::PageFault(fault)) = error else {
        panic!("expected selected x87 component write PF");
    };
    assert_eq!(
        (fault.linear(), fault.error_code()),
        (GuestAddress(address), 7)
    );
    assert_eq!(e.cr2(), Some(GuestAddress(address)));
    assert_eq!(e.arena(), before);
    let mut after = [0; 4096];
    e.memory()
        .unwrap()
        .read(GuestAddress(0x6000), &mut after)
        .unwrap();
    assert_eq!(after, ram);
    assert_eq!(e.memory().unwrap().code_validation_version(), version);
}
