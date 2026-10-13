use super::*;
use iced_x86::{Decoder, DecoderError, DecoderOptions};

fn decoded(bitness: u32, bytes: &[u8], pc: u64) -> Instruction {
    let mut decoder = Decoder::with_ip(bitness, bytes, pc, DecoderOptions::NONE);
    let instruction = decoder.decode();
    assert_eq!(decoder.last_error(), DecoderError::None);
    assert_eq!(instruction.len(), bytes.len());
    instruction
}

#[test]
fn implicit_strings_use_decoded_address_size_for_both_code_defaults() {
    let forms = [
        (0xa4, StringKind::Move, false),
        (0xa5, StringKind::Move, true),
        (0xaa, StringKind::Store, false),
        (0xab, StringKind::Store, true),
        (0xac, StringKind::Load, false),
        (0xad, StringKind::Load, true),
        (0xa6, StringKind::Compare, false),
        (0xa7, StringKind::Compare, true),
        (0xae, StringKind::Scan, false),
        (0xaf, StringKind::Scan, true),
    ];
    for bitness in [16, 32] {
        for address_override in [false, true] {
            for operand_override in [false, true] {
                for repeated in [false, true] {
                    for (opcode, expected_kind, wide) in forms {
                        let mut bytes = Vec::new();
                        if repeated {
                            bytes.push(0xf3);
                        }
                        if operand_override {
                            bytes.push(0x66);
                        }
                        if address_override {
                            bytes.push(0x67);
                        }
                        bytes.push(opcode);
                        let instruction = decoded(bitness, &bytes, 0x1000);
                        let expected_address = if (bitness == 16) ^ address_override {
                            AddressSize::Bits16
                        } else {
                            AddressSize::Bits32
                        };
                        let expected_width = if !wide {
                            StringWidth::Byte
                        } else if (bitness == 16) ^ operand_override {
                            StringWidth::Word
                        } else {
                            StringWidth::Dword
                        };
                        let operation = lower(&instruction, &bytes).unwrap().unwrap();
                        let (kind, width, address_size, is_repeat) = match operation {
                            Operation::String {
                                kind,
                                width,
                                address_size,
                                ..
                            } => (kind, width, address_size, false),
                            Operation::RepeatString {
                                kind,
                                width,
                                address_size,
                                ..
                            } => (kind, width, address_size, true),
                            other => panic!("unexpected operation: {other:?}"),
                        };
                        assert_eq!(
                            (kind, width, address_size, is_repeat),
                            (expected_kind, expected_width, expected_address, repeated),
                            "bitness={bitness} bytes={bytes:02x?}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn default16_scalar_address_and_stack_forms_reach_existing_lowerers() {
    let forms: &[&[u8]] = &[
        &[0xb8, 0x34, 0x12],
        &[0x66, 0xb8, 0x78, 0x56, 0x34, 0x12],
        &[0x8b, 0x00],
        &[0x67, 0x8b, 0x00],
        &[0xa1, 0x00, 0x50],
        &[0x67, 0xa1, 0x00, 0x50, 0, 0],
        &[0xd7],
        &[0x67, 0xd7],
        &[0x50],
        &[0x66, 0x50],
        &[0x58],
        &[0x66, 0x58],
        &[0x9c],
        &[0x9d],
        &[0xc9],
        &[0x66, 0xc9],
        &[0xc8, 0, 0, 0],
        &[0xc3],
        &[0x66, 0xc3],
    ];
    for bytes in forms {
        let instruction = decoded(16, bytes, 0x1000);
        assert!(
            super::super::lower::lower(&instruction, bytes, None).is_ok(),
            "bytes={bytes:02x?}"
        );
    }
    for (bytes, expected) in [
        (&[0x8b, 0x00][..], AddressSize::Bits16),
        (&[0x67, 0x8b, 0x00][..], AddressSize::Bits32),
        (&[0xa1, 0x00, 0x50][..], AddressSize::Bits16),
        (&[0x67, 0xa1, 0x00, 0x50, 0, 0][..], AddressSize::Bits32),
    ] {
        let instruction = decoded(16, bytes, 0x1000);
        let Operation::LoadWord { address, .. } =
            super::super::lower::lower(&instruction, bytes, None).unwrap()
        else {
            panic!("expected word load: {bytes:02x?}");
        };
        assert_eq!(address.address_size, expected);
    }
}

#[test]
fn default16_branch_target_width_is_distinct_from_full_next_ip() {
    for (bytes, pc, expected_next, expected_target, uses_cx) in [
        (&[0xeb, 0][..], 0x1fffe, 0x20000, 0, None),
        (
            &[0x66, 0xe9, 0, 0, 0, 0][..],
            0x1fffa,
            0x20000,
            0x20000,
            None,
        ),
        (&[0x74, 0][..], 0x1fffe, 0x20000, 0, None),
        (&[0xe2, 0][..], 0x1fffe, 0x20000, 0, Some(true)),
        (&[0x67, 0xe2, 0][..], 0x1fffd, 0x20000, 0, Some(false)),
        (&[0xe8, 0, 0][..], 0x1fffd, 0x20000, 0, None),
        (
            &[0x66, 0xe8, 0, 0, 0, 0][..],
            0x1fffa,
            0x20000,
            0x20000,
            None,
        ),
    ] {
        let instruction = decoded(16, bytes, pc);
        assert_eq!(instruction.next_ip32(), expected_next);
        let operation = super::super::lower::lower(&instruction, bytes, None).unwrap();
        let target = match operation {
            Operation::Jump {
                target: crate::cpu::x86::ir::BranchTarget::Direct(target),
            }
            | Operation::ConditionalJump { target, .. }
            | Operation::Call {
                target: crate::cpu::x86::ir::BranchTarget::Direct(target),
            }
            | Operation::CallWord { target } => target,
            Operation::CountBranchWord { target, .. } => {
                assert_eq!(uses_cx, Some(true));
                target
            }
            Operation::CountBranch { target, .. } => {
                assert_eq!(uses_cx, Some(false));
                target
            }
            other => panic!("unexpected operation: {other:?}"),
        };
        assert_eq!(target.0, expected_target, "bytes={bytes:02x?}");
    }
    assert_eq!(decoded(16, &[0x90], 0xffff).next_ip32(), 0x10000);
}
