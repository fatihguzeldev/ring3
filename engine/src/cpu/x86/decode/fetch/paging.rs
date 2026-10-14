use super::*;
use crate::{
    cpu::x86::decode::{DecodeError, DecodePolicy, decode_bound, decode_one},
    memory::{PageRange, Permissions},
};

fn memory() -> AddressSpace {
    let mut memory = AddressSpace::new(2).unwrap();
    for address in [0, 0xffff_f000] {
        memory
            .map_zeroed(
                PageRange::new(GuestAddress(address), 1).unwrap(),
                Permissions::ALL,
            )
            .unwrap();
    }
    memory
}
fn context(default32: bool) -> CodeFetchContext {
    let mut segments = SegmentContext::default();
    segments.state.bases[SegmentRegister::Cs.index()] = 0xffff_efff;
    if !default32 {
        segments.caches[SegmentRegister::Cs.index()] =
            SegmentCache::from_bits(0xf0bf_ffff).unwrap();
    }
    CodeFetchContext::from_segments(&segments).unwrap()
}
#[test]
fn pure_fetch_returns_exact_execute_pf_without_register_detection_and_preserves_decode_key() {
    for default32 in [false, true] {
        let mut memory = memory();
        memory.write(GuestAddress(u32::MAX), &[0xb8]).unwrap();
        memory
            .write(GuestAddress(0), &[0x34, 0x12, 0x78, 0x56])
            .unwrap();
        let low_page = PageRange::new(GuestAddress(0), 1).unwrap();
        memory.unmap(low_page).unwrap();
        let cs = context(default32);
        let mut segments = SegmentContext::default();
        segments.state.bases[1] = 0xffff_efff;
        segments.caches[1] =
            SegmentCache::from_bits(if default32 { 0xf2bf_ffff } else { 0xf0bf_ffff }).unwrap();
        assert_eq!(
            cs.decode_key(),
            CodeFetchContext::from_segments(&segments)
                .unwrap()
                .decode_key()
        );
        let error =
            decode_bound(&memory, GuestAddress(0x1000), cs, DecodePolicy::Strict).unwrap_err();
        let DecodeError::Paging {
            pc,
            error: CpuPagingError32::PageFault(fault),
            attempted_length,
        } = error
        else {
            panic!("expected missing low translated byte");
        };
        assert_eq!((pc, attempted_length), (GuestAddress(0x1000), 2));
        assert_eq!(
            (
                fault.linear(),
                fault.error_code(),
                fault.access(),
                fault.domain()
            ),
            (
                GuestAddress(0),
                20,
                PagingAccess32::Execute,
                crate::cpu::x86::PagingDomain32::User
            )
        );
        let BoundDecode::CodeFault(fault) =
            decode_bound(&memory, GuestAddress(0x1000), cs, DecodePolicy::Execution).unwrap()
        else {
            panic!("expected consumed-prefix paging terminal")
        };
        assert_eq!(fault.kind, CapturedFetchFaultKind32::Paging);
        assert_eq!(
            (fault.attempted_length, fault.consumed_prefix_length),
            (2, 1)
        );
        assert!(memory.is_code_current(fault.prefix.as_ref().unwrap()));
        assert!(fault.remains_denied(&memory, cs));
        memory.map_zeroed(low_page, Permissions::ALL).unwrap();
        memory
            .write(GuestAddress(0), &[0x34, 0x12, 0x78, 0x56])
            .unwrap();
        assert!(!fault.remains_denied(&memory, cs));
        assert!(memory.is_code_current(fault.prefix.as_ref().unwrap()));
        let BoundDecode::Instruction(instruction) =
            decode_bound(&memory, GuestAddress(0x1000), cs, DecodePolicy::Execution).unwrap()
        else {
            panic!("expected repaired MOV")
        };
        assert_eq!(instruction.length(), if default32 { 5 } else { 3 });
        memory
            .unmap(PageRange::new(GuestAddress(0xffff_f000), 1).unwrap())
            .unwrap();
        assert!(!memory.is_code_current(fault.prefix.as_ref().unwrap()));
    }
}
#[test]
fn source_free_execute_denial_repairs_and_logical_cs_priority_is_preserved() {
    let mut memory = memory();
    memory.write(GuestAddress(u32::MAX), &[0x90]).unwrap();
    let high_page = PageRange::new(GuestAddress(0xffff_f000), 1).unwrap();
    memory.protect(high_page, Permissions::READ).unwrap();
    let cs = context(true);
    let BoundDecode::CodeFault(fault) =
        decode_bound(&memory, GuestAddress(0x1000), cs, DecodePolicy::Execution).unwrap()
    else {
        panic!("expected source-free execute denial")
    };
    assert_eq!(fault.kind, CapturedFetchFaultKind32::Paging);
    assert_eq!(
        (fault.attempted_length, fault.consumed_prefix_length),
        (1, 0)
    );
    assert!(fault.prefix.is_none());
    assert!(fault.remains_denied(&memory, cs));
    memory.protect(high_page, Permissions::ALL).unwrap();
    assert!(!fault.remains_denied(&memory, cs));
    let mut segments = SegmentContext::default();
    segments.caches[1] = SegmentCache::from_bits(0xf2b0_0fff).unwrap();
    let limited = CodeFetchContext::from_segments(&segments).unwrap();
    let BoundDecode::CodeFault(fault) = decode_bound(
        &memory,
        GuestAddress(0x1000),
        limited,
        DecodePolicy::Execution,
    )
    .unwrap() else {
        panic!("expected logical CS denial before page access")
    };
    assert_eq!(fault.kind, CapturedFetchFaultKind32::Segment);
    assert!(fault.prefix.is_none());
}
#[test]
fn standalone_decode_uses_mapping_permissions_without_extra_runtime_configuration() {
    let mut memory = memory();
    memory.write(GuestAddress(0), &[0x90]).unwrap();
    let low_page = PageRange::new(GuestAddress(0), 1).unwrap();
    memory.protect(low_page, Permissions::READ).unwrap();
    assert!(matches!(
        decode_one(&memory, GuestAddress(0)),
        Err(DecodeError::Paging {
            attempted_length: 1,
            error: CpuPagingError32::PageFault(fault),
            ..
        }) if fault.linear() == GuestAddress(0) && fault.access() == PagingAccess32::Execute && fault.error_code() == 21
    ));
    memory.protect(low_page, Permissions::ALL).unwrap();
    assert!(decode_one(&memory, GuestAddress(0)).is_ok());
}
