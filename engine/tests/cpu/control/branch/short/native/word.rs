use super::support_fetch as fetch;
use ring3_engine::{
    cpu::{
        UnsupportedFeature,
        dbt::{BlockSpec, CompileLimits, compile_entry_region, compile_region},
        x86::{
            decode::{DecodeError, decode_one},
            ir::{BranchTarget, Condition, CountBranchKind, Operation},
        },
    },
    memory::{Access, AddressSpace, FaultReason, GuestAddress, PageRange, Permissions},
};

const CONDITIONS: [(u8, Condition); 16] = [
    (0x70, Condition::Overflow),
    (0x71, Condition::NotOverflow),
    (0x72, Condition::Below),
    (0x73, Condition::AboveOrEqual),
    (0x74, Condition::Equal),
    (0x75, Condition::NotEqual),
    (0x76, Condition::BelowOrEqual),
    (0x77, Condition::Above),
    (0x78, Condition::Sign),
    (0x79, Condition::NotSign),
    (0x7a, Condition::Parity),
    (0x7b, Condition::NotParity),
    (0x7c, Condition::Less),
    (0x7d, Condition::GreaterOrEqual),
    (0x7e, Condition::LessOrEqual),
    (0x7f, Condition::Greater),
];

const COUNTERS: [(u8, CountBranchKind); 4] = [
    (0xe0, CountBranchKind::LoopNotEqual),
    (0xe1, CountBranchKind::LoopEqual),
    (0xe2, CountBranchKind::Loop),
    (0xe3, CountBranchKind::EcxZero),
];

fn code(pc: u32, bytes: &[u8]) -> AddressSpace {
    let pages = (u64::from(pc & 0xfff) + bytes.len() as u64).div_ceil(4096) as u32;
    let mut memory = AddressSpace::new(pages).unwrap();
    let range = PageRange::new(GuestAddress(pc & !0xfff), pages).unwrap();
    memory.map_zeroed(range, Permissions::ALL).unwrap();
    memory.write(GuestAddress(pc), bytes).unwrap();
    memory.protect(range, Permissions::EXECUTE).unwrap();
    memory
}

#[test]
fn conditions_keep_truncated_taken_target_and_full_fallthrough() {
    for (opcode, condition) in CONDITIONS {
        for (displacement, target) in [(0x80, 0xff80), (0xfe, 0xfffe), (0, 0), (0x7f, 0x7f)] {
            let pc = 0x1234_fffd;
            let memory = code(pc, &[0x66, opcode, displacement]);
            let instruction = decode_one(&memory, GuestAddress(pc))
                .expect("operand16 short condition must decode in the fixed d32 code profile");
            assert_eq!(
                instruction.operation(),
                &Operation::ConditionalJump {
                    condition,
                    target: GuestAddress(target),
                }
            );
            assert_eq!(instruction.length(), 3);
            // only the taken target is truncated; the false path retains the full next pc.
            assert_eq!(instruction.next_pc(), GuestAddress(0x1235_0000));
            assert!(memory.is_code_current(instruction.code_snapshot()));
            assert!(
                memory
                    .resolve(GuestAddress(target), Access::Execute)
                    .is_err()
            );
        }
    }
}

#[test]
fn jump_truncates_signed_target_without_truncating_source_snapshot() {
    for pc in [0x1234_fffd, 0xffff_fffd] {
        for (displacement, target) in [(0x80, 0xff80), (0xfe, 0xfffe), (0, 0), (0x7f, 0x7f)] {
            let memory = code(pc, &[0x66, 0xeb, displacement]);
            let instruction = decode_one(&memory, GuestAddress(pc))
                .expect("operand16 short jump must decode without fetching its target");
            assert_eq!(
                instruction.operation(),
                &Operation::Jump {
                    target: BranchTarget::Direct(GuestAddress(target)),
                }
            );
            assert_eq!(instruction.pc(), GuestAddress(pc));
            assert_eq!(instruction.length(), 3);
            assert_eq!(instruction.next_pc(), GuestAddress(pc.wrapping_add(3)));
            assert!(memory.is_code_current(instruction.code_snapshot()));
            assert!(
                memory
                    .resolve(GuestAddress(target), Access::Execute)
                    .is_err()
            );
        }
    }
}

#[test]
fn counter_address_width_is_independent_of_short_target_width() {
    for (opcode, kind) in COUNTERS {
        for word_count in [false, true] {
            for displacement in [0x80_u8, 0xfe, 0, 0x7f] {
                let bytes = if word_count {
                    vec![0x66, 0x67, opcode, displacement]
                } else {
                    vec![0x66, opcode, displacement]
                };
                let pc = 0x1235_0000 - bytes.len() as u32;
                let target = u32::from(displacement as i8 as u16);
                let memory = code(pc, &bytes);
                let instruction = decode_one(&memory, GuestAddress(pc))
                    .expect("operand16 counter branch must preserve independent address width");
                let expected = if word_count {
                    Operation::CountBranchWord {
                        kind,
                        target: GuestAddress(target),
                    }
                } else {
                    Operation::CountBranch {
                        kind,
                        target: GuestAddress(target),
                    }
                };
                assert_eq!(instruction.operation(), &expected);
                assert_eq!(instruction.length() as usize, bytes.len());
                assert_eq!(instruction.next_pc(), GuestAddress(0x1235_0000));
                assert!(memory.is_code_current(instruction.code_snapshot()));
                if word_count {
                    let reversed = code(pc, &[0x67, 0x66, opcode, displacement]);
                    assert_eq!(
                        decode_one(&reversed, GuestAddress(pc)).unwrap().operation(),
                        &expected
                    );
                }
            }
        }
    }
}

#[test]
fn all_short_word_forms_end_explicit_and_discovered_blocks_at_source() {
    let mut forms: Vec<Vec<u8>> = CONDITIONS
        .iter()
        .map(|(opcode, _)| vec![0x66, *opcode, 0])
        .collect();
    forms.push(vec![0x66, 0xeb, 0]);
    for (opcode, _) in COUNTERS {
        forms.push(vec![0x66, opcode, 0]);
        forms.push(vec![0x66, 0x67, opcode, 0]);
    }
    assert_eq!(forms.len(), 25);
    for bytes in forms {
        let pc = 0x1235_0000 - bytes.len() as u32;
        // the malformed tail must remain outside both compilation extents.
        let source: Vec<_> = bytes.iter().copied().chain([0xf0, 0x90]).collect();
        let memory = code(pc, &source);
        let explicit = compile_region(
            &memory,
            &[BlockSpec {
                entry: GuestAddress(pc),
                byte_length: bytes.len() as u32,
            }],
            CompileLimits::default(),
        )
        .expect("explicit operand16 short branch extent must compile");
        let discovered =
            compile_entry_region(&memory, &[GuestAddress(pc)], CompileLimits::default())
                .expect("entry discovery must stop at the operand16 short branch");
        assert_eq!(explicit.metadata().blocks, 1);
        assert_eq!(explicit.metadata().instructions, 1);
        assert_eq!(explicit.metadata(), discovered.metadata());
        assert_eq!(
            explicit.wasm_bytes(&memory).unwrap(),
            discovered.wasm_bytes(&memory).unwrap()
        );
    }
}

#[test]
fn incomplete_displacement_faults_before_decode_and_repairs_exactly() {
    let pc = 0x1ffe;
    let mut memory = code(pc, &[0x66, 0x74]);
    fetch::assert_error(
        decode_one(&memory, GuestAddress(pc)).unwrap_err(),
        pc,
        0x2000,
        FaultReason::Unmapped,
        3,
    );
    let second = PageRange::new(GuestAddress(0x2000), 1).unwrap();
    // create capacity independently because the first mapping deliberately held one page.
    let mut repaired = AddressSpace::new(2).unwrap();
    let first = PageRange::new(GuestAddress(0x1000), 1).unwrap();
    repaired.map_zeroed(first, Permissions::ALL).unwrap();
    repaired.write(GuestAddress(pc), &[0x66, 0x74]).unwrap();
    repaired.protect(first, Permissions::EXECUTE).unwrap();
    repaired.map_zeroed(second, Permissions::READ).unwrap();
    fetch::assert_error(
        decode_one(&repaired, GuestAddress(pc)).unwrap_err(),
        pc,
        0x2000,
        FaultReason::Permission,
        3,
    );
    repaired.protect(second, Permissions::EXECUTE).unwrap();
    let instruction = decode_one(&repaired, GuestAddress(pc))
        .expect("fetch repair must admit exactly the complete operand16 short branch");
    assert_eq!(instruction.length(), 3);
    assert_eq!(instruction.next_pc(), GuestAddress(0x2001));
    assert_eq!(
        instruction.operation(),
        &Operation::ConditionalJump {
            condition: Condition::Equal,
            target: GuestAddress(0x2001),
        }
    );
    assert!(repaired.is_code_current(instruction.code_snapshot()));
    memory.protect(first, Permissions::READ).unwrap();
    fetch::assert_error(
        decode_one(&memory, GuestAddress(pc)).unwrap_err(),
        pc,
        0x1ffe,
        FaultReason::Permission,
        1,
    );
}

#[test]
fn existing_short_dword_and_invalid_prefix_controls_remain_exact() {
    for (opcode, condition) in CONDITIONS {
        let memory = code(0x1234_fffe, &[opcode, 0]);
        assert_eq!(
            decode_one(&memory, GuestAddress(0x1234_fffe))
                .unwrap()
                .operation(),
            &Operation::ConditionalJump {
                condition,
                target: GuestAddress(0x1235_0000),
            }
        );
    }
    for opcode in [0x74, 0xeb, 0xe0, 0xe1, 0xe2, 0xe3] {
        for bytes in [
            vec![0x66, 0x66, opcode, 0],
            vec![0xf2, 0x66, opcode, 0],
            vec![0xf3, 0x66, opcode, 0],
        ] {
            if bytes[0] == 0xf2 && opcode == 0x74 {
                super::support_word_prefix::admitted(&bytes);
                continue;
            }
            crate::support_fetch::assert_decode_contract(
                decode_one(&code(0x1000, &bytes), GuestAddress(0x1000)).unwrap_err(),
                DecodeError::Unsupported(UnsupportedFeature::Opcode),
            );
        }
        crate::support_fetch::assert_decode_contract(
            decode_one(
                &code(0x1000, &[0xf0, 0x66, opcode, 0]),
                GuestAddress(0x1000),
            )
            .unwrap_err(),
            DecodeError::InvalidEncoding,
        );
    }
}
