use super::*;
use crate::{
    cpu::x86::decode::{decode_for_execution, decode_one},
    cpu::x86::ir::Operation,
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
                assert!(canonical_nop(&instruction, &bytes));
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

#[test]
fn finite_gate_does_not_admit_memory_modes_or_extra_prefixes() {
    for (bitness, bytes) in [
        (16, &[0x66, 0x0f, 0x1a, 0xc1][..]),
        (32, &[0x67, 0x66, 0x0f, 0x1a, 0xc1]),
        (32, &[0x66, 0x66, 0x0f, 0x1a, 0xc1]),
        (32, &[0x66, 0x0f, 0x1a, 0x00]),
        (32, &[0x0f, 0x1a, 0xc1]),
        (32, &[0xf0, 0x66, 0x0f, 0x1a, 0xc1]),
        (32, &[0x66, 0x0f, 0x1a, 0xe1]),
        (32, &[0x66, 0x0f, 0x1a, 0xc4]),
    ] {
        let instruction = Decoder::new(bitness, bytes, DecoderOptions::NONE).decode();
        assert!(!canonical_nop(&instruction, bytes));
    }
}
