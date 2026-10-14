use super::*;
use crate::{
    cpu::x86::{
        ControlOperation, DescriptorTableRegister32, SegmentCache, SegmentContext,
        UserSystemRegisterBank32,
        decode::{
            BoundDecode, CodeFetchContext, DecodeAuthority, DecodePolicy, decode_for_execution,
            decode_one, decode_with_authority,
        },
    },
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
};
fn memory(pc: u32, bytes: &[u8]) -> AddressSpace {
    let mut memory = AddressSpace::new(2).unwrap();
    let page = PageRange::new(GuestAddress(pc & !0xfff), 1).unwrap();
    memory.map_zeroed(page, Permissions::ALL).unwrap();
    memory.write(GuestAddress(pc), bytes).unwrap();
    memory.protect(page, Permissions::EXECUTE).unwrap();
    memory
}
fn context(d32: bool) -> CodeFetchContext {
    let mut context = SegmentContext::default();
    context.caches[2] = SegmentCache::from_bits(0xf2300000).unwrap();
    context.state.selectors[3] = 0;
    context.caches[3] = SegmentCache::UNUSABLE;
    if !d32 {
        context.caches[1] = SegmentCache::from_bits(0xf1bfffff).unwrap();
    }
    CodeFetchContext::from_segments(&context).unwrap()
}
#[test]
fn software_vectors_keep_bare_and_66_length_payload_modes_and_no_data_access() {
    for d32 in [false, true] {
        for (prefix, length) in [(&[][..], 2_u8), (&[0x66][..], 3)] {
            for vector in [0, 1, 3, 4, 0x26, 0x66, 0x67, 0xf0, 0xf2, 0xf3, 0xff] {
                let bytes = [prefix, &[0xcd, vector]].concat();
                let memory = memory(0x1000, &bytes);
                for policy in [DecodePolicy::Strict, DecodePolicy::Execution] {
                    let BoundDecode::Instruction(decoded) = decode_with_authority(
                        &memory,
                        GuestAddress(0x1000),
                        policy,
                        Some(context(d32)),
                        DecodeAuthority::new(None),
                    )
                    .unwrap() else {
                        panic!("complete INT");
                    };
                    let expected = if vector == 3 {
                        Operation::Control(ControlOperation::Breakpoint)
                    } else {
                        Operation::SoftwareInterrupt { vector }
                    };
                    assert_eq!(decoded.operation(), &expected);
                    assert_eq!(
                        (decoded.pc(), decoded.next_pc(), decoded.length()),
                        (
                            GuestAddress(0x1000),
                            GuestAddress(0x1000 + u32::from(length)),
                            length
                        )
                    );
                    assert_eq!(
                        decoded.segment_accesses(),
                        0,
                        "software INT does not read an application return frame"
                    );
                    assert!(memory.is_code_current(decoded.code_snapshot()));
                }
            }
        }
    }
}
#[test]
fn software_vectors_decode_without_hardware_idt_or_query_bank_authority() {
    let bank = UserSystemRegisterBank32 {
        idtr: Some(DescriptorTableRegister32 {
            base: 0,
            limit: u16::MAX,
        }),
        ..Default::default()
    };
    for bytes in [&[0xcd, 0x80][..], &[0x66, 0xcd, 0x80]] {
        let memory = memory(0x1000, bytes);
        for d32 in [false, true] {
            for authority in [DecodeAuthority::default(), DecodeAuthority::new(Some(bank))] {
                for policy in [DecodePolicy::Strict, DecodePolicy::Execution] {
                    let BoundDecode::Instruction(decoded) = decode_with_authority(
                        &memory,
                        GuestAddress(0x1000),
                        policy,
                        Some(context(d32)),
                        authority,
                    )
                    .unwrap() else {
                        panic!("complete software INT");
                    };
                    assert_eq!(
                        decoded.operation(),
                        &Operation::SoftwareInterrupt { vector: 0x80 }
                    );
                }
            }
        }
        assert_eq!(
            decode_one(&memory, GuestAddress(0x1000))
                .unwrap()
                .operation(),
            &Operation::SoftwareInterrupt { vector: 0x80 }
        );
        assert_eq!(
            decode_for_execution(&memory, GuestAddress(0x1000))
                .unwrap()
                .operation(),
            &Operation::SoftwareInterrupt { vector: 0x80 }
        );
    }
}
#[test]
fn complete_immediate_fetch_and_cs_limit_precede_authority_and_invalid_lock_keeps_its_owner() {
    for (partial, pc, length) in [(&[0xcd][..], 0x1fff, 2_u8), (&[0x66, 0xcd][..], 0x1ffe, 3)] {
        let mut memory = memory(pc, partial);
        let error = decode_with_authority(
            &memory,
            GuestAddress(pc),
            DecodePolicy::Strict,
            Some(context(true)),
            DecodeAuthority::new(None),
        )
        .unwrap_err();
        assert!(matches!(error, DecodeError::Paging {
            attempted_length: observed,
            error: crate::cpu::x86::CpuPagingError32::PageFault(fault), ..
        } if observed == length && fault.linear() == GuestAddress(0x2000) && fault.error_code() == 20));
        let BoundDecode::CodeFault(fault) = decode_with_authority(
            &memory,
            GuestAddress(pc),
            DecodePolicy::Execution,
            Some(context(true)),
            DecodeAuthority::new(None),
        )
        .unwrap() else {
            panic!("execution retains the incomplete INT fetch")
        };
        assert_eq!(
            (fault.attempted_length, fault.consumed_prefix_length),
            (length, length - 1)
        );
        assert!(memory.is_code_current(fault.prefix.as_ref().unwrap()));
        let page = PageRange::new(GuestAddress(0x2000), 1).unwrap();
        memory.map_zeroed(page, Permissions::ALL).unwrap();
        memory.write(GuestAddress(0x2000), &[0x80]).unwrap();
        memory.protect(page, Permissions::READ).unwrap();
        assert!(matches!(
            decode_with_authority(
                &memory,
                GuestAddress(pc),
                DecodePolicy::Strict,
                Some(context(true)),
                DecodeAuthority::new(None)
            ),
            Err(DecodeError::Paging { attempted_length: observed, error: crate::cpu::x86::CpuPagingError32::PageFault(_), .. }) if u32::from(observed) == u32::from(length)
        ));
        memory.protect(page, Permissions::EXECUTE).unwrap();
        let BoundDecode::Instruction(decoded) = decode_with_authority(
            &memory,
            GuestAddress(pc),
            DecodePolicy::Strict,
            Some(context(true)),
            DecodeAuthority::new(None),
        )
        .unwrap() else {
            panic!("repaired INT");
        };
        assert_eq!(
            decoded.operation(),
            &Operation::SoftwareInterrupt { vector: 0x80 }
        );
        assert_eq!(
            (decoded.length(), decoded.next_pc()),
            (length, GuestAddress(0x2001))
        );
        assert_eq!(decoded.code_snapshot().versions.len(), 2);
        memory.protect(page, Permissions::ALL).unwrap();
        let BoundDecode::Instruction(fresh) = decode_with_authority(
            &memory,
            GuestAddress(pc),
            DecodePolicy::Strict,
            Some(context(true)),
            DecodeAuthority::new(None),
        )
        .unwrap() else {
            panic!("fresh INT");
        };
        assert!(memory.is_code_current(fresh.code_snapshot()));
        memory.write(GuestAddress(0x2000), &[0x80]).unwrap();
        assert!(!memory.is_code_current(fresh.code_snapshot()));
        // Retain the prefix/opcode page as well as the immediate page in the snapshot.
        let prefix_page = PageRange::new(GuestAddress(0x1000), 1).unwrap();
        memory.protect(prefix_page, Permissions::ALL).unwrap();
        let BoundDecode::Instruction(fresh) = decode_with_authority(
            &memory,
            GuestAddress(pc),
            DecodePolicy::Strict,
            Some(context(true)),
            DecodeAuthority::new(None),
        )
        .unwrap() else {
            panic!("fresh original prefix");
        };
        memory.write(GuestAddress(pc), &partial[..1]).unwrap();
        assert!(!memory.is_code_current(fresh.code_snapshot()));
        let mut segments = SegmentContext::default();
        segments.caches[1] = SegmentCache::from_bits(0xf2b01fff).unwrap();
        let cs = CodeFetchContext::from_segments(&segments).unwrap();
        assert!(matches!(
            decode_with_authority(
                &memory,
                GuestAddress(pc),
                DecodePolicy::Strict,
                Some(cs),
                DecodeAuthority::new(None)
            ),
            Err(DecodeError::CodeSegment { attempted_length, .. }) if attempted_length == length
        ));
        let BoundDecode::CodeFault(fault) = decode_with_authority(
            &memory,
            GuestAddress(pc),
            DecodePolicy::Execution,
            Some(cs),
            DecodeAuthority::new(None),
        )
        .unwrap() else {
            panic!("CS span precedes INT");
        };
        assert_eq!(
            (fault.attempted_length, fault.consumed_prefix_length),
            (length, length - 1)
        );
    }
    let locked = memory_for_lock();
    assert_eq!(
        decode_with_authority(
            &locked,
            GuestAddress(0x1000),
            DecodePolicy::Strict,
            None,
            DecodeAuthority::new(None)
        )
        .unwrap_err(),
        DecodeError::InvalidEncoding
    );
    let BoundDecode::Instruction(decoded) = decode_with_authority(
        &locked,
        GuestAddress(0x1000),
        DecodePolicy::Execution,
        None,
        DecodeAuthority::new(None),
    )
    .unwrap() else {
        panic!("invalid LOCK");
    };
    assert_eq!(
        decoded.operation(),
        &Operation::Control(ControlOperation::InvalidOpcode)
    );
}
fn memory_for_lock() -> AddressSpace {
    memory(0x1000, &[0xf0, 0xcd, 0x80])
}
#[test]
fn unselected_prefixes_remain_neutral_and_existing_event_opcodes_stay_distinct() {
    for prefix in [&[0x67][..], &[0x2e], &[0xf2], &[0xf3], &[0x66, 0x66]] {
        let bytes = [prefix, &[0xcd, 0x80]].concat();
        let memory = memory(0x1000, &bytes);
        for policy in [DecodePolicy::Strict, DecodePolicy::Execution] {
            assert_eq!(
                decode_with_authority(
                    &memory,
                    GuestAddress(0x1000),
                    policy,
                    None,
                    DecodeAuthority::new(None)
                )
                .unwrap_err(),
                unsupported()
            );
        }
    }
    for (bytes, operation) in [
        (&[0xcc][..], ControlOperation::Breakpoint),
        (&[0xf1][..], ControlOperation::DebugTrap),
        (&[0xce][..], ControlOperation::Overflow),
    ] {
        let memory = memory(0x1000, bytes);
        let BoundDecode::Instruction(decoded) = decode_with_authority(
            &memory,
            GuestAddress(0x1000),
            DecodePolicy::Strict,
            None,
            DecodeAuthority::new(None),
        )
        .unwrap() else {
            panic!("existing event");
        };
        assert_eq!(decoded.operation(), &Operation::Control(operation));
    }
}
