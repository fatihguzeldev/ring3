use ring3_engine::cpu::x86::Register32;
use ring3_engine::cpu::x86::decode::{DecodeError, decode_one};
use ring3_engine::cpu::x86::ir::{
    BranchTarget, Condition, EffectiveAddress, Location32, Operation, Value32,
};
use ring3_engine::memory::{
    Access, AddressSpace, GuestAddress, MemoryError, PageRange, Permissions,
};

fn code_space(pc: u32, bytes: &[u8]) -> AddressSpace {
    let first = pc & !0xfff;
    let pages = ((pc as u64 - first as u64 + bytes.len() as u64).div_ceil(4096)) as u32;
    let mut space = AddressSpace::new(pages).unwrap();
    let range = PageRange::new(GuestAddress(first), pages).unwrap();
    space.map_zeroed(range, Permissions::ALL).unwrap();
    space.write(GuestAddress(pc), bytes).unwrap();
    space.protect(range, Permissions::EXECUTE).unwrap();
    space
}

fn address(base: Option<Register32>, displacement: u32) -> EffectiveAddress {
    EffectiveAddress {
        base,
        index: None,
        scale: 1,
        displacement,
    }
}

fn direct(address: u32) -> BranchTarget {
    BranchTarget::Direct(GuestAddress(address))
}

fn assert_decodes(pc: u32, bytes: &[u8], expected: Operation) {
    let space = code_space(pc, bytes);
    let decoded = decode_one(&space, GuestAddress(pc)).unwrap();
    assert_eq!(decoded.pc().0, pc);
    assert_eq!(decoded.length() as usize, bytes.len());
    assert_eq!(decoded.next_pc().0, pc.wrapping_add(bytes.len() as u32));
    assert_eq!(decoded.operation(), &expected);
    assert!(space.is_code_current(decoded.code_snapshot()));
}

#[test]
fn authored_direct_flow_fixture_preserves_llvm_observed_targets() {
    for (pc, bytes, expected) in [
        (
            0x1057,
            &[0xe8, 0x9a, 0, 0, 0][..],
            Operation::Call {
                target: direct(0x10f6),
            },
        ),
        (
            0x1067,
            &[0x74, 2][..],
            Operation::ConditionalJump {
                condition: Condition::Equal,
                target: GuestAddress(0x106b),
            },
        ),
        (
            0x1069,
            &[0xeb, 0][..],
            Operation::Jump {
                target: direct(0x106b),
            },
        ),
        (
            0x106b,
            &[0x0f, 0x85, 0x85, 0, 0, 0][..],
            Operation::ConditionalJump {
                condition: Condition::NotEqual,
                target: GuestAddress(0x10f6),
            },
        ),
        (
            0x1071,
            &[0xe9, 0x80, 0, 0, 0][..],
            Operation::Jump {
                target: direct(0x10f6),
            },
        ),
    ] {
        assert_decodes(pc, bytes, expected);
    }
}

#[test]
fn authored_indirect_flow_fixture_preserves_register_or_memory_target() {
    use Register32::{Eax, Ebx, Edx, Esp};
    for (bytes, expected) in [
        (
            &[0xff, 0xd0][..],
            Operation::Call {
                target: BranchTarget::Indirect(Location32::Register(Eax)),
            },
        ),
        (
            &[0xff, 0x53, 8][..],
            Operation::Call {
                target: BranchTarget::Indirect(Location32::Memory(address(Some(Ebx), 8))),
            },
        ),
        (
            &[0xff, 0xe2][..],
            Operation::Jump {
                target: BranchTarget::Indirect(Location32::Register(Edx)),
            },
        ),
        (
            &[0xff, 0x64, 0x24, 4][..],
            Operation::Jump {
                target: BranchTarget::Indirect(Location32::Memory(address(Some(Esp), 4))),
            },
        ),
    ] {
        assert_decodes(0x1000, bytes, expected);
    }
}

#[test]
fn all_sixteen_short_and_near_condition_encodings_have_explicit_conditions() {
    for (short, near, condition) in [
        (0x70, 0x80, Condition::Overflow),
        (0x71, 0x81, Condition::NotOverflow),
        (0x72, 0x82, Condition::Below),
        (0x73, 0x83, Condition::AboveOrEqual),
        (0x74, 0x84, Condition::Equal),
        (0x75, 0x85, Condition::NotEqual),
        (0x76, 0x86, Condition::BelowOrEqual),
        (0x77, 0x87, Condition::Above),
        (0x78, 0x88, Condition::Sign),
        (0x79, 0x89, Condition::NotSign),
        (0x7a, 0x8a, Condition::Parity),
        (0x7b, 0x8b, Condition::NotParity),
        (0x7c, 0x8c, Condition::Less),
        (0x7d, 0x8d, Condition::GreaterOrEqual),
        (0x7e, 0x8e, Condition::LessOrEqual),
        (0x7f, 0x8f, Condition::Greater),
    ] {
        assert_decodes(
            0x1000,
            &[short, 0x80],
            Operation::ConditionalJump {
                condition,
                target: GuestAddress(0x0f82),
            },
        );
        assert_decodes(
            0x1000,
            &[0x0f, near, 0xff, 0xff, 0xff, 0xff],
            Operation::ConditionalJump {
                condition,
                target: GuestAddress(0x1005),
            },
        );
    }
}

#[test]
fn negative_relative_transfers_are_relative_to_instruction_end() {
    for (bytes, expected) in [
        (
            &[0xeb, 0xfe][..],
            Operation::Jump {
                target: direct(0x1000),
            },
        ),
        (
            &[0xe9, 0xfb, 0xff, 0xff, 0xff][..],
            Operation::Jump {
                target: direct(0x1000),
            },
        ),
        (
            &[0xe8, 0xfb, 0xff, 0xff, 0xff][..],
            Operation::Call {
                target: direct(0x1000),
            },
        ),
        (
            &[0x74, 0xfe][..],
            Operation::ConditionalJump {
                condition: Condition::Equal,
                target: GuestAddress(0x1000),
            },
        ),
    ] {
        assert_decodes(0x1000, bytes, expected);
    }
}

#[test]
fn branch_targets_wrap_in_32_bits_while_instruction_bytes_stay_in_range() {
    for (pc, bytes, expected) in [
        (
            0xffff_ff00,
            &[0xe8, 0, 2, 0, 0][..],
            Operation::Call {
                target: direct(0x105),
            },
        ),
        (
            0xffff_ff00,
            &[0xe9, 0, 2, 0, 0][..],
            Operation::Jump {
                target: direct(0x105),
            },
        ),
        (
            0xffff_ff00,
            &[0x0f, 0x84, 0, 2, 0, 0][..],
            Operation::ConditionalJump {
                condition: Condition::Equal,
                target: GuestAddress(0x106),
            },
        ),
        (
            0xffff_fff0,
            &[0xeb, 0x7f][..],
            Operation::Jump {
                target: direct(0x71),
            },
        ),
        (
            0xffff_fff0,
            &[0x75, 0x7f][..],
            Operation::ConditionalJump {
                condition: Condition::NotEqual,
                target: GuestAddress(0x71),
            },
        ),
    ] {
        assert_decodes(pc, bytes, expected);
    }
}

#[test]
fn decoding_indirect_targets_and_stack_operands_does_not_read_unmapped_ram() {
    for (bytes, expected) in [
        (
            &[0xff, 0x15, 0x78, 0x56, 0x34, 0x12][..],
            Operation::Call {
                target: BranchTarget::Indirect(Location32::Memory(address(None, 0x1234_5678))),
            },
        ),
        (
            &[0xff, 0x25, 0x78, 0x56, 0x34, 0x12][..],
            Operation::Jump {
                target: BranchTarget::Indirect(Location32::Memory(address(None, 0x1234_5678))),
            },
        ),
        (
            &[0xff, 0x35, 0x78, 0x56, 0x34, 0x12][..],
            Operation::Push {
                source: Value32::Memory(address(None, 0x1234_5678)),
            },
        ),
        (
            &[0x8f, 0x05, 0x78, 0x56, 0x34, 0x12][..],
            Operation::Pop {
                destination: Location32::Memory(address(None, 0x1234_5678)),
            },
        ),
    ] {
        let space = code_space(0x1000, bytes);
        assert!(matches!(
            space.resolve(GuestAddress(0x1234_5678), Access::Read),
            Err(MemoryError::Fault(_))
        ));
        let decoded = decode_one(&space, GuestAddress(0x1000)).unwrap();
        assert_eq!(decoded.operation(), &expected);
    }
    let space = code_space(0x1000, &[0xe8, 0xfb, 0xef, 0, 0]);
    assert!(matches!(
        space.resolve(GuestAddress(0x10000), Access::Execute),
        Err(MemoryError::Fault(_))
    ));
    assert_eq!(
        decode_one(&space, GuestAddress(0x1000))
            .unwrap()
            .operation(),
        &Operation::Call {
            target: direct(0x10000)
        }
    );
}

#[test]
fn authored_stack_and_return_fixture_matches_operand_identity() {
    use Register32::{Eax, Ecx, Edi, Esp};
    for (bytes, expected) in [
        (
            &[0x50][..],
            Operation::Push {
                source: Value32::Register(Eax),
            },
        ),
        (
            &[0xff, 0x74, 0x24, 4][..],
            Operation::Push {
                source: Value32::Memory(address(Some(Esp), 4)),
            },
        ),
        (
            &[0x6a, 0xff][..],
            Operation::Push {
                source: Value32::Immediate(u32::MAX),
            },
        ),
        (
            &[0x59][..],
            Operation::Pop {
                destination: Location32::Register(Ecx),
            },
        ),
        (
            &[0x8f, 0x07][..],
            Operation::Pop {
                destination: Location32::Memory(address(Some(Edi), 0)),
            },
        ),
        (&[0xc3][..], Operation::Return { stack_adjust: 0 }),
        (&[0xc2, 12, 0][..], Operation::Return { stack_adjust: 12 }),
    ] {
        assert_decodes(0x1000, bytes, expected);
    }
}

#[test]
fn all_register_push_and_pop_encodings_use_the_x86_register_order() {
    use Register32::{Eax, Ebp, Ebx, Ecx, Edi, Edx, Esi, Esp};
    for (push, pop, register) in [
        (0x50, 0x58, Eax),
        (0x51, 0x59, Ecx),
        (0x52, 0x5a, Edx),
        (0x53, 0x5b, Ebx),
        (0x54, 0x5c, Esp),
        (0x55, 0x5d, Ebp),
        (0x56, 0x5e, Esi),
        (0x57, 0x5f, Edi),
    ] {
        assert_decodes(
            0x1000,
            &[push],
            Operation::Push {
                source: Value32::Register(register),
            },
        );
        assert_decodes(
            0x1000,
            &[pop],
            Operation::Pop {
                destination: Location32::Register(register),
            },
        );
    }
}

#[test]
fn push_immediates_preserve_32_bits_and_sign_extend_8_bit_values() {
    assert_decodes(
        0x1000,
        &[0x68, 0x78, 0x56, 0x34, 0x12],
        Operation::Push {
            source: Value32::Immediate(0x1234_5678),
        },
    );
    for (byte, value) in [(0, 0), (0x7f, 127), (0x80, 0xffff_ff80), (0xff, u32::MAX)] {
        assert_decodes(
            0x1000,
            &[0x6a, byte],
            Operation::Push {
                source: Value32::Immediate(value),
            },
        );
    }
}

#[test]
fn pop_esp_address_and_unsigned_return_adjustment_are_preserved() {
    assert_decodes(
        0x1000,
        &[0x8f, 0x04, 0x24],
        Operation::Pop {
            destination: Location32::Memory(address(Some(Register32::Esp), 0)),
        },
    );
    assert_decodes(
        0x1000,
        &[0xc2, 0xff, 0xff],
        Operation::Return {
            stack_adjust: u16::MAX,
        },
    );
}

#[test]
fn final_address_return_decodes_without_reading_stack_or_wrapping_fetch() {
    let space = code_space(u32::MAX, &[0xc3]);
    let decoded = decode_one(&space, GuestAddress(u32::MAX)).unwrap();
    assert_eq!(decoded.length(), 1);
    assert_eq!(decoded.next_pc().0, 0);
    assert_eq!(decoded.operation(), &Operation::Return { stack_adjust: 0 });
}

#[test]
fn far_control_transfers_are_rejected_as_unsupported() {
    for bytes in [
        &[0x9a, 0x78, 0x56, 0x34, 0x12, 8, 0][..],
        &[0xea, 0x78, 0x56, 0x34, 0x12, 8, 0][..],
        &[0xff, 0x1b][..],
        &[0xff, 0x2b][..],
        &[0xcb][..],
        &[0xca, 12, 0][..],
    ] {
        let space = code_space(0x1000, bytes);
        assert!(matches!(
            decode_one(&space, GuestAddress(0x1000)),
            Err(DecodeError::Unsupported(_))
        ));
    }
}

#[test]
fn sixteen_bit_flow_and_stack_forms_are_rejected_without_lowering() {
    for bytes in [
        &[0x66, 0xe8, 0x34, 0x12][..],
        &[0x66, 0xe9, 0x34, 0x12][..],
        &[0x66, 0x0f, 0x84, 0x34, 0x12][..],
        &[0x66, 0x50][..],
        &[0x66, 0x58][..],
        &[0x66, 0xc3][..],
    ] {
        let space = code_space(0x1000, bytes);
        assert!(matches!(
            decode_one(&space, GuestAddress(0x1000)),
            Err(DecodeError::Unsupported(_))
        ));
    }
}

#[test]
fn segment_stack_flags_bulk_stack_and_loop_forms_are_outside_profile() {
    for bytes in [
        &[0x06][..],
        &[0x0e][..],
        &[0x16][..],
        &[0x1e][..],
        &[0x0f, 0xa0][..],
        &[0x0f, 0xa8][..],
        &[0x07][..],
        &[0x17][..],
        &[0x1f][..],
        &[0x0f, 0xa1][..],
        &[0x0f, 0xa9][..],
        &[0x9c][..],
        &[0x9d][..],
        &[0x60][..],
        &[0x61][..],
        &[0xe0, 0xfe][..],
        &[0xe1, 0xfe][..],
        &[0xe2, 0xfe][..],
        &[0xe3, 0xfe][..],
    ] {
        let space = code_space(0x1000, bytes);
        assert!(matches!(
            decode_one(&space, GuestAddress(0x1000)),
            Err(DecodeError::Unsupported(_))
        ));
    }
}
