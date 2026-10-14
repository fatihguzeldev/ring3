use crate::{
    cpu::x86::{
        DescriptorTableRegister32, SegmentCache, SegmentContext, SystemRegisterQuery,
        UserSystemRegisterBank32,
        decode::{
            BoundDecode, CodeFetchContext, DecodeError, DecodePolicy, decode_one,
            decode_with_authority,
        },
        ir::{Operation, Value32, WordValue},
    },
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
};

fn bank() -> UserSystemRegisterBank32 {
    UserSystemRegisterBank32 {
        gdtr: Some(DescriptorTableRegister32 {
            base: 0x1234_5678,
            limit: 0x2345,
        }),
        idtr: Some(DescriptorTableRegister32 {
            base: 0xfedc_ba98,
            limit: 0x3456,
        }),
        ldtr: Some(0x43),
        tr: Some(0x53),
        cr0_low16: Some(0x33),
    }
}

fn context(default32: bool) -> CodeFetchContext {
    let mut value = SegmentContext::default();
    if !default32 {
        value.caches[1] = SegmentCache::from_bits(0xf1bf_ffff).unwrap();
    }
    CodeFetchContext::from_segments(&value).unwrap()
}

fn code(bytes: &[u8]) -> AddressSpace {
    let mut memory = AddressSpace::new(1).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory
        .write(GuestAddress(0x2000 - bytes.len() as u32), bytes)
        .unwrap();
    memory
}

#[test]
fn application_defaults_lower_to_literal_user_readback_values() {
    for (bytes, value) in [
        (&[0x0f, 0x00, 0xc0][..], 0),
        (&[0x0f, 0x00, 0xc8], 0x28),
        (&[0x0f, 0x01, 0xe0], 0x33),
    ] {
        let memory = code(bytes);
        let BoundDecode::Instruction(decoded) = decode_with_authority(
            &memory,
            GuestAddress(0x2000 - bytes.len() as u32),
            DecodePolicy::Strict,
            Some(context(true)),
            crate::cpu::x86::decode::DecodeAuthority::new(Some(
                UserSystemRegisterBank32::application(),
            )),
        )
        .unwrap() else {
            panic!("default register query");
        };
        assert!(match decoded.operation() {
            Operation::Move {
                source: Value32::Immediate(actual),
                ..
            } => *actual == value,
            Operation::MoveWord {
                source: WordValue::Immediate(actual),
                ..
            } => u32::from(*actual) == value,
            _ => false,
        });
        assert_eq!(memory.mapped_pages(), 1);
    }
    for (modrm, base) in [(0x00, 0xffff0000), (0x08, 0xffff1000)] {
        let bytes = [0x0f, 0x01, modrm];
        let memory = code(&bytes);
        let BoundDecode::Instruction(decoded) = decode_with_authority(
            &memory,
            GuestAddress(0x1ffd),
            DecodePolicy::Strict,
            Some(context(true)),
            crate::cpu::x86::decode::DecodeAuthority::new(Some(
                UserSystemRegisterBank32::application(),
            )),
        )
        .unwrap() else {
            panic!("default descriptor query");
        };
        assert!(
            matches!(decoded.operation(), Operation::StoreSystemDescriptor {
            base: actual_base, limit: 0x7ff, ..
        } if *actual_base == base)
        );
        assert_eq!(memory.mapped_pages(), 1);
    }
}

#[test]
fn complete_queries_bind_exact_widths_without_reading_source_tables() {
    for default32 in [false, true] {
        for prefix in [false, true] {
            for (opcode, value) in [(0x00, 0x43), (0x08, 0x53), (0x20, 0x33)] {
                let mut bytes = if prefix { vec![0x66] } else { vec![] };
                bytes.extend(if opcode == 0x20 {
                    [0x0f, 0x01, 0xe0]
                } else {
                    [0x0f, 0x00, 0xc0 | opcode]
                });
                let memory = code(&bytes);
                let pc = GuestAddress(0x2000 - bytes.len() as u32);
                let decoded = decode_with_authority(
                    &memory,
                    pc,
                    DecodePolicy::Strict,
                    Some(context(default32)),
                    crate::cpu::x86::decode::DecodeAuthority::new(Some(bank())),
                )
                .unwrap();
                let BoundDecode::Instruction(decoded) = decoded else {
                    panic!("complete query")
                };
                let word = !default32 ^ prefix;
                if word || opcode == 0x20 {
                    assert!(
                        matches!(decoded.operation(), Operation::MoveWord { source: WordValue::Immediate(got), .. } if *got == value)
                    );
                } else {
                    assert!(
                        matches!(decoded.operation(), Operation::Move { source: Value32::Immediate(got), .. } if *got == u32::from(value))
                    );
                }
                assert_eq!(decoded.segment_accesses(), 0);
                assert_eq!(decoded.next_pc(), GuestAddress(0x2000));
                assert!(memory.is_code_current(decoded.code_snapshot()));
            }
            for (modrm, base, limit) in [(0x00, 0x1234_5678, 0x2345), (0x08, 0xfedc_ba98, 0x3456)] {
                let mut bytes = if prefix { vec![0x66] } else { vec![] };
                bytes.extend([0x0f, 0x01, modrm]);
                let memory = code(&bytes);
                let pc = GuestAddress(0x2000 - bytes.len() as u32);
                let BoundDecode::Instruction(decoded) = decode_with_authority(
                    &memory,
                    pc,
                    DecodePolicy::Strict,
                    Some(context(default32)),
                    crate::cpu::x86::decode::DecodeAuthority::new(Some(bank())),
                )
                .unwrap() else {
                    panic!("complete query")
                };
                assert!(
                    matches!(decoded.operation(), Operation::StoreSystemDescriptor {base: got_base, limit: got_limit, ..} if *got_base==base && *got_limit==limit)
                );
                assert_ne!(decoded.segment_accesses(), 0);
            }
        }
    }
}

#[test]
fn missing_authority_follows_complete_fetch_and_preserves_prefix_policy() {
    for (bytes, query) in [
        (&[0x0f, 0x01, 0x00][..], SystemRegisterQuery::Gdtr),
        (&[0x0f, 0x01, 0x08], SystemRegisterQuery::Idtr),
        (&[0x0f, 0x00, 0xc0], SystemRegisterQuery::Ldtr),
        (&[0x0f, 0x00, 0xc8], SystemRegisterQuery::Tr),
        (&[0x0f, 0x01, 0xe0], SystemRegisterQuery::MachineStatusWord),
    ] {
        let memory = code(bytes);
        let pc = GuestAddress(0x2000 - bytes.len() as u32);
        assert_eq!(
            decode_one(&memory, pc).unwrap_err(),
            DecodeError::MissingSystemRegisterAuthority(query)
        );
    }
    let memory = code(&[0x0f, 0x01]);
    assert!(matches!(
        decode_one(&memory, GuestAddress(0x1ffe)),
        Err(DecodeError::Paging {
            error: crate::cpu::x86::CpuPagingError32::PageFault(fault),
            attempted_length: 3,
            ..
        }) if fault.linear() == GuestAddress(0x2000) && fault.error_code() == 20
    ));
    let memory = code(&[0x66, 0x66, 0x0f, 0x00, 0xc0]);
    assert!(matches!(
        decode_one(&memory, GuestAddress(0x1ffb)),
        Err(DecodeError::Unsupported(_))
    ));
    let memory = code(&[0x0f, 0x00, 0xc8]);
    let partial = UserSystemRegisterBank32 {
        ldtr: Some(0),
        ..Default::default()
    };
    assert!(matches!(
        decode_with_authority(
            &memory,
            GuestAddress(0x1ffd),
            DecodePolicy::Strict,
            Some(context(true)),
            crate::cpu::x86::decode::DecodeAuthority::new(Some(partial))
        ),
        Err(DecodeError::MissingSystemRegisterAuthority(
            SystemRegisterQuery::Tr
        ))
    ));
}

#[test]
fn current_cs_authority_precedes_missing_bank_for_real_and_source_free_prefixes() {
    let memory = code(&[0x66, 0x0f, 0x01, 0x00]);
    let mut segments = SegmentContext::default();
    segments.caches[1] = SegmentCache::from_bits(0xf2b0_1ffe).unwrap();
    let cs = CodeFetchContext::from_segments(&segments).unwrap();
    assert!(matches!(
        decode_with_authority(
            &memory,
            GuestAddress(0x1ffc),
            DecodePolicy::Strict,
            Some(cs),
            crate::cpu::x86::decode::DecodeAuthority::default()
        ),
        Err(DecodeError::CodeSegment {
            pc: GuestAddress(0x1ffc),
            attempted_length: 4
        })
    ));
    let BoundDecode::CodeFault(fault) = decode_with_authority(
        &memory,
        GuestAddress(0x1ffc),
        DecodePolicy::Execution,
        Some(cs),
        crate::cpu::x86::decode::DecodeAuthority::default(),
    )
    .unwrap() else {
        panic!("real prefix")
    };
    assert_eq!(
        (fault.attempted_length, fault.consumed_prefix_length),
        (4, 3)
    );
    assert!(memory.is_code_current(fault.prefix.as_ref().unwrap()));
    let unmapped = AddressSpace::new(1).unwrap();
    segments.caches[1] = SegmentCache::from_bits(0xf2b0_0fff).unwrap();
    let cs = CodeFetchContext::from_segments(&segments).unwrap();
    let BoundDecode::CodeFault(fault) = decode_with_authority(
        &unmapped,
        GuestAddress(0x1000),
        DecodePolicy::Execution,
        Some(cs),
        crate::cpu::x86::decode::DecodeAuthority::default(),
    )
    .unwrap() else {
        panic!("source free")
    };
    assert_eq!(
        (fault.attempted_length, fault.consumed_prefix_length),
        (1, 0)
    );
    assert!(fault.prefix.is_none());
}
