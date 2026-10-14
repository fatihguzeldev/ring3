use crate::{
    cpu::x86::{
        ControlOperation, SegmentCache, SegmentContext,
        decode::{
            BoundDecode, CodeFetchContext, DecodeError, DecodePolicy, decode_bound,
            decode_for_execution, decode_one,
        },
        ir::Operation,
    },
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
};
use iced_x86::{Code, Decoder, DecoderOptions};

fn pairs() -> [(Code, Code, &'static [u8]); 17] {
    [
        (
            Code::Add_rm8_imm8_82,
            Code::Add_rm8_imm8,
            &[0x82, 0xc0, 0x01],
        ),
        (Code::Or_rm8_imm8_82, Code::Or_rm8_imm8, &[0x82, 0xc8, 0x01]),
        (
            Code::Adc_rm8_imm8_82,
            Code::Adc_rm8_imm8,
            &[0x82, 0xd0, 0x01],
        ),
        (
            Code::Sbb_rm8_imm8_82,
            Code::Sbb_rm8_imm8,
            &[0x82, 0xd8, 0x01],
        ),
        (
            Code::And_rm8_imm8_82,
            Code::And_rm8_imm8,
            &[0x82, 0xe0, 0x01],
        ),
        (
            Code::Sub_rm8_imm8_82,
            Code::Sub_rm8_imm8,
            &[0x82, 0xe8, 0x01],
        ),
        (
            Code::Xor_rm8_imm8_82,
            Code::Xor_rm8_imm8,
            &[0x82, 0xf0, 0x01],
        ),
        (
            Code::Cmp_rm8_imm8_82,
            Code::Cmp_rm8_imm8,
            &[0x82, 0xf8, 0x01],
        ),
        (Code::Sal_rm8_1, Code::Shl_rm8_1, &[0xd0, 0xf0]),
        (Code::Sal_rm8_CL, Code::Shl_rm8_CL, &[0xd2, 0xf0]),
        (Code::Sal_rm8_imm8, Code::Shl_rm8_imm8, &[0xc0, 0xf0, 0x01]),
        (Code::Sal_rm16_1, Code::Shl_rm16_1, &[0x66, 0xd1, 0xf0]),
        (Code::Sal_rm16_CL, Code::Shl_rm16_CL, &[0x66, 0xd3, 0xf0]),
        (
            Code::Sal_rm16_imm8,
            Code::Shl_rm16_imm8,
            &[0x66, 0xc1, 0xf0, 0x01],
        ),
        (Code::Sal_rm32_1, Code::Shl_rm32_1, &[0xd1, 0xf0]),
        (Code::Sal_rm32_CL, Code::Shl_rm32_CL, &[0xd3, 0xf0]),
        (
            Code::Sal_rm32_imm8,
            Code::Shl_rm32_imm8,
            &[0xc1, 0xf0, 0x01],
        ),
    ]
}
fn executable(bytes: &[u8], pc: u32) -> AddressSpace {
    let mut m = AddressSpace::new(2).unwrap();
    m.map_zeroed(
        PageRange::new(GuestAddress(pc & !0xfff), 1).unwrap(),
        Permissions::ALL,
    )
    .unwrap();
    m.write(GuestAddress(pc), bytes).unwrap();
    m
}
fn mode_bytes(bytes: &[u8], word_or_dword: bool, default32: bool) -> Vec<u8> {
    let mut bytes = bytes.to_vec();
    if word_or_dword && !default32 {
        if bytes[0] == 0x66 {
            bytes.remove(0);
        } else {
            bytes.insert(0, 0x66);
        }
    }
    bytes
}
#[test]
fn exact_seventeen_aliases_exclude_held_and_canonical_codes() {
    for (alias, canonical, _) in pairs() {
        assert_eq!(super::canonical_code(alias), Some(canonical));
        assert_eq!(super::canonical_code(canonical), None);
    }
    for code in [
        Code::Salc,
        Code::Test_rm8_imm8_F6r1,
        Code::Test_rm16_imm16_F7r1,
        Code::Test_rm32_imm32_F7r1,
        Code::Movzx_r16_rm16,
        Code::Movsx_r16_rm16,
        Code::Bswap_r16,
        Code::Reservednop_rm32_r32_0F1A,
    ] {
        assert_eq!(super::canonical_code(code), None);
    }
}
#[test]
fn normalization_preserves_every_decoded_field_and_both_modes_register_memory_ir() {
    for (alias, canonical, bytes) in pairs() {
        let width32 = matches!(
            alias,
            Code::Sal_rm16_1
                | Code::Sal_rm16_CL
                | Code::Sal_rm16_imm8
                | Code::Sal_rm32_1
                | Code::Sal_rm32_CL
                | Code::Sal_rm32_imm8
        );
        for default32 in [false, true] {
            for memory in [false, true] {
                let mut raw = mode_bytes(bytes, width32, default32);
                let opcode = usize::from(raw[0] == 0x66);
                let mut normal = raw.clone();
                if raw[opcode] == 0x82 {
                    normal[opcode] = 0x80;
                } else {
                    normal[opcode + 1] -= 0x10;
                }
                if memory {
                    raw[opcode + 1] &= 0x38;
                    normal[opcode + 1] &= 0x38;
                    raw.splice(0..0, [0x64, 0x67]);
                    normal.splice(0..0, [0x64, 0x67]);
                }
                let original = Decoder::with_ip(
                    if default32 { 32 } else { 16 },
                    &raw,
                    0x1000,
                    DecoderOptions::NONE,
                )
                .decode();
                let expected = Decoder::with_ip(
                    if default32 { 32 } else { 16 },
                    &normal,
                    0x1000,
                    DecoderOptions::NONE,
                )
                .decode();
                assert_eq!(original.code(), alias);
                assert_eq!(expected.code(), canonical);
                let mut normalized = original;
                normalized.set_code(super::canonical_code(alias).unwrap());
                assert!(normalized.eq_all_bits(&expected));
                assert_eq!(original.code(), alias);
                let a = executable(&raw, 0x1000);
                let c = executable(&normal, 0x1000);
                let mut segments = SegmentContext::default();
                if !default32 {
                    segments.caches[1] = SegmentCache::from_bits(0xf1bf_ffff).unwrap();
                }
                let cs = CodeFetchContext::from_segments(&segments).unwrap();
                for policy in [DecodePolicy::Strict, DecodePolicy::Execution] {
                    let BoundDecode::Instruction(actual) =
                        decode_bound(&a, GuestAddress(0x1000), cs, policy).unwrap()
                    else {
                        panic!("complete alias")
                    };
                    let BoundDecode::Instruction(want) =
                        decode_bound(&c, GuestAddress(0x1000), cs, policy).unwrap()
                    else {
                        panic!("complete canonical")
                    };
                    assert_eq!(actual.operation(), want.operation());
                    assert_eq!(actual.segment_accesses(), want.segment_accesses());
                    assert_eq!(actual.length(), want.length());
                    assert_eq!(actual.next_pc(), want.next_pc());
                    assert!(a.is_code_current(actual.code_snapshot()));
                }
            }
        }
    }
}
#[test]
fn count_literals_and_existing_locked_atomic_routes_keep_canonical_ir() {
    for (alias, _, bytes) in pairs() {
        if !matches!(
            alias,
            Code::Sal_rm8_imm8 | Code::Sal_rm16_imm8 | Code::Sal_rm32_imm8
        ) {
            continue;
        }
        for count in [0, 32, 33, 255] {
            let mut a = bytes.to_vec();
            *a.last_mut().unwrap() = count;
            let mut c = a.clone();
            let opcode = usize::from(c[0] == 0x66);
            c[opcode + 1] -= 0x10;
            assert_eq!(
                decode_one(&executable(&a, 0x1000), GuestAddress(0x1000))
                    .unwrap()
                    .operation(),
                decode_one(&executable(&c, 0x1000), GuestAddress(0x1000))
                    .unwrap()
                    .operation()
            );
        }
    }
    for reg in 0..7 {
        let a = [0xf0, 0x82, reg << 3, 1];
        let c = [0xf0, 0x80, reg << 3, 1];
        assert_eq!(
            decode_one(&executable(&a, 0x1000), GuestAddress(0x1000))
                .unwrap()
                .operation(),
            decode_one(&executable(&c, 0x1000), GuestAddress(0x1000))
                .unwrap()
                .operation()
        );
    }
}
#[test]
fn fetch_and_invalid_lock_or_duplicate_refusals_precede_alias_lowering() {
    for bytes in [&[0x82, 0xc0, 1][..], &[0xc1, 0xf0, 33]] {
        let pc = 0x2001 - bytes.len() as u32;
        let mut m = executable(&bytes[..bytes.len() - 1], pc);
        assert!(
            matches!(decode_one(&m,GuestAddress(pc)),Err(DecodeError::MemoryFault{length,..}) if length==bytes.len() as u32)
        );
        let page = PageRange::new(GuestAddress(0x2000), 1).unwrap();
        m.map_zeroed(page, Permissions::ALL).unwrap();
        m.write(GuestAddress(0x2000), &bytes[bytes.len() - 1..])
            .unwrap();
        m.protect(page, Permissions::READ).unwrap();
        assert!(matches!(
            decode_for_execution(&m, GuestAddress(pc)),
            Err(DecodeError::MemoryFault { .. })
        ));
        m.protect(page, Permissions::EXECUTE).unwrap();
        assert!(decode_for_execution(&m, GuestAddress(pc)).is_ok());
    }
    for bytes in [&[0xf0, 0x82, 0x38, 1][..], &[0xf0, 0xd1, 0x30]] {
        let m = executable(bytes, 0x1000);
        assert_eq!(
            decode_one(&m, GuestAddress(0x1000)).unwrap_err(),
            DecodeError::InvalidEncoding
        );
        assert_eq!(
            decode_for_execution(&m, GuestAddress(0x1000))
                .unwrap()
                .operation(),
            &Operation::Control(ControlOperation::InvalidOpcode)
        );
    }
    let bytes = [0x66, 0x66, 0x82, 0xc0, 1];
    let m = executable(&bytes, 0x1000);
    let error = decode_one(&m, GuestAddress(0x1000)).unwrap_err();
    assert!(matches!(error, DecodeError::Unsupported(_)));
    assert_eq!(
        decode_for_execution(&m, GuestAddress(0x1000)).unwrap_err(),
        error
    );
}
