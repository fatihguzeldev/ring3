use super::*;
use crate::{
    cpu::x86::{
        SegmentCache, SegmentContext,
        decode::{
            BoundDecode, CodeFetchContext, DecodePolicy, DecodedInstruction, decode_bound,
            decode_for_execution, decode_one,
        },
        ir::Operation,
    },
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
};
use iced_x86::{Decoder, DecoderOptions};

#[test]
fn canonical_register_pairs_are_disabled_mpx_nops_without_operand_accesses() {
    for (opcode, code) in [
        (0x1a, Code::Reservednop_rm16_r16_0F1A),
        (0x1b, Code::Reservednop_rm16_r16_0F1B),
    ] {
        for left in 0..4 {
            for right in 0..4 {
                let bytes = [0x66, 0x0f, opcode, 0xc0 | left << 3 | right];
                let instruction = Decoder::new(32, &bytes, DecoderOptions::NONE).decode();
                assert_eq!(instruction.code(), code);
                assert!(disabled_nop(&instruction));
                let mut memory = AddressSpace::new(1).unwrap();
                memory
                    .map_zeroed(
                        PageRange::new(GuestAddress(0x1000), 1).unwrap(),
                        Permissions::ALL,
                    )
                    .unwrap();
                memory.write(GuestAddress(0x1000), &bytes).unwrap();
                for decoded in [
                    decode_one(&memory, GuestAddress(0x1000)).unwrap(),
                    decode_for_execution(&memory, GuestAddress(0x1000)).unwrap(),
                ] {
                    assert_eq!(decoded.operation(), &Operation::Nop);
                    assert_eq!(decoded.length(), 4);
                    assert_eq!(decoded.segment_accesses(), 0);
                }
            }
        }
    }
}

fn decoded(bytes: &[u8], default32: bool, policy: DecodePolicy) -> DecodedInstruction {
    let mut memory = AddressSpace::new(1).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory.write(GuestAddress(0x1000), bytes).unwrap();
    let mut segments = SegmentContext::default();
    segments.state.selectors[3] = 0;
    segments.caches[3] = SegmentCache::from_bits(0).unwrap();
    if !default32 {
        segments.caches[1] = SegmentCache::from_bits(0xf1bf_ffff).unwrap();
    }
    let context = CodeFetchContext::from_segments(&segments).unwrap();
    let BoundDecode::Instruction(decoded) =
        decode_bound(&memory, GuestAddress(0x1000), context, policy).unwrap()
    else {
        panic!("complete disabled mpx source must decode");
    };
    assert!(memory.is_code_current(decoded.code_snapshot()));
    decoded
}

#[test]
fn disabled_mpx_forms_ignore_operands_and_enabled_address_size_restrictions() {
    for default32 in [false, true] {
        for bytes in [
            &[0x0f, 0x1a, 0x00][..],
            &[0x0f, 0x1b, 0x00],
            &[0xf3, 0x0f, 0x1a, 0x00],
            &[0xf2, 0x0f, 0x1a, 0x00],
            &[0xf2, 0x0f, 0x1b, 0x00],
            &[0xf3, 0x0f, 0x1b, 0x00],
            &[0x66, 0x0f, 0x1a, 0x00],
            &[0x66, 0x0f, 0x1b, 0x00],
            &[0x67, 0x66, 0x0f, 0x1a, 0x00],
            &[0x67, 0xf3, 0x0f, 0x1b, 0x00],
            &[0x64, 0x66, 0x0f, 0x1a, 0x00],
            &[0x36, 0x66, 0x0f, 0x1b, 0x00],
            &[0x66, 0xf2, 0x0f, 0x1a, 0xc1],
            &[0xf2, 0x66, 0x0f, 0x1a, 0xc1],
            &[0x66, 0x0f, 0x1a, 0xe1],
            &[0x66, 0x0f, 0x1b, 0xc4],
        ] {
            let bitness = if default32 { 32 } else { 16 };
            let instruction = Decoder::new(bitness, bytes, DecoderOptions::NONE).decode();
            assert!(disabled_nop(&instruction), "{bitness}: {bytes:02x?}");
            for policy in [DecodePolicy::Strict, DecodePolicy::Execution] {
                let decoded = decoded(bytes, default32, policy);
                assert_eq!(decoded.operation(), &Operation::Nop);
                assert_eq!(usize::from(decoded.length()), bytes.len());
                assert_eq!(decoded.next_pc(), GuestAddress(0x1000 + bytes.len() as u32));
                assert_eq!(decoded.segment_accesses(), 0);
            }
        }
    }
}

#[test]
fn neighboring_reserved_opcodes_and_duplicate_or_lock_prefixes_stay_outside_the_gate() {
    for bytes in [
        &[0x66, 0x0f, 0x19, 0xc1][..],
        &[0x66, 0x0f, 0x1c, 0xc1],
        &[0xf0, 0x66, 0x0f, 0x1a, 0xc1],
        &[0xf0, 0x66, 0x0f, 0x1b, 0x00],
    ] {
        let instruction = Decoder::new(32, bytes, DecoderOptions::NONE).decode();
        assert!(!disabled_nop(&instruction));
    }
    for bytes in [
        &[0x66, 0x66, 0x0f, 0x1a, 0xc1][..],
        &[0xf2, 0xf3, 0x0f, 0x1b, 0x00],
    ] {
        let instruction = Decoder::new(32, bytes, DecoderOptions::NONE).decode();
        assert!(disabled_nop(&instruction));
        assert!(super::super::profile::check_profile(&instruction, bytes).is_err());
    }
}
