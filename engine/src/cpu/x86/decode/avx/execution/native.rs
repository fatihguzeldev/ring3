use iced_x86::{Code, Decoder, DecoderError, DecoderOptions, EncodingKind};

use crate::{
    cpu::{
        UnsupportedFeature,
        x86::{
            AvxOperation, ControlOperation, Register32, SegmentRegister,
            bits::BitManipulationKind,
            capabilities::AVX_ENABLED,
            decode::{DecodeError, decode_for_execution, decode_one},
            ir::{EffectiveAddress, Operation, Value32},
        },
    },
    memory::{
        Access, AddressSpace, FaultReason, GuestAddress, MemoryFault, PageRange, Permissions,
    },
};

fn code(bytes: &[u8], pc: u32) -> AddressSpace {
    let mut memory = AddressSpace::new(1).unwrap();
    let range = PageRange::new(GuestAddress(pc & !0xfff), 1).unwrap();
    memory.map_zeroed(range, Permissions::ALL).unwrap();
    memory.write(GuestAddress(pc), bytes).unwrap();
    memory.protect(range, Permissions::EXECUTE).unwrap();
    memory
}

fn assert_invalid(bytes: &[u8], pc: u32) {
    let memory = code(bytes, pc);
    assert_eq!(
        decode_one(&memory, GuestAddress(pc)).unwrap_err(),
        DecodeError::InvalidEncoding,
        "{bytes:02x?}"
    );
    let decoded = decode_for_execution(&memory, GuestAddress(pc)).unwrap();
    assert_eq!(
        decoded.operation(),
        &Operation::Control(ControlOperation::InvalidOpcode),
        "{bytes:02x?}"
    );
    assert_eq!(usize::from(decoded.length()), bytes.len());
    assert_eq!(
        decoded.next_pc(),
        GuestAddress(pc.wrapping_add(bytes.len() as u32))
    );
    assert_eq!(decoded.segment_accesses(), 0);
    assert!(memory.is_code_current(decoded.code_snapshot()));
}

#[test]
fn reserved_w_rejection_keeps_strict_and_execution_policies_distinct() {
    // the pinned decoder rejects W=1 before reading the variable-blend immediate.
    // byte lowering independently rejects it for the canonical decoded identity.
    for modrm in [0xcb, 0x08] {
        let canonical = [0xc4, 0xe3, 0x69, 0x4a, modrm, 0x40];
        let mut decoder = Decoder::new(32, &canonical, DecoderOptions::NONE);
        let instruction = decoder.decode();
        assert_eq!(decoder.last_error(), DecoderError::None);
        assert_eq!(instruction.encoding(), EncodingKind::VEX);
        assert_eq!(instruction.code(), Code::VEX_Vblendvps_xmm_xmm_xmmm128_xmm);
        let mut invalid = canonical;
        invalid[2] |= 0x80;
        assert_eq!(
            super::lower(&instruction, &invalid),
            Some(Err(DecodeError::InvalidEncoding))
        );
        let invalid = &invalid[..5];
        let mut decoder = Decoder::new(32, invalid, DecoderOptions::NONE);
        let instruction = decoder.decode();
        assert_eq!(decoder.last_error(), DecoderError::InvalidInstruction);
        assert_eq!(instruction.len(), invalid.len());
        assert_invalid(invalid, 0x2000 - invalid.len() as u32);
    }
}

#[test]
fn forbidden_prefixes_and_reserved_vvvv_keep_strict_and_execution_distinct() {
    for prefix in [0x66, 0xf0, 0xf2, 0xf3] {
        assert_invalid(&[prefix, 0xc5, 0xf8, 0x77], 0x1000);
    }
    assert_invalid(&[0xc5, 0xf0, 0x10, 0xc1], 0x1ffc);
    assert_invalid(&[0xc4, 0xe1, 0x70, 0x10, 0xc1], 0x1ffb);
}

#[test]
fn integer_extensions_are_admitted_while_evex_and_unpredictable_l_remain_refused() {
    for bytes in [
        &[0xc4, 0xe1, 0x6d, 0xfe, 0xcb][..], // vpaddd ymm
        &[0xc4, 0xe2, 0x6d, 0x40, 0xcb],     // vpmulld ymm
    ] {
        let mut decoder = Decoder::new(32, bytes, DecoderOptions::NONE);
        let instruction = decoder.decode();
        assert_eq!(decoder.last_error(), DecoderError::None);
        assert_eq!(instruction.encoding(), EncodingKind::VEX);
        let memory = code(bytes, 0x1000);
        for decoded in [
            decode_one(&memory, GuestAddress(0x1000)).unwrap(),
            decode_for_execution(&memory, GuestAddress(0x1000)).unwrap(),
        ] {
            assert!(matches!(decoded.operation(), Operation::Avx(_)));
            assert_eq!(decoded.pc(), GuestAddress(0x1000));
            assert_eq!(decoded.length(), bytes.len() as u8);
            assert_eq!(decoded.next_pc(), GuestAddress(0x1000 + bytes.len() as u32));
            assert!(memory.is_code_current(decoded.code_snapshot()));
        }
    }
    for (bytes, encoding) in [
        (
            &[0x62, 0xf1, 0x7c, 0x48, 0x58, 0xc1][..],
            EncodingKind::EVEX,
        ), // vaddps zmm0,zmm0,zmm1
        (&[0xc4, 0xe1, 0x6e, 0x58, 0xcb][..], EncodingKind::VEX), // vaddss with unpredictable l=1
    ] {
        let mut decoder = Decoder::new(32, bytes, DecoderOptions::NONE);
        let instruction = decoder.decode();
        assert_eq!(decoder.last_error(), DecoderError::None, "{bytes:02x?}");
        assert_eq!(instruction.encoding(), encoding);
        if encoding == EncodingKind::EVEX {
            assert_eq!(
                instruction.code(),
                iced_x86::Code::EVEX_Vaddps_zmm_k1z_zmm_zmmm512b32_er
            );
        }
        let memory = code(bytes, 0x1000);
        let strict = decode_one(&memory, GuestAddress(0x1000)).unwrap_err();
        assert_eq!(strict, DecodeError::Unsupported(UnsupportedFeature::Simd));
        assert_eq!(
            decode_for_execution(&memory, GuestAddress(0x1000)).unwrap_err(),
            strict
        );
    }
}
#[test]
fn canonical_gate_does_not_intercept_scalar_vex_or_legacy_les_lds() {
    // these scalar VEX families retain their existing typed owner and profile.
    for (bytes, expected) in [
        (
            &[0xc4, 0xe2, 0x70, 0xf2, 0xc2][..],
            Operation::BitManipulation {
                kind: BitManipulationKind::AndNot {
                    left: Register32::Ecx,
                },
                destination: Register32::Eax,
                source: Value32::Register(Register32::Edx),
            },
        ),
        (
            &[0xc4, 0xe2, 0x73, 0xf5, 0xc2][..],
            Operation::BitManipulation {
                kind: BitManipulationKind::Deposit {
                    value: Register32::Ecx,
                },
                destination: Register32::Eax,
                source: Value32::Register(Register32::Edx),
            },
        ),
        (
            &[0xc4, 0x00][..],
            Operation::LoadFarPointer {
                segment: SegmentRegister::Es,
                destination: Register32::Eax,
                address: EffectiveAddress {
                    base: Some(Register32::Eax),
                    ..Default::default()
                },
                word: false,
            },
        ),
        (
            &[0xc5, 0x00][..],
            Operation::LoadFarPointer {
                segment: SegmentRegister::Ds,
                destination: Register32::Eax,
                address: EffectiveAddress {
                    base: Some(Register32::Eax),
                    ..Default::default()
                },
                word: false,
            },
        ),
    ] {
        let memory = code(bytes, 0x1000);
        assert_eq!(
            decode_one(&memory, GuestAddress(0x1000))
                .unwrap()
                .operation(),
            &expected
        );
        assert_eq!(
            decode_for_execution(&memory, GuestAddress(0x1000))
                .unwrap()
                .operation(),
            &expected
        );
    }
    let memory = code(&[0xc5, 0xf8, 0x58, 0xc1], 0x1ffc);
    let strict = decode_one(&memory, GuestAddress(0x1ffc));
    if AVX_ENABLED {
        // the facade reexport is deliberately public while construction stays opaque.
        let decoded = strict.unwrap();
        let Operation::Avx(operation) = *decoded.operation() else {
            panic!("AVX operation")
        };
        let _: AvxOperation = operation;
        assert_eq!(
            decode_for_execution(&memory, GuestAddress(0x1ffc))
                .unwrap()
                .operation(),
            decoded.operation()
        );
    } else {
        assert_eq!(
            strict.unwrap_err(),
            DecodeError::Unsupported(UnsupportedFeature::Simd)
        );
        assert_eq!(
            decode_for_execution(&memory, GuestAddress(0x1ffc)).unwrap_err(),
            DecodeError::Unsupported(UnsupportedFeature::Simd)
        );
    }
}

#[test]
fn incomplete_vex_keeps_progressive_execute_fault_before_semantic_invalidity() {
    for bytes in [
        &[0xc5, 0xf8][..],
        &[0xc5, 0xf8, 0x10][..],
        &[0xc4, 0xe3, 0x69, 0x4a, 0xcb][..],
    ] {
        let pc = GuestAddress(0x2000 - bytes.len() as u32);
        let memory = code(bytes, pc.0);
        let expected = DecodeError::MemoryFault {
            pc,
            fault: MemoryFault {
                address: GuestAddress(0x2000),
                access: Access::Execute,
                reason: FaultReason::Unmapped,
            },
            length: bytes.len() as u32 + 1,
        };
        assert_eq!(decode_one(&memory, pc).unwrap_err(), expected);
        assert_eq!(decode_for_execution(&memory, pc).unwrap_err(), expected);
    }
}

#[test]
fn complete_fifteen_byte_vex_is_distinct_from_ambiguous_decoder_ceiling() {
    let mut valid = vec![0x2e; 11];
    valid.extend([0xc5, 0xf8, 0x58, 0xc1]);
    assert_eq!(valid.len(), 15);
    let mut decoder = Decoder::new(32, &valid, DecoderOptions::NONE);
    let instruction = decoder.decode();
    assert_eq!(decoder.last_error(), DecoderError::None);
    assert_eq!(instruction.len(), 15);
    let memory = code(&valid, 0x1ff1);
    if AVX_ENABLED {
        for decoded in [
            decode_one(&memory, GuestAddress(0x1ff1)).unwrap(),
            decode_for_execution(&memory, GuestAddress(0x1ff1)).unwrap(),
        ] {
            assert!(matches!(decoded.operation(), Operation::Avx(_)));
            assert_eq!(decoded.length(), 15);
            assert_eq!(decoded.next_pc(), GuestAddress(0x2000));
            assert!(memory.is_code_current(decoded.code_snapshot()));
        }
    } else {
        assert_eq!(
            decode_one(&memory, GuestAddress(0x1ff1)).unwrap_err(),
            DecodeError::Unsupported(UnsupportedFeature::Simd)
        );
        assert_eq!(
            decode_for_execution(&memory, GuestAddress(0x1ff1)).unwrap_err(),
            DecodeError::Unsupported(UnsupportedFeature::Simd)
        );
    }
    // at the 15-byte ceiling iced merges malformed and overlong instructions.
    let mut invalid = vec![0x2e; 11];
    invalid.extend([0xc5, 0xf0, 0x10, 0xc1]);
    assert_eq!(invalid.len(), 15);
    let memory = code(&invalid, 0x1ff1);
    for result in [
        decode_one(&memory, GuestAddress(0x1ff1)),
        decode_for_execution(&memory, GuestAddress(0x1ff1)),
    ] {
        assert_eq!(result.unwrap_err(), DecodeError::InvalidEncoding);
    }
}
