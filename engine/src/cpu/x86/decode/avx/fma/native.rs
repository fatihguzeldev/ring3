use super::*;
use crate::{
    cpu::x86::{
        avx::{AvxDecodeError, AvxSemantic, FmaShape},
        capabilities,
        decode::decode_one,
        ir::Operation,
    },
    memory::{Access, AddressSpace, GuestAddress, PageRange, Permissions},
};
use iced_x86::{Decoder, DecoderError, DecoderOptions};
use std::collections::HashSet;

fn bytes(text: &str) -> Vec<u8> {
    text.as_bytes()
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}
fn decoded(bytes: &[u8]) -> Instruction {
    let mut decoder = Decoder::new(32, bytes, DecoderOptions::NONE);
    let value = decoder.decode();
    assert_eq!(decoder.last_error(), DecoderError::None, "{bytes:02x?}");
    assert_eq!(value.len(), bytes.len());
    value
}
fn operation(bytes: &[u8]) -> AvxOperation {
    super::super::lower(&decoded(bytes), bytes)
        .unwrap()
        .unwrap()
}

#[test]
fn all_ninety_six_codes_bind_exact_canonical_operands_and_checked_memory() {
    let mut codes = HashSet::new();
    let mut forms = 0;
    for line in include_str!("fixtures/canonical.tsv").lines() {
        let fields: Vec<_> = line.split('\t').collect();
        let bytes = bytes(fields[1]);
        let iced = decoded(&bytes);
        assert_eq!(format!("{:?}", iced.code()), fields[0]);
        let value = operation(&bytes);
        assert_eq!(
            value.instruction().encode(),
            (
                u32::from_str_radix(fields[2], 16).unwrap(),
                u32::from_str_radix(fields[3], 16).unwrap(),
                0
            )
        );
        let AvxSemantic::Fma(instruction) = value.instruction().semantic() else {
            panic!("fused family");
        };
        let length: u8 = fields[4].parse().unwrap();
        if length == 0 {
            assert_eq!(value.instruction().memory(), MemoryEffect::None);
            assert_eq!(value.address(), None);
        } else {
            assert_eq!(
                value.instruction().memory(),
                MemoryEffect::Full(crate::cpu::x86::avx::MemoryRequirement {
                    access: Access::Read,
                    bytes: length,
                    alignment: 1
                })
            );
            assert_eq!(value.address().unwrap().displacement, 0x1122_3344);
        }
        if fields[6] == "2" {
            assert!(matches!(instruction.shape, FmaShape::Scalar { .. }));
        }
        codes.insert(fields[0]);
        forms += 1;
    }
    assert_eq!(codes.len(), 96);
    assert_eq!(forms, 192);
}

#[test]
fn scalar_lig_and_ia32_register_alias_bits_preserve_the_typed_operation() {
    for line in include_str!("fixtures/canonical.tsv").lines() {
        let fields: Vec<_> = line.split('\t').collect();
        let original = bytes(fields[1]);
        let expected = operation(&original);
        for bit in [0x20, 0x40] {
            let mut alias = original.clone();
            alias[if bit == 0x20 { 1 } else { 2 }] ^= bit;
            assert_eq!(operation(&alias), expected, "{fields:?}");
        }
        if fields[6] == "2" {
            let mut alias = original.clone();
            alias[2] ^= 4;
            assert_eq!(operation(&alias), expected, "scalar FMA is LIG");
        }
    }
}

#[test]
fn byte_validation_enforces_exact_element_w_and_forbidden_legacy_prefixes() {
    for line in include_str!("fixtures/canonical.tsv").lines() {
        let fields: Vec<_> = line.split('\t').collect();
        let original = bytes(fields[1]);
        let instruction = decoded(&original);
        let mut wrong = original.clone();
        wrong[2] ^= 0x80;
        assert_eq!(
            lower(&instruction, Encoding::parse(&wrong).unwrap()),
            Some(Err(DecodeError::InvalidEncoding))
        );
        for prefix in [0x66, 0xf0, 0xf2, 0xf3] {
            let mut wrong = vec![prefix];
            wrong.extend_from_slice(&original);
            assert!(matches!(
                Encoding::parse(&wrong),
                Err(DecodeError::InvalidEncoding)
            ));
        }
    }
    assert_eq!(
        AvxInstruction::decode_descriptor(0x0100_1361, 0, 0),
        Err(AvxDecodeError::Family)
    );
}

#[test]
fn canonical_fused_gate_matches_cpuid_and_keeps_private_lowering_complete() {
    let bytes = [0xc4, 0xe2, 0x69, 0x98, 0xcb];
    assert!(matches!(
        operation(&bytes).instruction().semantic(),
        AvxSemantic::Fma(_)
    ));
    let mut memory = AddressSpace::new(1).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory.write(GuestAddress(0x1000), &bytes).unwrap();
    let value = decode_one(&memory, GuestAddress(0x1000));
    if capabilities::FMA_ENABLED {
        assert!(matches!(value.unwrap().operation(), Operation::Avx(_)));
    } else {
        assert_eq!(
            value.unwrap_err(),
            DecodeError::Unsupported(UnsupportedFeature::Simd)
        );
    }
    let leaf = capabilities::CPU_ID_LEAVES
        .iter()
        .find(|leaf| leaf.function == 1)
        .unwrap();
    assert_eq!(
        leaf.registers[2] & (1 << 12) != 0,
        capabilities::FMA_ENABLED
    );
}
