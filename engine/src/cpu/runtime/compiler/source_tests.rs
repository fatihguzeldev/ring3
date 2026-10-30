use super::*;
use crate::memory::{Access, FaultReason, MemoryFault, PageRange, Permissions};

fn page(address: u32) -> PageRange {
    PageRange::new(GuestAddress(address), 1).unwrap()
}

fn mapped(pages: &[u32]) -> AddressSpace {
    let mut memory = AddressSpace::new(3).unwrap();
    for &address in pages {
        memory.map_zeroed(page(address), Permissions::ALL).unwrap();
    }
    memory
}

#[test]
fn decoded_block_sources_cover_both_pages_and_preserve_mapping_identity() {
    for changed in [0x1ffe, 0x2000] {
        let mut memory = mapped(&[0x1000, 0x2000, 0x5000]);
        memory
            .write(GuestAddress(0x1ffe), &[0x90, 0x90, 0x90, 0xc3])
            .unwrap();
        let unit = compile(&memory, GuestAddress(0x1ffe), 64).unwrap();
        assert!(unit.is_current(&memory));
        memory.write(GuestAddress(0x5000), &[0xcc]).unwrap();
        assert!(unit.is_current(&memory));
        memory.write(GuestAddress(changed), &[0x90]).unwrap();
        assert!(!unit.is_current(&memory));
    }

    let mut memory = mapped(&[0x1000, 0x2000]);
    memory
        .write(GuestAddress(0x1ffe), &[0x90, 0x90, 0x90, 0xc3])
        .unwrap();
    let unit = compile(&memory, GuestAddress(0x1ffe), 64).unwrap();
    memory.protect(page(0x2000), Permissions::READ).unwrap();
    assert!(!unit.is_current(&memory));
    memory.protect(page(0x2000), Permissions::ALL).unwrap();
    assert!(!unit.is_current(&memory));
    let remapped = compile(&memory, GuestAddress(0x1ffe), 64).unwrap();
    memory.unmap(page(0x2000)).unwrap();
    memory.map_zeroed(page(0x2000), Permissions::ALL).unwrap();
    memory.write(GuestAddress(0x2000), &[0x90, 0xc3]).unwrap();
    assert!(!remapped.is_current(&memory));
}

#[test]
fn partial_decode_and_instruction_limits_do_not_claim_the_unreached_page() {
    let mut memory = mapped(&[0x1000]);
    memory.write(GuestAddress(0x1ffe), &[0x90, 0x0f]).unwrap();
    let partial = compile(&memory, GuestAddress(0x1ffe), 64).unwrap();
    assert_eq!(partial.instruction_count, 1);
    let failure = compile(&memory, GuestAddress(0x1fff), 64).unwrap_err();
    assert!(
        matches!(
            failure,
            CompileError::Decode(DecodeError::Paging {
                pc: GuestAddress(0x1fff),
                error: crate::cpu::x86::CpuPagingError32::PageFault(fault),
                attempted_length: 2,
            }) if fault.linear() == GuestAddress(0x2000) && fault.error_code() == 20
        ),
        "unexpected first-instruction fetch failure: {failure:?}"
    );
    memory.map_zeroed(page(0x2000), Permissions::ALL).unwrap();
    memory.write(GuestAddress(0x2000), &[0x0b]).unwrap();
    assert!(partial.is_current(&memory));

    let mut memory = mapped(&[0x1000]);
    memory.write(GuestAddress(0x1fff), &[0x90]).unwrap();
    let gates = [(GuestAddress(0x2000), 0), (GuestAddress(0x2000), 1)];
    let limited = compile_with_gates(&memory, GuestAddress(0x1fff), 1, &gates).unwrap();
    assert_eq!(limited.instruction_count, 1);
    memory.map_zeroed(page(0x2000), Permissions::ALL).unwrap();
    memory.write(GuestAddress(0x2000), &[0xcc]).unwrap();
    assert!(limited.is_current(&memory));

    let mut memory = mapped(&[0x1000]);
    memory.write(GuestAddress(0x1fc0), &[0x90; 64]).unwrap();
    let capped = compile(&memory, GuestAddress(0x1fc0), 4096).unwrap();
    assert_eq!(capped.instruction_count, 64);
    memory.map_zeroed(page(0x2000), Permissions::ALL).unwrap();
    assert!(capped.is_current(&memory));
}

#[test]
fn gate_source_includes_the_validated_tail_and_retains_error_priority() {
    let gate = (GuestAddress(0x1fff), 17);
    let mut memory = mapped(&[0x1000, 0x2000]);
    memory
        .write(GuestAddress(0x1ffe), &[0x90, 0x0f, 0x0b])
        .unwrap();
    let unit = compile_with_gates(&memory, GuestAddress(0x1ffe), 64, &[gate]).unwrap();
    assert_eq!(unit.instruction_count, 2);
    assert!(unit.is_current(&memory));
    memory.write(GuestAddress(0x2000), &[0x0b]).unwrap();
    assert!(!unit.is_current(&memory));

    let mut memory = mapped(&[0x1000]);
    memory.write(GuestAddress(0x1ffe), &[0x90, 0x0f]).unwrap();
    assert!(matches!(
        compile_with_gates(&memory, GuestAddress(0x1ffe), 64, &[gate]),
        Err(CompileError::Memory(MemoryError::Fault(MemoryFault {
            address: GuestAddress(0x2000),
            access: Access::Execute,
            reason: FaultReason::Unmapped,
        })))
    ));
    for gates in [vec![gate, gate], vec![(gate.0, 0)]] {
        assert!(matches!(
            compile_with_gates(&memory, GuestAddress(0x1ffe), 64, &gates),
            Err(CompileError::InvalidLimits)
        ));
    }
    memory.map_zeroed(page(0x2000), Permissions::READ_WRITE).unwrap();
    memory.write(GuestAddress(0x2000), &[0x0b]).unwrap();
    assert!(matches!(
        compile_with_gates(&memory, GuestAddress(0x1ffe), 64, &[gate]),
        Err(CompileError::Memory(MemoryError::Fault(MemoryFault {
            address: GuestAddress(0x2000),
            access: Access::Execute,
            reason: FaultReason::Permission,
        })))
    ));
}

#[test]
fn separate_control_flow_blocks_do_not_capture_the_gap_between_them() {
    let mut memory = mapped(&[0x1000, 0x2000, 0x3000]);
    memory
        .write(GuestAddress(0x1000), &[0xe9, 0xfb, 0x1f, 0, 0])
        .unwrap();
    memory.write(GuestAddress(0x3000), &[0xc3]).unwrap();
    let unit = compile(&memory, GuestAddress(0x1000), 64).unwrap();
    assert_eq!(unit.entries, [0x1000, 0x3000]);
    memory.write(GuestAddress(0x2000), &[0xcc]).unwrap();
    assert!(unit.is_current(&memory));
    memory.write(GuestAddress(0x3000), &[0xc3]).unwrap();
    assert!(!unit.is_current(&memory));
}

#[test]
fn block_boundary_wrap_tracks_both_pages_without_allowing_instruction_fetch_wrap() {
    for changed in [u32::MAX, 0] {
        let mut memory = mapped(&[0xfffff000, 0, 0x2000]);
        memory.write(GuestAddress(u32::MAX), &[0x90]).unwrap();
        memory.write(GuestAddress(0), &[0xc3]).unwrap();
        let unit = compile(&memory, GuestAddress(u32::MAX), 64).unwrap();
        assert!(unit.is_current(&memory));
        memory.write(GuestAddress(0x2000), &[0xcc]).unwrap();
        assert!(unit.is_current(&memory));
        memory
            .write(GuestAddress(changed), &[if changed == 0 { 0xc3 } else { 0x90 }])
            .unwrap();
        assert!(!unit.is_current(&memory));
    }

    let mut memory = mapped(&[0xfffff000, 0]);
    memory.write(GuestAddress(u32::MAX), &[0x0f]).unwrap();
    memory.write(GuestAddress(0), &[0x0b]).unwrap();
    assert!(matches!(
        compile(&memory, GuestAddress(u32::MAX), 64),
        Err(CompileError::Decode(DecodeError::MemoryFault {
            fault: MemoryFault {
                address: GuestAddress(u32::MAX),
                access: Access::Execute,
                reason: FaultReason::AddressOverflow,
            },
            ..
        }))
    ));
}
