use super::*;
use iced_x86::{Decoder, DecoderOptions};

fn decoded(bytes: &[u8]) -> Instruction {
    let mut decoder = Decoder::with_ip(32, bytes, 0x1000, DecoderOptions::NONE);
    let instruction = decoder.decode();
    assert!(!instruction.is_invalid());
    instruction
}

#[test]
fn every_raw_alias_keeps_mixed_roles_and_requires_the_common_avx2_gate() {
    for (id, control, integer_opcode) in [
        (1, 0x61, 0x90),
        (2, 0x65, 0x90),
        (3, 0xe1, 0x90),
        (4, 0xe5, 0x90),
        (5, 0x61, 0x91),
        (6, 0x65, 0x91),
        (7, 0xe1, 0x91),
        (8, 0xe5, 0x91),
    ] {
        for opcode in [integer_opcode, integer_opcode + 2] {
            let bytes = [0xc4, 0xe2, control, opcode, 0x0c, 0x10];
            let instruction = decoded(&bytes);
            let result = lower(&instruction, Encoding::parse(&bytes).unwrap()).unwrap();
            if crate::cpu::x86::capabilities::AVX2_ENABLED {
                let operation = result.unwrap();
                assert_eq!(
                    operation.instruction().encode(),
                    (0x01001400 | id, 1 | (2 << 3) | (3 << 9) | (1 << 12), 0)
                );
                assert_eq!(operation.address(), None);
                let address = operation.gather_address().unwrap();
                assert_eq!(address.base(), Some(Register32::Eax));
                assert_eq!(address.displacement(), 0);
                assert_eq!(address.resolved_segment(), SegmentRegister::Ds);
            } else {
                assert_eq!(
                    result,
                    Err(DecodeError::Unsupported(UnsupportedFeature::Simd))
                );
            }
        }
    }
}

#[test]
fn synthetic_wrong_vector_roles_and_aliases_refuse_before_capability() {
    let bytes = [0xc4, 0xe2, 0x61, 0x90, 0x0c, 0x10];
    let valid = decoded(&bytes);
    let encoding = Encoding::parse(&bytes).unwrap();
    for target in 0..5 {
        let mut instruction = valid;
        match target {
            0 => instruction.set_op0_register(Register::YMM1),
            1 => instruction.set_op2_register(Register::YMM3),
            2 => instruction.set_memory_index(Register::YMM2),
            3 => instruction.set_memory_index(Register::XMM1),
            4 => instruction.set_memory_base(Register::SI),
            _ => unreachable!(),
        }
        assert_eq!(
            lower(&instruction, encoding),
            Some(Err(DecodeError::InvalidEncoding))
        );
    }
    let wrong_w = Encoding::parse(&[0xc4, 0xe2, 0xe1, 0x90, 0x0c, 0x10]).unwrap();
    assert_eq!(
        lower(&valid, wrong_w),
        Some(Err(DecodeError::InvalidEncoding))
    );
    let wrong_l = Encoding::parse(&[0xc4, 0xe2, 0x65, 0x90, 0x0c, 0x10]).unwrap();
    assert_eq!(
        lower(&valid, wrong_l),
        Some(Err(DecodeError::InvalidEncoding))
    );
}

#[test]
fn gathered_operands_keep_default_and_override_segment_record_preflight() {
    use crate::{
        cpu::x86::decode::decode_one,
        memory::{AddressSpace, GuestAddress, PageRange, Permissions},
    };
    for (bytes, segment) in [
        (
            vec![0xc4, 0xe2, 0x61, 0x90, 0x0c, 0x10],
            SegmentRegister::Ds,
        ),
        (
            vec![0xc4, 0xe2, 0x61, 0x90, 0x4c, 0x15, 0],
            SegmentRegister::Ss,
        ),
        (
            vec![0x64, 0xc4, 0xe2, 0x61, 0x90, 0x0c, 0x10],
            SegmentRegister::Fs,
        ),
    ] {
        let mut memory = AddressSpace::new(1).unwrap();
        memory
            .map_zeroed(
                PageRange::new(GuestAddress(0x1000), 1).unwrap(),
                Permissions::ALL,
            )
            .unwrap();
        memory.write(GuestAddress(0x1000), &bytes).unwrap();
        let result = decode_one(&memory, GuestAddress(0x1000));
        if crate::cpu::x86::capabilities::AVX2_ENABLED {
            let decoded = result.unwrap();
            assert!(decoded.uses_segments());
            assert_eq!(decoded.segment_accesses(), 1 << segment.index());
            let crate::cpu::x86::ir::Operation::Avx(operation) = decoded.operation() else {
                panic!("gather operation")
            };
            assert_eq!(operation.address(), None);
            assert_eq!(
                operation.gather_address().unwrap().resolved_segment(),
                segment
            );
        } else {
            assert_eq!(
                result.unwrap_err(),
                DecodeError::Unsupported(UnsupportedFeature::Simd)
            );
        }
    }
}
