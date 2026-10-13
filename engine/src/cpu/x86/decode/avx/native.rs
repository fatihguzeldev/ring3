use super::*;
use crate::{
    cpu::x86::avx::{AvxDecodeError, AvxSemantic, MaskedInstruction},
    memory::Access,
};
use iced_x86::{Decoder, DecoderError, DecoderOptions};
use std::collections::HashSet;

struct Case {
    code: &'static str,
    bytes: Vec<u8>,
    descriptor: u32,
    operands: u32,
    immediate: u32,
    memory: Option<(u8, Access, u8)>,
    strict_w: bool,
    l_policy: u8,
    unused_vvvv: bool,
    used_vvvv: bool,
    is4: bool,
}
fn corpus() -> Vec<Case> {
    include_str!("native/fixtures/canonical.tsv")
        .lines()
        .filter(|line| !line.starts_with('#'))
        .map(|line| {
            let fields: Vec<_> = line.split('\t').collect();
            assert_eq!(fields.len(), 13);
            let number = |index: usize| fields[index].parse::<u8>().unwrap();
            let bytes = fields[1]
                .as_bytes()
                .chunks_exact(2)
                .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
                .collect();
            let access = match fields[6] {
                "read" => Some(Access::Read),
                "write" => Some(Access::Write),
                "none" => None,
                _ => panic!("invalid corpus access"),
            };
            Case {
                code: fields[0],
                bytes,
                descriptor: u32::from_str_radix(fields[2], 16).unwrap(),
                operands: u32::from_str_radix(fields[3], 16).unwrap(),
                immediate: u32::from_str_radix(fields[4], 16).unwrap(),
                memory: access.map(|access| (number(5), access, number(7))),
                strict_w: number(8) != 0,
                l_policy: number(9),
                unused_vvvv: number(10) != 0,
                used_vvvv: number(11) != 0,
                is4: number(12) != 0,
            }
        })
        .collect()
}
fn iced(bytes: &[u8]) -> Instruction {
    let mut decoder = Decoder::with_ip(32, bytes, 0x1234_5678, DecoderOptions::NONE);
    let instruction = decoder.decode();
    assert_eq!(decoder.last_error(), DecoderError::None, "{bytes:02x?}");
    assert_eq!(instruction.len(), bytes.len(), "{bytes:02x?}");
    instruction
}
fn operation(bytes: &[u8]) -> AvxOperation {
    let instruction = iced(bytes);
    lower(&instruction, bytes).unwrap().unwrap()
}

#[test]
fn every_baseline_code_and_register_memory_form_has_exact_typed_transport() {
    let mut codes = HashSet::new();
    for case in &corpus() {
        let instruction = iced(&case.bytes);
        assert_eq!(
            format!("{:?}", instruction.code()),
            case.code,
            "{:02x?}",
            case.bytes
        );
        let value = lower(&instruction, &case.bytes).unwrap().unwrap();
        assert_eq!(
            value.instruction().encode(),
            (case.descriptor, case.operands, case.immediate),
            "{:?}",
            case.code
        );
        let effect = value.instruction().memory();
        match (case.memory, effect) {
            (None, MemoryEffect::None) => assert_eq!(value.address(), None),
            (Some((bytes, access, alignment)), MemoryEffect::Full(required)) => {
                assert_eq!(
                    (required.bytes, required.access, required.alignment),
                    (bytes, access, alignment),
                    "{:?}",
                    case.code
                );
                assert_eq!(value.address().unwrap().displacement, 0x1122_3344);
            }
            (Some((bytes, access, 1)), MemoryEffect::Selected(selected)) => {
                assert_eq!(
                    (selected.width().bytes(), selected.access()),
                    (usize::from(bytes), access)
                );
                assert!(value.address().is_some());
            }
            other => panic!("{:?}: {other:?}", case.code),
        }
        codes.insert(case.code);
    }
    assert_eq!(codes.len(), 374);
    assert_eq!(corpus().len(), 688);
}

#[test]
fn descriptor_versions_reserved_bits_and_unused_shapes_fail_closed() {
    for case in &corpus() {
        let value = operation(&case.bytes).instruction();
        let (descriptor, operands, immediate) = value.encode();
        assert_eq!(
            AvxInstruction::decode_descriptor(descriptor, operands, immediate).unwrap(),
            value
        );
        for version in [0, 2, 255] {
            assert_eq!(
                AvxInstruction::decode_descriptor(
                    (descriptor & 0xffffff) | version << 24,
                    operands,
                    immediate
                ),
                Err(AvxDecodeError::Version)
            );
        }
        for bit in 16..24 {
            assert_eq!(
                AvxInstruction::decode_descriptor(descriptor | 1 << bit, operands, immediate),
                Err(AvxDecodeError::ReservedBits)
            );
        }
        for bit in 14..32 {
            assert_eq!(
                AvxInstruction::decode_descriptor(descriptor, operands | 1 << bit, immediate),
                Err(AvxDecodeError::ReservedBits)
            );
        }
        for family in [0, 18, 255] {
            assert_eq!(
                AvxInstruction::decode_descriptor(
                    (descriptor & !0xff00) | family << 8,
                    operands,
                    immediate
                ),
                Err(AvxDecodeError::Family)
            );
        }
        for variant in [0, 255] {
            assert_eq!(
                AvxInstruction::decode_descriptor(
                    (descriptor & !255) | variant,
                    operands,
                    immediate
                ),
                Err(AvxDecodeError::Family)
            );
        }
        assert!(
            AvxInstruction::decode_descriptor(descriptor, operands | 3 << 12, immediate).is_err()
        );
        assert_eq!(
            AvxInstruction::decode_descriptor(descriptor, operands, 256),
            Err(AvxDecodeError::Immediate)
        );
        assert!(
            AvxOperation::new(
                value,
                if value.memory() == MemoryEffect::None {
                    Some(EffectiveAddress::default())
                } else {
                    None
                }
            )
            .is_err()
        );
    }
    // vzeroupper has no operands or immediate, and VMOVD memory sources do not
    // carry a simultaneously encoded GPR source.
    let zero = operation(&[0xc5, 0xf8, 0x77]).instruction();
    for slots in 1..0x1000 {
        assert!(AvxInstruction::decode_descriptor(zero.encode().0, slots, 0).is_err());
    }
    assert!(AvxInstruction::decode_descriptor(zero.encode().0, 0, 1).is_err());
    let load = operation(&[0xc5, 0xf9, 0x6e, 0x0d, 0, 0, 0, 0]).instruction();
    assert!(
        AvxInstruction::decode_descriptor(load.encode().0, load.encode().1 | 1 << 6, 0).is_err()
    );
}

#[test]
fn c4_c5_ignored_w_b_and_used_vvvv_high_bits_preserve_semantics() {
    for case in &corpus() {
        let expected = operation(&case.bytes);
        let mut bytes = case.bytes.to_vec();
        bytes[1] ^= 0x20;
        assert_eq!(operation(&bytes), expected, "ignored B {:?}", case.code);
        if !case.strict_w {
            bytes = case.bytes.to_vec();
            bytes[2] |= 0x80;
            assert_eq!(operation(&bytes), expected, "ignored W {:?}", case.code);
        }
        if case.used_vvvv {
            bytes = case.bytes.to_vec();
            bytes[2] ^= 0x40;
            assert_eq!(
                operation(&bytes),
                expected,
                "ignored vvvv high {:?}",
                case.code
            );
        }
        if case.bytes[1] & 0x1f == 1 {
            bytes = vec![0xc5, case.bytes[2] | 0x80];
            bytes.extend_from_slice(&case.bytes[3..]);
            assert_eq!(operation(&bytes), expected, "C5 {:?}", case.code);
        }
    }
}

#[test]
fn reserved_w_vvvv_and_legacy_prefixes_are_distinct_from_unpredictable_l() {
    for case in &corpus() {
        let instruction = iced(&case.bytes);
        if case.strict_w {
            let mut bytes = case.bytes.to_vec();
            bytes[2] |= 0x80;
            assert_eq!(
                lower(&instruction, &bytes),
                Some(Err(DecodeError::InvalidEncoding))
            );
        }
        if case.unused_vvvv {
            let mut bytes = case.bytes.to_vec();
            bytes[2] ^= 8;
            assert_eq!(
                lower(&instruction, &bytes),
                Some(Err(DecodeError::InvalidEncoding))
            );
        }
        if case.l_policy == 3 {
            let mut bytes = case.bytes.to_vec();
            bytes[2] |= 4;
            assert_eq!(
                lower(&iced(&bytes), &bytes),
                Some(Err(unsupported())),
                "{:?}",
                case.code
            );
        } else if case.l_policy == 2 {
            let mut bytes = case.bytes.to_vec();
            bytes[2] ^= 4;
            assert_eq!(
                operation(&bytes),
                operation(&case.bytes),
                "LIG {:?}",
                case.code
            );
        }
        for prefix in [0x66, 0xf0, 0xf2, 0xf3] {
            let mut bytes = vec![prefix];
            bytes.extend_from_slice(&case.bytes);
            assert_eq!(
                lower(&instruction, &bytes),
                Some(Err(DecodeError::InvalidEncoding))
            );
        }
    }
}

#[test]
fn scalar_merge_reverse_moves_and_implicit_selected_address_keep_real_owners() {
    let normal = operation(&[0xc4, 0xe1, 0x6a, 0x10, 0xcb]);
    let reverse = operation(&[0xc4, 0xe1, 0x6a, 0x11, 0xd9]);
    assert_eq!(
        normal.instruction().semantic(),
        reverse.instruction().semantic()
    );
    let value = operation(&[0x64, 0x67, 0xc4, 0xe1, 0x68, 0x58, 0x4a, 0x12]);
    let address = value.address().unwrap();
    assert_eq!(address.base, Some(Register32::Ebp));
    assert_eq!(address.index, Some(Register32::Esi));
    assert_eq!(address.address_size, AddressSize::Bits16);
    assert_eq!(address.segment, Some(SegmentRegister::Fs));
    assert_eq!(address.displacement, 0x12);
    let masked = operation(&[0x64, 0x67, 0xc4, 0xe1, 0x79, 0xf7, 0xd3]);
    assert_eq!(
        masked.address(),
        Some(EffectiveAddress {
            base: Some(Register32::Edi),
            address_size: AddressSize::Bits16,
            segment: Some(SegmentRegister::Fs),
            ..EffectiveAddress::default()
        })
    );
    assert!(matches!(
        masked.instruction().semantic(),
        AvxSemantic::Masked(MaskedInstruction::StoreBytes { .. })
    ));
    let all = operation(&[0xc5, 0xfc, 0x77]);
    assert!(matches!(
        all.instruction().semantic(),
        AvxSemantic::Move(crate::cpu::x86::avx::MoveInstruction::ZeroAll)
    ));
    assert_eq!(all.instruction().memory(), MemoryEffect::None);
}

#[test]
fn immediate_mask_register_alias_bits_canonicalize_without_losing_control() {
    for case in corpus().iter().filter(|case| case.is4) {
        let expected = operation(&case.bytes);
        for low in 0..16 {
            for high in [0, 0x80] {
                let mut bytes = case.bytes.to_vec();
                *bytes.last_mut().unwrap() = (case.bytes.last().unwrap() & 0x70) | low | high;
                assert_eq!(operation(&bytes), expected);
            }
        }
        assert_eq!(expected.instruction().encode().2, 0);
    }
}

#[test]
fn gpr_and_flags_effects_are_exact_even_for_register_memory_aliases() {
    use crate::cpu::x86::avx::{MoveInstruction, ScalarInstruction, WordSource, WordTarget};
    let load = operation(&[0xc5, 0xf9, 0x6e, 0xcb]).instruction();
    assert_eq!(load.inputs(), [Some(Register32::Ebx), None]);
    assert_eq!(load.output(), None);
    assert!(matches!(
        load.semantic(),
        AvxSemantic::Move(MoveInstruction::DwordLoad {
            source: WordSource::Register(Register32::Ebx),
            ..
        })
    ));
    let extract = operation(&[0xc4, 0xe3, 0x79, 0x14, 0xd3, 0xff]).instruction();
    assert_eq!(extract.inputs(), [None, None]);
    assert_eq!(extract.output(), Some(Register32::Ebx));
    assert!(!extract.writes_flags());
    assert!(matches!(
        extract.semantic(),
        AvxSemantic::Scalar(ScalarInstruction::Extract {
            destination: WordTarget::Register(Register32::Ebx),
            ..
        })
    ));
    let explicit = operation(&[0xc4, 0xe3, 0x79, 0x61, 0xca, 0xff]).instruction();
    assert_eq!(
        explicit.inputs(),
        [Some(Register32::Eax), Some(Register32::Edx)]
    );
    assert_eq!(explicit.output(), Some(Register32::Ecx));
    assert!(explicit.writes_flags());
    let implicit = operation(&[0xc4, 0xe3, 0x79, 0x62, 0xca, 0xff]).instruction();
    assert_eq!(implicit.inputs(), [None, None]);
    assert_eq!(implicit.output(), None);
    assert!(implicit.writes_flags());
}
