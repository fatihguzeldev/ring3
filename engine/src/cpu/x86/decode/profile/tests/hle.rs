use iced_x86::{Decoder, DecoderOptions};

use crate::{
    cpu::x86::{
        ControlOperation, SegmentCache, SegmentContext,
        capabilities::CPU_ID_LEAVES,
        decode::{
            BoundDecode, CodeFetchContext, DecodeError, DecodePolicy, decode_bound,
            decode_for_execution, decode_one,
        },
        ir::Operation,
    },
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
};

fn executable(bytes: &[u8], pc: u32) -> AddressSpace {
    let mut memory = AddressSpace::new(2).unwrap();
    let page = PageRange::new(GuestAddress(pc & !0xfff), 1).unwrap();
    memory.map_zeroed(page, Permissions::ALL).unwrap();
    memory.write(GuestAddress(pc), bytes).unwrap();
    memory.protect(page, Permissions::EXECUTE).unwrap();
    memory
}

fn underlying_operation(bytes: &[u8]) -> Operation {
    let memory = executable(bytes, 0x1000);
    *decode_one(&memory, GuestAddress(0x1000))
        .unwrap()
        .operation()
}

#[test]
fn eligible_hints_keep_the_exact_underlying_operation_with_hle_absent() {
    let leaf = CPU_ID_LEAVES
        .iter()
        .find(|leaf| leaf.function == 7 && leaf.subfunction == Some(0))
        .unwrap();
    assert_eq!(leaf.registers[1] & (1 << 4), 0);
    // every eligible locked mnemonic, implicit-lock xchg, and selected width routes.
    let locked: &[&[u8]] = &[
        &[0xf0, 0x01, 0x08],
        &[0xf0, 0x11, 0x08],
        &[0xf0, 0x21, 0x08],
        &[0xf0, 0x0f, 0xbb, 0x08],
        &[0xf0, 0x0f, 0xb3, 0x08],
        &[0xf0, 0x0f, 0xab, 0x08],
        &[0xf0, 0x0f, 0xb1, 0x08],
        &[0xf0, 0x0f, 0xc7, 0x08],
        &[0xf0, 0xff, 0x08],
        &[0xf0, 0xff, 0x00],
        &[0xf0, 0xf7, 0x18],
        &[0xf0, 0xf7, 0x10],
        &[0xf0, 0x83, 0x08, 0x01],
        &[0xf0, 0x19, 0x08],
        &[0xf0, 0x29, 0x08],
        &[0xf0, 0x31, 0x08],
        &[0xf0, 0x0f, 0xc1, 0x08],
        &[0xf0, 0x87, 0x08],
        &[0x87, 0x08],
        &[0x66, 0xf0, 0x83, 0x08, 0x01],
        &[0xf0, 0x80, 0x08, 0x01],
        &[0x66, 0x87, 0x08],
        &[0x86, 0x08],
    ];
    for &bytes in locked {
        let expected = underlying_operation(bytes);
        for hint in [0xf2, 0xf3] {
            let hinted = [&[hint][..], bytes].concat();
            let mut raw = Decoder::with_ip(32, &hinted, 0x1000, DecoderOptions::NONE);
            let instruction = raw.decode();
            assert!(!instruction.is_invalid(), "{hinted:02x?}");
            assert!(instruction.has_xacquire_prefix() || instruction.has_xrelease_prefix());
            let memory = executable(&hinted, 0x1000);
            for decoded in [
                decode_one(&memory, GuestAddress(0x1000)).unwrap(),
                decode_for_execution(&memory, GuestAddress(0x1000)).unwrap(),
            ] {
                assert_eq!(decoded.operation(), &expected, "{hinted:02x?}");
                assert_eq!(decoded.length() as usize, hinted.len());
                assert_eq!(
                    decoded.next_pc(),
                    GuestAddress(0x1000 + hinted.len() as u32)
                );
                assert!(decoded.uses_segments());
                assert!(memory.is_code_current(decoded.code_snapshot()));
            }
        }
    }
    // only 88/89/c6/c7 stores qualify for unlocked xrelease.
    for bytes in [
        &[0x88, 0x08][..],
        &[0x89, 0x08],
        &[0xc6, 0x00, 0x35],
        &[0xc7, 0x00, 0x78, 0x56, 0x34, 0x12],
        &[0x66, 0x89, 0x08],
        &[0x66, 0xc7, 0x00, 0x34, 0x12],
        &[0x67, 0x66, 0x89, 0x08],
    ] {
        let hinted = [&[0xf3][..], bytes].concat();
        let expected = underlying_operation(bytes);
        let memory = executable(&hinted, 0x1000);
        for decoded in [
            decode_one(&memory, GuestAddress(0x1000)).unwrap(),
            decode_for_execution(&memory, GuestAddress(0x1000)).unwrap(),
        ] {
            assert_eq!(decoded.operation(), &expected, "{hinted:02x?}");
            assert_eq!(decoded.length() as usize, hinted.len());
            assert!(decoded.uses_segments());
        }
    }
}

#[test]
fn mixed_hints_select_the_closest_prefix_without_changing_the_ordinary_operation() {
    for plain in [&[0xf0, 0x83, 0x08, 0x01][..], &[0x87, 0x08], &[0x89, 0x08]] {
        for hints in [
            &[0xf2, 0xf3][..],
            &[0xf3, 0xf2],
            &[0xf2, 0xf2, 0xf3],
            &[0xf3, 0xf3, 0xf2],
        ] {
            let bytes = [hints, plain].concat();
            let raw = Decoder::new(32, &bytes, DecoderOptions::NONE).decode();
            let acquire = hints.last() == Some(&0xf2);
            let qualifies = plain[0] != 0x89 || !acquire;
            assert_eq!(raw.has_xacquire_prefix(), qualifies && acquire);
            assert_eq!(raw.has_xrelease_prefix(), qualifies && !acquire);
            let memory = executable(&bytes, 0x1000);
            if qualifies {
                let expected = underlying_operation(plain);
                for decoded in [
                    decode_one(&memory, GuestAddress(0x1000)).unwrap(),
                    decode_for_execution(&memory, GuestAddress(0x1000)).unwrap(),
                ] {
                    assert_eq!(decoded.operation(), &expected);
                    assert_eq!(usize::from(decoded.length()), bytes.len());
                    assert!(decoded.uses_segments());
                    assert!(memory.is_code_current(decoded.code_snapshot()));
                }
            } else {
                let strict = decode_one(&memory, GuestAddress(0x1000)).unwrap_err();
                assert!(matches!(strict, DecodeError::Unsupported(_)));
                assert_eq!(
                    decode_for_execution(&memory, GuestAddress(0x1000)).unwrap_err(),
                    strict
                );
            }
        }
    }
}

#[test]
fn hint_admission_preserves_undefined_repeat_and_duplicate_profile_refusals() {
    for bytes in [
        &[0xf2, 0x89, 0x08][..],
        &[0xf3, 0x89, 0xc8],
        &[0xf2, 0x83, 0x08, 0x01],
        &[0xf3, 0x03, 0x08],
        &[0xf3, 0xa3, 0x00, 0x50, 0x00, 0x00],
        &[0xf2, 0xf2, 0xf0, 0x83, 0x08, 0x01],
        &[0xf3, 0xf3, 0x89, 0x08],
        &[0x66, 0x66, 0xf3, 0x89, 0x08],
    ] {
        let memory = executable(bytes, 0x1000);
        let strict = decode_one(&memory, GuestAddress(0x1000)).unwrap_err();
        assert!(
            matches!(strict, DecodeError::Unsupported(_)),
            "{bytes:02x?}"
        );
        assert_eq!(
            decode_for_execution(&memory, GuestAddress(0x1000)).unwrap_err(),
            strict
        );
    }
    for bytes in [&[0xf3, 0x90][..], &[0xf3, 0xa4], &[0xf2, 0xa6]] {
        let memory = executable(bytes, 0x1000);
        let strict = decode_one(&memory, GuestAddress(0x1000)).unwrap();
        assert_eq!(
            decode_for_execution(&memory, GuestAddress(0x1000))
                .unwrap()
                .operation(),
            strict.operation()
        );
    }
}

#[test]
fn invalid_lock_forms_stay_invalid_and_execution_ud() {
    for bytes in [
        &[0xf3, 0xf0, 0x89, 0x08][..],
        &[0xf2, 0xf0, 0x83, 0xc8, 0x01],
        &[0xf3, 0xf0, 0x90],
    ] {
        let memory = executable(bytes, 0x1000);
        assert_eq!(
            decode_one(&memory, GuestAddress(0x1000)).unwrap_err(),
            DecodeError::InvalidEncoding
        );
        assert_eq!(
            decode_for_execution(&memory, GuestAddress(0x1000))
                .unwrap()
                .operation(),
            &Operation::Control(ControlOperation::InvalidOpcode)
        );
    }
}

#[test]
fn complete_fetch_is_required_before_hint_admission() {
    let bytes = [0xf2, 0xf0, 0x83, 0x08, 0x01];
    let mut memory = executable(&bytes[..4], 0x1ffc);
    let missing = decode_one(&memory, GuestAddress(0x1ffc)).unwrap_err();
    assert!(matches!(
        missing,
        DecodeError::MemoryFault { length: 5, .. }
    ));
    assert_eq!(
        decode_for_execution(&memory, GuestAddress(0x1ffc)).unwrap_err(),
        missing
    );
    let second = PageRange::new(GuestAddress(0x2000), 1).unwrap();
    memory.map_zeroed(second, Permissions::ALL).unwrap();
    memory.write(GuestAddress(0x2000), &bytes[4..]).unwrap();
    memory.protect(second, Permissions::READ).unwrap();
    let denied = decode_one(&memory, GuestAddress(0x1ffc)).unwrap_err();
    assert!(matches!(denied, DecodeError::MemoryFault { length: 5, .. }));
    assert_eq!(
        decode_for_execution(&memory, GuestAddress(0x1ffc)).unwrap_err(),
        denied
    );
    memory.protect(second, Permissions::EXECUTE).unwrap();
    let decoded = decode_for_execution(&memory, GuestAddress(0x1ffc)).unwrap();
    assert_eq!(decoded.operation(), &underlying_operation(&bytes[1..]));
    assert_eq!(decoded.next_pc(), GuestAddress(0x2001));
    assert!(memory.is_code_current(decoded.code_snapshot()));
}

#[test]
fn hints_preserve_operand_and_address_defaults_independently_of_stack_width() {
    for default32 in [false, true] {
        for stack32 in [false, true] {
            let mut segments = SegmentContext::default();
            segments.caches[1] =
                SegmentCache::from_bits(if default32 { 0xf2b0_ffff } else { 0xf0b0_ffff }).unwrap();
            segments.caches[2] =
                SegmentCache::from_bits(if stack32 { 0xf230_ffff } else { 0xf030_ffff }).unwrap();
            let cs = CodeFetchContext::from_segments(&segments).unwrap();
            for prefix in [&[][..], &[0x66], &[0x67], &[0x66, 0x67]] {
                let plain = [prefix, &[0x89, 0x08]].concat();
                let hinted = [&[0xf3][..], plain.as_slice()].concat();
                let expected_memory = executable(&plain, 0x1000);
                let memory = executable(&hinted, 0x1000);
                for policy in [DecodePolicy::Strict, DecodePolicy::Execution] {
                    let BoundDecode::Instruction(expected) =
                        decode_bound(&expected_memory, GuestAddress(0x1000), cs, policy).unwrap()
                    else {
                        panic!("plain store")
                    };
                    let BoundDecode::Instruction(actual) =
                        decode_bound(&memory, GuestAddress(0x1000), cs, policy).unwrap()
                    else {
                        panic!("hinted store")
                    };
                    assert_eq!(actual.operation(), expected.operation());
                    assert_eq!(actual.segment_accesses(), expected.segment_accesses());
                    assert_eq!(actual.length(), expected.length() + 1);
                }
            }
        }
    }
}
