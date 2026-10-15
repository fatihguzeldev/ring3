use super::*;
use crate::{
    cpu::x86::SegmentContext,
    memory::{PageRange, Permissions},
};

fn memory() -> AddressSpace {
    let mut memory = AddressSpace::new(2).unwrap();
    memory.map_zeroed(
        PageRange::new(GuestAddress(0x1000), 1).unwrap(),
        Permissions::ALL,
    ).unwrap();
    memory
}

#[test]
fn internal_compilation_keeps_public_semantics_and_public_snapshot_authority() {
    for bytes in [
        &[0x90][..],
        &[0xb8, 0x78, 0x56, 0x34, 0x12],
        &[0x01, 0x08],
        &[0x66, 0x0f, 0xfc, 0xc1],
        &[0xd9, 0xe8],
        &[0xcd, 0x2e],
    ] {
        let mut memory = memory();
        let pc = GuestAddress(0x1000);
        memory.write(pc, bytes).unwrap();
        let public = decode_one(&memory, pc).unwrap();
        let BoundDecode::Instruction(internal) = decode_for_compilation(
            &memory, pc, DecodePolicy::Strict, None, DecodeAuthority::default(),
        ).unwrap() else {
            panic!("complete executable instruction");
        };
        assert_eq!(internal.pc(), public.pc());
        assert_eq!(internal.length(), public.length());
        assert_eq!(internal.next_pc(), public.next_pc());
        assert_eq!(internal.operation(), public.operation());
        assert_eq!(internal.segment_accesses(), public.segment_accesses());
        assert!(memory.is_code_current(public.code_snapshot()));
        memory.write(pc, bytes).unwrap();
        assert!(!memory.is_code_current(public.code_snapshot()));
    }
}

#[test]
fn internal_fault_capture_retains_exact_prefix_and_rechecks_repaired_mapping() {
    let mut memory = memory();
    let pc = GuestAddress(0x1ffe);
    memory.write(pc, &[0xb8, 0x12]).unwrap();
    let cs = CodeFetchContext::from_segments(&SegmentContext::default()).unwrap();
    let BoundDecode::CodeFault(fault) = decode_for_compilation(
        &memory, pc, DecodePolicy::Execution, Some(cs), DecodeAuthority::default(),
    ).unwrap() else {
        panic!("captured instruction fetch fault");
    };
    assert_eq!(fault.kind, CapturedFetchFaultKind32::Paging);
    assert_eq!(fault.attempted_length, 3);
    assert_eq!(fault.consumed_prefix_length, 2);
    assert!(memory.is_code_current(fault.prefix.as_ref().unwrap()));
    assert!(fault.remains_denied(&memory, cs));
    memory.map_zeroed(
        PageRange::new(GuestAddress(0x2000), 1).unwrap(), Permissions::ALL,
    ).unwrap();
    assert!(!fault.remains_denied(&memory, cs));
    assert!(memory.is_code_current(fault.prefix.as_ref().unwrap()));
    memory.write(GuestAddress(0x1fff), &[0x34]).unwrap();
    assert!(!memory.is_code_current(fault.prefix.as_ref().unwrap()));
}
