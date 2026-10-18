use super::*;
use crate::memory::{PageRange, Permissions};

mod read;

fn range(address: u32) -> PageRange {
    PageRange::new(GuestAddress(address), 1).unwrap()
}
fn fault(result: Result<(), PagingCheckError32>) -> PagingFaultFacts32 {
    let Err(PagingCheckError32::Fault(facts)) = result else {
        panic!("expected address-space access fault: {result:?}");
    };
    facts
}

#[test]
fn current_mappings_and_rwx_permissions_are_the_only_access_authority() {
    let mut memory = AddressSpace::new(1).unwrap();
    let facts = fault(memory.cpu_paging_check32(
        GuestAddress(0x1011),
        4,
        Access::Read,
        PagingBacking32::Read,
    ));
    assert_eq!(
        (facts.identity(), facts.linear(), facts.present()),
        (memory.identity(), GuestAddress(0x1011), false)
    );
    memory
        .map_zeroed(range(0x1000), Permissions::READ_WRITE)
        .unwrap();
    assert_eq!(
        memory.cpu_paging_check32(GuestAddress(0x1011), 4, Access::Read, PagingBacking32::Read),
        Ok(())
    );
    let facts = fault(memory.cpu_paging_check32(
        GuestAddress(0x1011),
        4,
        Access::Execute,
        PagingBacking32::Execute,
    ));
    assert_eq!(
        (facts.linear(), facts.access(), facts.present()),
        (GuestAddress(0x1011), Access::Execute, true)
    );
    memory.protect(range(0x1000), Permissions::EXECUTE).unwrap();
    assert_eq!(
        memory.cpu_paging_check32(
            GuestAddress(0x1011),
            4,
            Access::Execute,
            PagingBacking32::Execute
        ),
        Ok(())
    );
    assert!(
        fault(memory.cpu_paging_check32(
            GuestAddress(0x1011),
            4,
            Access::Read,
            PagingBacking32::Read
        ))
        .present()
    );
    memory.unmap(range(0x1000)).unwrap();
    assert!(
        !fault(memory.cpu_paging_check32(
            GuestAddress(0x1011),
            4,
            Access::Execute,
            PagingBacking32::Execute
        ))
        .present()
    );
}

#[test]
fn rmw_checks_write_intent_and_required_read_access_before_any_payload() {
    let mut memory = AddressSpace::new(1).unwrap();
    memory.map_zeroed(range(0x1000), Permissions::READ).unwrap();
    let facts = fault(memory.cpu_paging_check32(
        GuestAddress(0x1011),
        4,
        Access::Write,
        PagingBacking32::Read,
    ));
    assert_eq!((facts.access(), facts.present()), (Access::Write, true));
    memory
        .protect(range(0x1000), Permissions::from_bits(2).unwrap())
        .unwrap();
    assert_eq!(
        memory.cpu_paging_check32(
            GuestAddress(0x1011),
            4,
            Access::Write,
            PagingBacking32::Write
        ),
        Ok(())
    );
    let facts = fault(memory.cpu_paging_check32(
        GuestAddress(0x1011),
        4,
        Access::Write,
        PagingBacking32::Read,
    ));
    assert_eq!((facts.access(), facts.present()), (Access::Write, true));
}

#[test]
fn full_span_and_wrapping_access_report_the_first_denied_byte_without_side_effects() {
    let mut memory = AddressSpace::new(2).unwrap();
    memory.map_zeroed(range(0x1000), Permissions::ALL).unwrap();
    memory
        .map_zeroed(range(0xfffff000), Permissions::ALL)
        .unwrap();
    let version = memory.code_validation_version();
    let facts = fault(memory.cpu_paging_check32(
        GuestAddress(0x1fff),
        2,
        Access::Write,
        PagingBacking32::Modify,
    ));
    assert_eq!(
        (facts.linear(), facts.present()),
        (GuestAddress(0x2000), false)
    );
    let facts = fault(memory.cpu_paging_check32(
        GuestAddress(0xfffffffe),
        4,
        Access::Read,
        PagingBacking32::Read,
    ));
    assert_eq!((facts.linear(), facts.present()), (GuestAddress(0), false));
    assert_eq!(memory.code_validation_version(), version);
}

#[test]
fn invalid_requests_stay_engine_errors_and_empty_spans_do_not_fault() {
    let memory = AddressSpace::new(1).unwrap();
    assert_eq!(
        memory.cpu_paging_check32(
            GuestAddress(u32::MAX),
            0,
            Access::Read,
            PagingBacking32::Read
        ),
        Ok(())
    );
    #[cfg(target_pointer_width = "64")]
    assert_eq!(
        memory.cpu_paging_check32(
            GuestAddress(0),
            (1_usize << 32) + 1,
            Access::Read,
            PagingBacking32::Read
        ),
        Err(PagingCheckError32::Memory(
            crate::memory::MemoryError::InvalidRange
        ))
    );
}
