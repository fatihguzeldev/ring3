use ring3_engine::cpu::UnsupportedFeature;
use ring3_engine::cpu::x86::Register32;
use ring3_engine::cpu::x86::decode::{DecodeError, decode_one};
use ring3_engine::cpu::x86::ir::{
    EffectiveAddress, ExtensionKind, Location32, Operation, SmallSource, SmallWidth, Value32,
};
use ring3_engine::memory::{
    Access, AddressSpace, FaultReason, GuestAddress, MemoryError, PageRange, Permissions,
};

fn range(address: u32, pages: u32) -> PageRange {
    PageRange::new(GuestAddress(address), pages).unwrap()
}

fn code_space(pc: u32, bytes: &[u8], permissions: Permissions) -> AddressSpace {
    let first = pc & !0xfff;
    let pages = ((pc as u64 - first as u64 + bytes.len() as u64).div_ceil(4096)) as u32;
    let mut space = AddressSpace::new(pages + 1).unwrap();
    space
        .map_zeroed(range(first, pages), Permissions::ALL)
        .unwrap();
    space.write(GuestAddress(pc), bytes).unwrap();
    space.protect(range(first, pages), permissions).unwrap();
    space
}

fn address(
    base: Option<Register32>,
    index: Option<Register32>,
    scale: u8,
    displacement: u32,
) -> EffectiveAddress {
    EffectiveAddress {
        base,
        index,
        scale,
        displacement,
    }
}

fn assert_decodes(bytes: &[u8], expected: Operation) {
    let space = code_space(0x1000, bytes, Permissions::EXECUTE);
    let decoded = decode_one(&space, GuestAddress(0x1000)).unwrap();
    assert_eq!(decoded.pc().0, 0x1000);
    assert_eq!(decoded.length() as usize, bytes.len());
    assert_eq!(decoded.next_pc().0, 0x1000 + bytes.len() as u32);
    assert_eq!(decoded.operation(), &expected);
    assert!(space.is_code_current(decoded.code_snapshot()));
}

fn assert_decode_fault(
    space: &AddressSpace,
    pc: u32,
    address: u32,
    span: u32,
    reason: FaultReason,
) {
    let error = decode_one(space, GuestAddress(pc)).err().unwrap();
    let DecodeError::MemoryFault { fault, length } = error else {
        panic!("expected instruction fetch fault, got {error:?}");
    };
    assert_eq!(fault.address.0, address);
    assert_eq!(fault.access, Access::Execute);
    assert_eq!(fault.reason, reason);
    assert_eq!(length, span);
}

#[test]
fn authored_mov_fixture_preserves_directions_immediate_and_sib_displacements() {
    use Register32::{Eax, Ebp, Ebx, Ecx, Edi, Edx, Esp};
    let cases: [(&[u8], Operation); 6] = [
        (
            &[0xb8, 0x78, 0x56, 0x34, 0x12],
            Operation::Move {
                destination: Location32::Register(Eax),
                source: Value32::Immediate(0x1234_5678),
            },
        ),
        (
            &[0x89, 0xc7],
            Operation::Move {
                destination: Location32::Register(Edi),
                source: Value32::Register(Eax),
            },
        ),
        (
            &[0x8b, 0x43, 0x10],
            Operation::Move {
                destination: Location32::Register(Eax),
                source: Value32::Memory(address(Some(Ebx), None, 1, 16)),
            },
        ),
        (
            &[0x89, 0x03],
            Operation::Move {
                destination: Location32::Memory(address(Some(Ebx), None, 1, 0)),
                source: Value32::Register(Eax),
            },
        ),
        (
            &[0x8b, 0x54, 0x8d, 0xe0],
            Operation::Move {
                destination: Location32::Register(Edx),
                source: Value32::Memory(address(Some(Ebp), Some(Ecx), 4, 0xffff_ffe0)),
            },
        ),
        (
            &[0x89, 0x94, 0x44, 0x78, 0x56, 0x34, 0x12],
            Operation::Move {
                destination: Location32::Memory(address(Some(Esp), Some(Eax), 2, 0x1234_5678)),
                source: Value32::Register(Edx),
            },
        ),
    ];
    for (bytes, expected) in cases {
        assert_decodes(bytes, expected);
    }
}

#[test]
fn every_immediate_register_encoding_uses_the_x86_register_order() {
    use Register32::{Eax, Ebp, Ebx, Ecx, Edi, Edx, Esi, Esp};
    for (opcode, register) in [
        (0xb8, Eax),
        (0xb9, Ecx),
        (0xba, Edx),
        (0xbb, Ebx),
        (0xbc, Esp),
        (0xbd, Ebp),
        (0xbe, Esi),
        (0xbf, Edi),
    ] {
        assert_decodes(
            &[opcode, 0xff, 0xff, 0xff, 0xff],
            Operation::Move {
                destination: Location32::Register(register),
                source: Value32::Immediate(u32::MAX),
            },
        );
    }
}

#[test]
fn alternate_register_move_direction_and_memory_immediate_are_explicit() {
    assert_decodes(
        &[0x8b, 0xc7],
        Operation::Move {
            destination: Location32::Register(Register32::Eax),
            source: Value32::Register(Register32::Edi),
        },
    );
    assert_decodes(
        &[0xc7, 0x03, 0x78, 0x56, 0x34, 0x12],
        Operation::Move {
            destination: Location32::Memory(address(Some(Register32::Ebx), None, 1, 0)),
            source: Value32::Immediate(0x1234_5678),
        },
    );
}

#[test]
fn absolute_modrm_moffs_and_sib_without_base_preserve_addresses() {
    use Register32::{Eax, Ecx};
    for bytes in [
        &[0x8b, 0x05, 0x78, 0x56, 0x34, 0x12][..],
        &[0xa1, 0x78, 0x56, 0x34, 0x12][..],
    ] {
        assert_decodes(
            bytes,
            Operation::Move {
                destination: Location32::Register(Eax),
                source: Value32::Memory(address(None, None, 1, 0x1234_5678)),
            },
        );
    }
    assert_decodes(
        &[0xa3, 0x78, 0x56, 0x34, 0x12],
        Operation::Move {
            destination: Location32::Memory(address(None, None, 1, 0x1234_5678)),
            source: Value32::Register(Eax),
        },
    );
    assert_decodes(
        &[0x8b, 0x04, 0x8d, 0x78, 0x56, 0x34, 0x12],
        Operation::Move {
            destination: Location32::Register(Eax),
            source: Value32::Memory(address(None, Some(Ecx), 4, 0x1234_5678)),
        },
    );
}

#[test]
fn sib_without_index_and_signed_displacements_are_not_conflated() {
    use Register32::{Eax, Ebp, Ebx, Esp};
    for (bytes, expected_address) in [
        (
            &[0x8b, 0x44, 0x24, 0x7f][..],
            address(Some(Esp), None, 1, 127),
        ),
        (&[0x8b, 0x45, 0][..], address(Some(Ebp), None, 1, 0)),
        (
            &[0x8b, 0x43, 0x80][..],
            address(Some(Ebx), None, 1, 0xffff_ff80),
        ),
        (
            &[0x8b, 0x83, 0x78, 0x56, 0x34, 0xf2][..],
            address(Some(Ebx), None, 1, 0xf234_5678),
        ),
    ] {
        assert_decodes(
            bytes,
            Operation::Move {
                destination: Location32::Register(Eax),
                source: Value32::Memory(expected_address),
            },
        );
    }
}

#[test]
fn authored_extend_fixture_preserves_source_width_sign_and_address() {
    use Register32::{Eax, Ebp, Ebx, Ecx, Edi, Edx, Esi};
    let cases = [
        (
            &[0x0f, 0xb6, 0x46, 7][..],
            ExtensionKind::Zero,
            Eax,
            SmallWidth::Byte,
            address(Some(Esi), None, 1, 7),
        ),
        (
            &[0x0f, 0xb7, 0x4f, 0xfe][..],
            ExtensionKind::Zero,
            Ecx,
            SmallWidth::Word,
            address(Some(Edi), None, 1, 0xffff_fffe),
        ),
        (
            &[0x0f, 0xbe, 0x54, 0xcb, 3][..],
            ExtensionKind::Sign,
            Edx,
            SmallWidth::Byte,
            address(Some(Ebx), Some(Ecx), 8, 3),
        ),
        (
            &[0x0f, 0xbf, 0x74, 0x55, 0x80][..],
            ExtensionKind::Sign,
            Esi,
            SmallWidth::Word,
            address(Some(Ebp), Some(Edx), 2, 0xffff_ff80),
        ),
    ];
    for (bytes, kind, destination, width, address) in cases {
        assert_decodes(
            bytes,
            Operation::Extend {
                kind,
                destination,
                source: SmallSource::Memory { address, width },
            },
        );
    }
}

#[test]
fn byte_extension_registers_preserve_low_and_high_byte_identity() {
    use Register32::{Eax, Ebx, Ecx, Edx};
    for (opcode, kind) in [(0xb6, ExtensionKind::Zero), (0xbe, ExtensionKind::Sign)] {
        for (modrm, register, high_byte) in [
            (0xc0, Eax, false),
            (0xc1, Ecx, false),
            (0xc2, Edx, false),
            (0xc3, Ebx, false),
            (0xc4, Eax, true),
            (0xc5, Ecx, true),
            (0xc6, Edx, true),
            (0xc7, Ebx, true),
        ] {
            assert_decodes(
                &[0x0f, opcode, modrm],
                Operation::Extend {
                    kind,
                    destination: Eax,
                    source: SmallSource::Register {
                        register,
                        width: SmallWidth::Byte,
                        high_byte,
                    },
                },
            );
        }
    }
}

#[test]
fn word_extensions_use_full_register_identity_with_no_high_byte() {
    use Register32::{Eax, Ebp, Ebx, Ecx, Edi, Edx, Esi, Esp};
    for (opcode, kind) in [(0xb7, ExtensionKind::Zero), (0xbf, ExtensionKind::Sign)] {
        for (modrm, register) in [
            (0xc0, Eax),
            (0xc1, Ecx),
            (0xc2, Edx),
            (0xc3, Ebx),
            (0xc4, Esp),
            (0xc5, Ebp),
            (0xc6, Esi),
            (0xc7, Edi),
        ] {
            assert_decodes(
                &[0x0f, opcode, modrm],
                Operation::Extend {
                    kind,
                    destination: Eax,
                    source: SmallSource::Register {
                        register,
                        width: SmallWidth::Word,
                        high_byte: false,
                    },
                },
            );
        }
    }
}

#[test]
fn authored_lea_and_absolute_lea_only_describe_address_calculation() {
    assert_decodes(
        &[0x8d, 0x7c, 0x8b, 0xe0],
        Operation::Lea {
            destination: Register32::Edi,
            address: address(Some(Register32::Ebx), Some(Register32::Ecx), 4, 0xffff_ffe0),
        },
    );
    let space = code_space(
        0x1000,
        &[0x8d, 0x05, 0x78, 0x56, 0x34, 0x12],
        Permissions::EXECUTE,
    );
    assert!(matches!(
        space.resolve(GuestAddress(0x1234_5678), Access::Read),
        Err(MemoryError::Fault(_))
    ));
    let decoded = decode_one(&space, GuestAddress(0x1000)).unwrap();
    assert_eq!(
        decoded.operation(),
        &Operation::Lea {
            destination: Register32::Eax,
            address: address(None, None, 1, 0x1234_5678),
        }
    );
}

#[test]
fn nop_decodes_and_execute_only_code_does_not_need_read_permission() {
    assert_decodes(&[0x90], Operation::Nop);
    let space = code_space(0x1000, &[0x90], Permissions::EXECUTE);
    assert!(matches!(
        space.resolve(GuestAddress(0x1000), Access::Read),
        Err(MemoryError::Fault(_))
    ));
    assert!(decode_one(&space, GuestAddress(0x1000)).is_ok());
}

#[test]
fn readable_nonexecutable_code_faults_on_first_byte() {
    let space = code_space(0x1000, &[0x90], Permissions::READ);
    assert_decode_fault(&space, 0x1000, 0x1000, 1, FaultReason::Permission);
}

#[test]
fn unmapped_instruction_start_reports_one_byte_fetch_span() {
    let space = AddressSpace::new(1).unwrap();
    assert_decode_fault(&space, 0x1731, 0x1731, 1, FaultReason::Unmapped);
}

#[test]
fn cross_page_truncation_reports_the_first_missing_byte_and_attempted_span() {
    let space = code_space(0x1ffe, &[0xb8, 0x78], Permissions::EXECUTE);
    assert_decode_fault(&space, 0x1ffe, 0x2000, 3, FaultReason::Unmapped);
    let mut denied = code_space(0x1ffc, &[0xb8, 0x78, 0x56, 0x34], Permissions::EXECUTE);
    denied
        .map_zeroed(range(0x2000, 1), Permissions::READ)
        .unwrap();
    assert_decode_fault(&denied, 0x1ffc, 0x2000, 5, FaultReason::Permission);
}

#[test]
fn short_page_end_instruction_stops_before_unmapped_next_page() {
    for (pc, bytes, expected) in [
        (0x1fff, &[0x90][..], Operation::Nop),
        (
            0x1ffe,
            &[0x89, 0xc7][..],
            Operation::Move {
                destination: Location32::Register(Register32::Edi),
                source: Value32::Register(Register32::Eax),
            },
        ),
    ] {
        let mut space = code_space(pc, bytes, Permissions::ALL);
        let decoded = decode_one(&space, GuestAddress(pc)).unwrap();
        assert_eq!(decoded.length() as usize, bytes.len());
        assert_eq!(decoded.next_pc().0, 0x2000);
        assert_eq!(decoded.operation(), &expected);
        assert!(space.is_code_current(decoded.code_snapshot()));
        space
            .map_zeroed(range(0x2000, 1), Permissions::ALL)
            .unwrap();
        space.write(GuestAddress(0x2000), &[0x90]).unwrap();
        space.protect(range(0x2000, 1), Permissions::READ).unwrap();
        space.unmap(range(0x2000, 1)).unwrap();
        assert!(space.is_code_current(decoded.code_snapshot()));
    }
}

#[test]
fn actual_cross_page_code_snapshot_tracks_writes_to_both_pages() {
    for changed_page in [0x1000, 0x2000] {
        let mut space = code_space(0x1ffe, &[0xb8, 0x78, 0x56, 0x34, 0x12], Permissions::ALL);
        let decoded = decode_one(&space, GuestAddress(0x1ffe)).unwrap();
        assert_eq!(decoded.length(), 5);
        assert!(space.is_code_current(decoded.code_snapshot()));
        space.write(GuestAddress(changed_page), &[0x90]).unwrap();
        assert!(!space.is_code_current(decoded.code_snapshot()));
    }
}

#[test]
fn actual_cross_page_code_snapshot_tracks_protect_and_remap_on_both_pages() {
    for changed_page in [0x1000, 0x2000] {
        let mut space = code_space(0x1ffe, &[0xb8, 0x78, 0x56, 0x34, 0x12], Permissions::ALL);
        let decoded = decode_one(&space, GuestAddress(0x1ffe)).unwrap();
        space
            .protect(range(changed_page, 1), Permissions::ALL)
            .unwrap();
        assert!(!space.is_code_current(decoded.code_snapshot()));

        let decoded = decode_one(&space, GuestAddress(0x1ffe)).unwrap();
        space.unmap(range(changed_page, 1)).unwrap();
        space
            .map_zeroed(range(changed_page, 1), Permissions::ALL)
            .unwrap();
        space
            .write(GuestAddress(0x1ffe), &[0xb8, 0x78, 0x56, 0x34, 0x12])
            .unwrap();
        assert!(!space.is_code_current(decoded.code_snapshot()));
    }
}

#[test]
fn final_address_nop_wraps_next_pc_but_incomplete_immediate_faults() {
    let nop = code_space(u32::MAX, &[0x90], Permissions::EXECUTE);
    let decoded = decode_one(&nop, GuestAddress(u32::MAX)).unwrap();
    assert_eq!(decoded.length(), 1);
    assert_eq!(decoded.next_pc().0, 0);
    assert_eq!(decoded.operation(), &Operation::Nop);
    assert!(nop.is_code_current(decoded.code_snapshot()));

    let immediate = code_space(u32::MAX, &[0xb8], Permissions::EXECUTE);
    assert_decode_fault(
        &immediate,
        u32::MAX,
        u32::MAX,
        2,
        FaultReason::AddressOverflow,
    );
}

#[test]
fn fifteen_prefixes_are_invalid_without_fetching_a_sixteenth_byte() {
    let mut space = code_space(0x1ff1, &[0x66; 15], Permissions::EXECUTE);
    assert!(matches!(
        decode_one(&space, GuestAddress(0x1ff1)),
        Err(DecodeError::InvalidEncoding)
    ));
    space
        .map_zeroed(range(0x2000, 1), Permissions::ALL)
        .unwrap();
    space.write(GuestAddress(0x2000), &[0x90]).unwrap();
    assert!(matches!(
        decode_one(&space, GuestAddress(0x1ff1)),
        Err(DecodeError::InvalidEncoding)
    ));
}

#[test]
fn malformed_encodings_are_distinct_from_unsupported_instructions() {
    for bytes in [
        &[0x8d, 0xc0][..],
        &[0xc7, 0xc8, 0, 0, 0, 0][..],
        &[0xf0, 0x90][..],
    ] {
        let space = code_space(0x1000, bytes, Permissions::EXECUTE);
        assert!(matches!(
            decode_one(&space, GuestAddress(0x1000)),
            Err(DecodeError::InvalidEncoding)
        ));
    }
}

#[test]
fn unsupported_floating_point_simd_segment_string_and_privileged_are_explicit() {
    for (bytes, expected) in [
        (&[0xd9, 0xe8][..], UnsupportedFeature::FloatingPoint),
        (&[0x0f, 0x57, 0xc0][..], UnsupportedFeature::Simd),
        (&[0x64, 0x8b, 0][..], UnsupportedFeature::Segment),
        (&[0xf3, 0xa4][..], UnsupportedFeature::RepeatedString),
        (&[0xf4][..], UnsupportedFeature::Privileged),
    ] {
        let space = code_space(0x1000, bytes, Permissions::EXECUTE);
        let error = decode_one(&space, GuestAddress(0x1000)).err().unwrap();
        assert_eq!(error, DecodeError::Unsupported(expected));
    }
}

#[test]
fn valid_lock_and_sixteen_bit_encodings_are_rejected_without_lowering() {
    for bytes in [
        &[0xf0, 0x01, 0x08][..],
        &[0x66, 0xb8, 0x34, 0x12][..],
        &[0x67, 0x8b, 0][..],
    ] {
        let space = code_space(0x1000, bytes, Permissions::EXECUTE);
        assert!(matches!(
            decode_one(&space, GuestAddress(0x1000)),
            Err(DecodeError::Unsupported(_))
        ));
    }
}
