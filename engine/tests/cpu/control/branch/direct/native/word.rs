use super::support_word_prefix as word_prefix;
use ring3_engine::{
    abi::arena::TRANSFER_OFFSET,
    cpu::{
        UnsupportedFeature,
        dbt::{
            BlockSpec, CompileError, CompileLimits, InstructionError, RegistryError,
            compile_entry_region, compile_region, prepare_entry_region, prepare_region,
        },
        x86::{
            Register32,
            decode::{DecodeError, decode_one},
            ir::{BranchTarget, Condition, CountBranchKind, Location32, Operation, Value32},
        },
    },
    memory::{
        Access, AddressSpace, FaultReason, GuestAddress, MemoryFault, PageRange, Permissions,
    },
    process::{EngineInstance, HostError},
};

const CODE: u32 = 0x1000;
const KEY: u64 = 0x574a_554d_5031_3630;
const BRANCH_FORMS: [&[u8]; 2] = [&[0x66, 0xe9, 0, 0], &[0x66, 0x0f, 0x84, 0, 0]];

fn range(pc: u32, pages: u32) -> PageRange {
    PageRange::new(GuestAddress(pc), pages).unwrap()
}

fn spec(pc: u32, length: u32) -> BlockSpec {
    BlockSpec {
        entry: GuestAddress(pc),
        byte_length: length,
    }
}

fn code_space(pc: u32, bytes: &[u8]) -> AddressSpace {
    let pages = (u64::from(pc & 0xfff) + bytes.len() as u64).div_ceil(4096) as u32;
    let mut memory = AddressSpace::new(pages).unwrap();
    let range = range(pc & !0xfff, pages);
    memory.map_zeroed(range, Permissions::ALL).unwrap();
    memory.write(GuestAddress(pc), bytes).unwrap();
    memory.protect(range, Permissions::EXECUTE).unwrap();
    memory
}

fn code(bytes: &[u8]) -> EngineInstance {
    let mut engine = EngineInstance::new(1, KEY).unwrap();
    engine.map(CODE, 1, 7).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(bytes);
    engine.upload(CODE, bytes.len() as u32).unwrap();
    engine.protect(CODE, 1, 4).unwrap();
    engine
}

fn describe(engine: &mut EngineInstance, blocks: &[(u32, u32)], entries: bool) {
    let descriptor = &mut engine.arena_mut().unwrap()[TRANSFER_OFFSET..];
    for (index, &(pc, length)) in blocks.iter().enumerate() {
        let offset = index * if entries { 4 } else { 8 };
        descriptor[offset..offset + 4].copy_from_slice(&pc.to_le_bytes());
        if !entries {
            descriptor[offset + 4..offset + 8].copy_from_slice(&length.to_le_bytes());
        }
    }
}

fn bound_compile(
    engine: &mut EngineInstance,
    blocks: &[(u32, u32)],
    resident: bool,
    entries: bool,
) -> Result<u64, HostError> {
    describe(engine, blocks, entries);
    match (resident, entries) {
        (false, false) => engine.compile(blocks.len() as u32).map(u64::from),
        (false, true) => engine
            .compile_entries(blocks.len() as u32, 0)
            .map(u64::from),
        (true, false) => engine
            .compile_resident(blocks.len() as u32)
            .map(|id| id.get()),
        (true, true) => engine
            .compile_resident_entries(blocks.len() as u32, 0)
            .map(|id| id.get()),
    }
}

fn assert_bound(bytes: &[u8], blocks: &[(u32, u32)]) {
    for resident in [false, true] {
        for entries in [false, true] {
            let mut engine = code(bytes);
            describe(&mut engine, blocks, entries);
            let before = engine.arena().to_vec();
            let id = bound_compile(&mut engine, blocks, resident, entries)
                .expect("WORD direct branch must compile in every bound profile");
            let bytes = if resident {
                engine.resident_bytes(id).unwrap()
            } else {
                engine.artifact_bytes().unwrap()
            };
            assert_eq!(&bytes[..8], b"\0asm\x01\0\0\0");
            assert_eq!(engine.arena(), before);
            assert_eq!(engine.memory().unwrap().mapped_pages(), 1);
            assert!(
                engine
                    .memory()
                    .unwrap()
                    .resolve(GuestAddress(0x8000), Access::Execute)
                    .is_err()
            );
        }
    }
}

#[test]
fn word_jump_decodes_high_pc_with_truncated_target() {
    let memory = code_space(0x1234_fffc, &[0x66, 0xe9, 0, 0]);
    let instruction = decode_one(&memory, GuestAddress(0x1234_fffc))
        .expect("exact WORD relative JMP must decode");
    assert_eq!(instruction.length(), 4);
    assert_eq!(instruction.next_pc(), GuestAddress(0x1235_0000));
    assert_eq!(
        instruction.operation(),
        &Operation::Jump {
            target: BranchTarget::Direct(GuestAddress(0)),
        }
    );
    assert!(memory.is_code_current(instruction.code_snapshot()));
}

#[test]
fn word_jump_compiles_standalone_explicit_and_entry() {
    let memory = code_space(CODE, &[0x66, 0xe9, 0xfc, 0x6f]);
    let explicit = compile_region(
        &memory,
        &[BlockSpec {
            entry: GuestAddress(CODE),
            byte_length: 4,
        }],
        CompileLimits::default(),
    )
    .expect("WORD relative JMP must compile standalone");
    let entry = compile_entry_region(&memory, &[GuestAddress(CODE)], CompileLimits::default())
        .expect("WORD relative JMP must compile from its standalone entry");
    assert_eq!(explicit.metadata(), entry.metadata());
    assert_eq!(
        explicit.wasm_bytes(&memory).unwrap(),
        entry.wasm_bytes(&memory).unwrap()
    );
}

#[test]
fn word_jump_compiles_four_bound_profiles_without_target_memory() {
    assert_bound(&[0x66, 0xe9, 0xfc, 0x6f], &[(CODE, 4)]);
}

#[test]
fn word_jump_signed_rel16_targets_and_full_next_pc_have_independent_anchors() {
    for (pc, displacement, target, next) in [
        (0x1234_fffc, 0_i16, 0, 0x1235_0000),
        (0x1234_0000, -5, 0xffff, 0x1234_0004),
        (0xffff_fffc, 1, 1, 0),
    ] {
        let [lo, hi] = displacement.to_le_bytes();
        let decoded = decode_one(&code_space(pc, &[0x66, 0xe9, lo, hi]), GuestAddress(pc)).unwrap();
        assert_eq!(
            decoded.operation(),
            &Operation::Jump {
                target: BranchTarget::Direct(GuestAddress(target))
            }
        );
        assert_eq!(
            (decoded.length(), decoded.next_pc()),
            (4, GuestAddress(next))
        );
    }
    for pc in [CODE, 0xfffc, 0xffff, 0x1234_fffc, 0xffff_fffc] {
        for displacement in [0_i16, 1, -1, i16::MIN, i16::MAX, 0x6766, 0xf0f3_u16 as i16] {
            let [lo, hi] = displacement.to_le_bytes();
            let memory = code_space(pc, &[0x66, 0xe9, lo, hi]);
            let decoded = decode_one(&memory, GuestAddress(pc)).unwrap();
            let target = (i64::from(pc) + 4 + i64::from(displacement)).rem_euclid(65536) as u32;
            assert_eq!(
                decoded.operation(),
                &Operation::Jump {
                    target: BranchTarget::Direct(GuestAddress(target))
                }
            );
            assert_eq!(
                (decoded.length(), decoded.next_pc()),
                (4, GuestAddress(pc.wrapping_add(4)))
            );
            assert!(memory.is_code_current(decoded.code_snapshot()));
        }
    }
}

#[test]
fn word_jump_immediate_prefix_bytes_are_payload_and_old_jump_forms_stay_exact() {
    for byte in [0x66, 0x67, 0xf0, 0xf2, 0xf3] {
        let bytes = [0xb8, 0x66, 0xe9, 0xf0, 0xf3, 0x66, 0xe9, byte, byte];
        let memory = code_space(CODE, &bytes);
        let first = decode_one(&memory, GuestAddress(CODE)).unwrap();
        assert_eq!(
            first.operation(),
            &Operation::Move {
                destination: Location32::Register(Register32::Eax),
                source: Value32::Immediate(0xf3f0_e966)
            }
        );
        let jump = decode_one(&memory, first.next_pc()).unwrap();
        let displacement = i16::from_le_bytes([byte, byte]);
        assert_eq!(
            jump.operation(),
            &Operation::Jump {
                target: BranchTarget::Direct(GuestAddress(
                    (i64::from(CODE) + 9 + i64::from(displacement)).rem_euclid(65536) as u32
                ))
            }
        );
        assert_eq!((jump.length(), jump.next_pc()), (4, GuestAddress(CODE + 9)));
    }
    for (bytes, target) in [
        (&[0xeb, 0xfe][..], CODE),
        (&[0xe9, 0xfb, 0xff, 0xff, 0xff][..], CODE),
        (&[0xe9, 0, 0, 0, 0][..], CODE + 5),
    ] {
        assert_eq!(
            decode_one(&code_space(CODE, bytes), GuestAddress(CODE))
                .unwrap()
                .operation(),
            &Operation::Jump {
                target: BranchTarget::Direct(GuestAddress(target))
            }
        );
    }
}

#[test]
fn word_jump_prefix_aliases_and_other_word_jump_families_remain_closed() {
    let far = [0x66, 0xea, 0, 0, 0, 0];
    assert_eq!(
        decode_one(&code_space(CODE, &far), GuestAddress(CODE))
            .unwrap()
            .operation(),
        &Operation::FarJump {
            width: ring3_engine::cpu::x86::FarWidth::Word,
            target: ring3_engine::cpu::x86::ir::FarPointer::Immediate {
                offset: 0,
                selector: 0
            },
        },
    );

    let opcode = DecodeError::Unsupported(UnsupportedFeature::Opcode);
    for prefix in [
        0x66, 0x67, 0xf0, 0xf2, 0xf3, 0x26, 0x2e, 0x36, 0x3e, 0x64, 0x65,
    ] {
        let expected = match prefix {
            0xf0 => DecodeError::InvalidEncoding,
            0x26 | 0x2e | 0x36 | 0x3e | 0x64 | 0x65 => {
                DecodeError::Unsupported(UnsupportedFeature::Segment)
            }
            _ => opcode,
        };
        for bytes in [[prefix, 0x66, 0xe9, 0, 0], [0x66, prefix, 0xe9, 0, 0]] {
            if matches!(prefix, 0x67 | 0x26 | 0x2e | 0x36 | 0x3e | 0x64 | 0x65) {
                word_prefix::admitted(&bytes);
                continue;
            }
            assert_eq!(
                decode_one(&code_space(CODE, &bytes), GuestAddress(CODE)).err(),
                Some(expected),
                "{bytes:02x?}"
            );
        }
    }
    for (bytes, expected) in [
        (
            [0x66, 0xeb, 0],
            Operation::Jump {
                target: BranchTarget::Direct(GuestAddress(CODE + 3)),
            },
        ),
        (
            [0x66, 0x74, 0],
            Operation::ConditionalJump {
                condition: Condition::Equal,
                target: GuestAddress(CODE + 3),
            },
        ),
    ] {
        let memory = code_space(CODE, &bytes);
        let instruction = decode_one(&memory, GuestAddress(CODE)).unwrap();
        assert_eq!(instruction.operation(), &expected, "{bytes:02x?}");
        assert_eq!(
            (instruction.length(), instruction.next_pc()),
            (3, GuestAddress(CODE + 3))
        );
        assert!(memory.is_code_current(instruction.code_snapshot()));
    }
    for bytes in [
        &[0x66, 0x66, 0x0f, 0x84, 0, 0][..],
        &[0x66, 0x66, 0xff, 0xe0],
        &[0x66, 0x66, 0xff, 0x23],
    ] {
        assert_eq!(
            decode_one(&code_space(CODE, bytes), GuestAddress(CODE)).err(),
            Some(opcode),
            "{bytes:02x?}"
        );
    }
}

#[test]
fn word_branches_terminate_all_six_profiles_before_trailing_decode() {
    for form in BRANCH_FORMS {
        let bytes = [vec![0x90], form.to_vec(), vec![0xf0, 0x90]].concat();
        let consumed = form.len() as u32 + 1;
        let memory = code_space(CODE, &bytes);
        let error = CompileError::Instruction {
            pc: GuestAddress(CODE + 1),
            cause: InstructionError::InvalidBlockEnd,
        };
        assert_eq!(
            prepare_region(
                &memory,
                &[spec(CODE, bytes.len() as u32)],
                CompileLimits::default()
            )
            .err(),
            Some(error)
        );
        assert_eq!(
            compile_region(
                &memory,
                &[spec(CODE, bytes.len() as u32)],
                CompileLimits::default()
            )
            .err(),
            Some(error)
        );
        let explicit =
            compile_region(&memory, &[spec(CODE, consumed)], CompileLimits::default()).unwrap();
        let entry =
            compile_entry_region(&memory, &[GuestAddress(CODE)], CompileLimits::default()).unwrap();
        assert_eq!(explicit.metadata().instructions, 2);
        assert_eq!(
            explicit.wasm_bytes(&memory).unwrap(),
            entry.wasm_bytes(&memory).unwrap()
        );
        for resident in [false, true] {
            let mut engine = code(&bytes);
            assert_eq!(
                bound_compile(&mut engine, &[(CODE, bytes.len() as u32)], resident, false),
                Err(if resident {
                    HostError::Resident(RegistryError::Compile(error))
                } else {
                    HostError::Compile(error)
                })
            );
        }
        assert_bound(&bytes, &[(CODE, consumed)]);
    }
}

fn fetch_error(pc: u32, address: u32, reason: FaultReason, length: u32) -> DecodeError {
    DecodeError::MemoryFault {
        pc: GuestAddress(pc),
        fault: MemoryFault {
            address: GuestAddress(address),
            access: Access::Execute,
            reason,
        },
        length,
    }
}

#[test]
fn word_branches_fetch_four_or_five_source_bytes_without_wrapping() {
    for bytes in BRANCH_FORMS {
        let length = bytes.len() as u32;
        for pc in [0x1fff, u32::MAX - length + 1] {
            let memory = code_space(pc, bytes);
            let decoded = decode_one(&memory, GuestAddress(pc)).unwrap();
            assert_eq!(
                (decoded.length(), decoded.next_pc()),
                (length as u8, GuestAddress(pc.wrapping_add(length)))
            );
            assert_eq!(
                prepare_entry_region(&memory, &[GuestAddress(pc)], CompileLimits::default())
                    .unwrap()
                    .instruction_count(),
                1
            );
        }
        for cut in 1..bytes.len() {
            let pc = 0x2000 - cut as u32;
            assert_eq!(
                decode_one(&code_space(pc, &bytes[..cut]), GuestAddress(pc)).err(),
                Some(fetch_error(
                    pc,
                    0x2000,
                    FaultReason::Unmapped,
                    cut as u32 + 1
                ))
            );
            let top = u32::MAX - cut as u32 + 1;
            let mut memory = AddressSpace::new(2).unwrap();
            memory
                .map_zeroed(range(0xffff_f000, 1), Permissions::ALL)
                .unwrap();
            memory.map_zeroed(range(0, 1), Permissions::ALL).unwrap();
            memory.write(GuestAddress(top), &bytes[..cut]).unwrap();
            memory.write(GuestAddress(0), &bytes[cut..]).unwrap();
            memory
                .protect(range(0xffff_f000, 1), Permissions::EXECUTE)
                .unwrap();
            memory.protect(range(0, 1), Permissions::EXECUTE).unwrap();
            assert_eq!(
                decode_one(&memory, GuestAddress(top)).err(),
                Some(fetch_error(
                    top,
                    top,
                    FaultReason::AddressOverflow,
                    cut as u32 + 1
                ))
            );
        }
        for (pc, denied, attempted) in [(CODE, CODE, 1), (0x2001 - length, 0x2000, length)] {
            let mut memory = code_space(pc, bytes);
            memory.protect(range(denied, 1), Permissions::READ).unwrap();
            assert_eq!(
                decode_one(&memory, GuestAddress(pc)).err(),
                Some(fetch_error(pc, denied, FaultReason::Permission, attempted))
            );
        }
    }
}

#[test]
fn word_branches_currentness_tracks_consumed_code_and_ignores_target_permissions() {
    for form in [
        &[0x66, 0xe9, 0xfd, 0x5f][..],
        &[0x66, 0x0f, 0x84, 0xfc, 0x5f],
    ] {
        let mut memory = AddressSpace::new(3).unwrap();
        memory.map_zeroed(range(CODE, 2), Permissions::ALL).unwrap();
        memory.write(GuestAddress(0x1fff), form).unwrap();
        assert_eq!(
            decode_one(&memory, GuestAddress(0x1fff))
                .unwrap()
                .operation(),
            &if form.len() == 4 {
                Operation::Jump {
                    target: BranchTarget::Direct(GuestAddress(0x8000)),
                }
            } else {
                Operation::ConditionalJump {
                    condition: Condition::Equal,
                    target: GuestAddress(0x8000),
                }
            }
        );
        let prepared =
            prepare_entry_region(&memory, &[GuestAddress(0x1fff)], CompileLimits::default())
                .unwrap();
        let compiled = compile_region(
            &memory,
            &[spec(0x1fff, form.len() as u32)],
            CompileLimits::default(),
        )
        .unwrap();
        memory
            .map_zeroed(range(0x8000, 1), Permissions::ALL)
            .unwrap();
        memory.write(GuestAddress(0x8000), &[0xf0, 0x90]).unwrap();
        for permission in [Permissions::READ, Permissions::NONE] {
            memory.protect(range(0x8000, 1), permission).unwrap();
            assert!(prepared.is_current(&memory));
            compiled.wasm_bytes(&memory).unwrap();
        }
        memory.unmap(range(0x8000, 1)).unwrap();
        assert!(prepared.is_current(&memory));
        compiled.wasm_bytes(&memory).unwrap();
        memory.write(GuestAddress(0x1fff), &[0x66]).unwrap();
        assert!(!prepared.is_current(&memory));
        assert!(compiled.wasm_bytes(&memory).is_err());
    }
}

#[test]
fn word_branches_with_63_nops_fit_exact_64_instruction_units_in_six_profiles() {
    for form in BRANCH_FORMS {
        let mut bytes = vec![0x90; 63];
        bytes.extend(form);
        let memory = code_space(CODE, &bytes);
        let explicit = compile_region(
            &memory,
            &[spec(CODE, bytes.len() as u32)],
            CompileLimits::default(),
        )
        .unwrap();
        let entry =
            compile_entry_region(&memory, &[GuestAddress(CODE)], CompileLimits::default()).unwrap();
        assert_eq!(explicit.metadata().instructions, 64);
        assert_eq!(entry.metadata().instructions, 64);
        assert_bound(&bytes, &[(CODE, bytes.len() as u32)]);
    }
}

#[test]
fn word_branches_late_refusal_preserves_dirty_arena_and_both_existing_owners() {
    let mut bytes = vec![0x90; 258];
    for offset in [0, 16, 32] {
        bytes[offset..offset + 4].copy_from_slice(&[0x66, 0xe9, 0, 0]);
    }
    bytes[16..21].copy_from_slice(&[0x66, 0x0f, 0x84, 0, 0]);
    bytes[49..51].copy_from_slice(&[0xf0, 0x90]);
    bytes[256..258].copy_from_slice(&[0xeb, 0]);
    let mut engine = code(&bytes);
    let replacement = bound_compile(&mut engine, &[(CODE + 256, 2)], false, false).unwrap() as u32;
    let resident = bound_compile(&mut engine, &[(CODE + 256, 2)], true, false).unwrap();
    let replacement_bytes = engine.artifact_bytes().unwrap().to_vec();
    let replacement_pointer = engine.artifact_bytes().unwrap().as_ptr();
    let resident_bytes = engine.resident_bytes(resident).unwrap().to_vec();
    let resident_pointer = engine.resident_bytes(resident).unwrap().as_ptr();
    let blocks = [(CODE, 4), (CODE + 16, 5), (CODE + 32, 4), (CODE + 48, 3)];
    let error = CompileError::Instruction {
        pc: GuestAddress(CODE + 49),
        cause: InstructionError::Decode(DecodeError::InvalidEncoding),
    };
    for is_resident in [false, true] {
        for entries in [false, true] {
            for (index, byte) in engine.arena_mut().unwrap().iter_mut().enumerate() {
                *byte = (index.wrapping_mul(37).wrapping_add(11) & 0xff) as u8;
            }
            describe(&mut engine, &blocks, entries);
            let before = engine.arena().to_vec();
            assert_eq!(before.len(), ring3_engine::abi::arena::ARENA_SIZE);
            assert_eq!(
                bound_compile(&mut engine, &blocks, is_resident, entries),
                Err(if is_resident {
                    HostError::Resident(RegistryError::Compile(error))
                } else {
                    HostError::Compile(error)
                })
            );
            assert_eq!(engine.arena(), before);
            assert_eq!(engine.artifact_bytes().unwrap(), replacement_bytes);
            assert_eq!(
                engine.artifact_bytes().unwrap().as_ptr(),
                replacement_pointer
            );
            assert_eq!(engine.resident_bytes(resident).unwrap(), resident_bytes);
            assert_eq!(
                engine.resident_bytes(resident).unwrap().as_ptr(),
                resident_pointer
            );
            engine.guard(KEY, replacement).unwrap();
            engine.guard_resident(KEY, resident).unwrap();
        }
    }
}

#[test]
fn word_jcc_decodes_high_pc_with_truncated_taken_target() {
    let pc = 0x1234_fffb;
    let memory = code_space(pc, &[0x66, 0x0f, 0x84, 0, 0]);
    let instruction =
        decode_one(&memory, GuestAddress(pc)).expect("exact WORD near Jcc must decode");
    assert_eq!(
        (instruction.length(), instruction.next_pc()),
        (5, GuestAddress(0x1235_0000))
    );
    assert_eq!(
        instruction.operation(),
        &Operation::ConditionalJump {
            condition: Condition::Equal,
            target: GuestAddress(0),
        }
    );
}

#[test]
fn word_jcc_compiles_standalone_explicit_and_entry() {
    let memory = code_space(CODE, &[0x66, 0x0f, 0x84, 0xfb, 0x6f]);
    let explicit = compile_region(&memory, &[spec(CODE, 5)], CompileLimits::default())
        .expect("WORD near Jcc must compile standalone");
    let entry = compile_entry_region(&memory, &[GuestAddress(CODE)], CompileLimits::default())
        .expect("WORD near Jcc must compile from its standalone entry");
    assert_eq!(explicit.metadata().instructions, 1);
    assert_eq!(explicit.metadata(), entry.metadata());
    assert_eq!(
        explicit.wasm_bytes(&memory).unwrap(),
        entry.wasm_bytes(&memory).unwrap()
    );
}

#[test]
fn word_jcc_compiles_four_bound_profiles_without_successor_memory() {
    assert_bound(&[0x66, 0x0f, 0x84, 0xfb, 0x6f], &[(CODE, 5)]);
}

const CONDITIONS: [Condition; 16] = [
    Condition::Overflow,
    Condition::NotOverflow,
    Condition::Below,
    Condition::AboveOrEqual,
    Condition::Equal,
    Condition::NotEqual,
    Condition::BelowOrEqual,
    Condition::Above,
    Condition::Sign,
    Condition::NotSign,
    Condition::Parity,
    Condition::NotParity,
    Condition::Less,
    Condition::GreaterOrEqual,
    Condition::LessOrEqual,
    Condition::Greater,
];

#[test]
fn word_jcc_all_conditions_have_literal_taken_and_full_fallthrough_anchors() {
    for (index, condition) in CONDITIONS.into_iter().enumerate() {
        for (pc, displacement, taken, fallthrough) in [
            (0x1234_fffb, 0_i16, 0, 0x1235_0000),
            (0x1234_0000, -6, 0xffff, 0x1234_0005),
            (0xffff_fffb, 1, 1, 0),
        ] {
            let [lo, hi] = displacement.to_le_bytes();
            let memory = code_space(pc, &[0x66, 0x0f, 0x80 + index as u8, lo, hi]);
            let decoded = decode_one(&memory, GuestAddress(pc)).unwrap();
            assert_eq!(
                decoded.operation(),
                &Operation::ConditionalJump {
                    condition,
                    target: GuestAddress(taken)
                }
            );
            assert_eq!(
                (decoded.length(), decoded.next_pc()),
                (5, GuestAddress(fallthrough))
            );
            assert!(memory.is_code_current(decoded.code_snapshot()));
        }
    }
}

#[test]
fn word_jcc_two_eight_block_banks_compile_all_conditions_in_six_profiles() {
    for bank in 0..2 {
        let mut bytes = vec![0xcc; 128];
        let mut blocks = Vec::new();
        for index in 0..8 {
            bytes[index * 16..index * 16 + 5].copy_from_slice(&[
                0x66,
                0x0f,
                0x80 + (bank * 8 + index) as u8,
                0xfb,
                0x6f,
            ]);
            blocks.push((CODE + index as u32 * 16, 5));
        }
        let memory = code_space(CODE, &bytes);
        let specs: Vec<_> = blocks
            .iter()
            .map(|&(pc, length)| spec(pc, length))
            .collect();
        let entries: Vec<_> = blocks.iter().map(|&(pc, _)| GuestAddress(pc)).collect();
        let explicit = compile_region(&memory, &specs, CompileLimits::default()).unwrap();
        let entry = compile_entry_region(&memory, &entries, CompileLimits::default()).unwrap();
        assert_eq!(
            (explicit.metadata().blocks, explicit.metadata().instructions),
            (8, 8)
        );
        assert_eq!(explicit.metadata(), entry.metadata());
        assert_eq!(
            explicit.wasm_bytes(&memory).unwrap(),
            entry.wasm_bytes(&memory).unwrap()
        );
        assert_bound(&bytes, &blocks);
    }
}

#[test]
fn word_jcc_strict_prefixes_short_count_neighbors_and_payloads_stay_distinct() {
    let opcode = DecodeError::Unsupported(UnsupportedFeature::Opcode);
    for (index, condition) in CONDITIONS.into_iter().enumerate() {
        let near = 0x80 + index as u8;
        for prefix in [
            0x66, 0x67, 0xf0, 0xf2, 0xf3, 0x26, 0x2e, 0x36, 0x3e, 0x64, 0x65,
        ] {
            let expected = match prefix {
                0xf0 => DecodeError::InvalidEncoding,
                0x26 | 0x2e | 0x36 | 0x3e | 0x64 | 0x65 => {
                    DecodeError::Unsupported(UnsupportedFeature::Segment)
                }
                _ => opcode,
            };
            for bytes in [
                [prefix, 0x66, 0x0f, near, 0, 0],
                [0x66, prefix, 0x0f, near, 0, 0],
            ] {
                if matches!(prefix, 0x67 | 0x26 | 0x2e | 0x36 | 0x3e | 0x64 | 0x65) {
                    word_prefix::admitted(&bytes);
                    continue;
                }
                assert_eq!(
                    decode_one(&code_space(CODE, &bytes), GuestAddress(CODE)).err(),
                    Some(expected),
                    "{bytes:02x?}"
                );
            }
        }
        for raw in [0x8000_u16, 0x7fff, 0x6666, 0x6767, 0xf0f0, 0xf2f2, 0xf3f3] {
            let [lo, hi] = raw.to_le_bytes();
            let memory = code_space(CODE, &[0x66, 0x0f, near, lo, hi]);
            let decoded = decode_one(&memory, GuestAddress(CODE)).unwrap();
            let target = (i64::from(CODE) + 5 + i64::from(raw as i16)).rem_euclid(65536) as u32;
            assert_eq!(
                decoded.operation(),
                &Operation::ConditionalJump {
                    condition,
                    target: GuestAddress(target)
                }
            );
            assert_eq!(
                (decoded.length(), decoded.next_pc()),
                (5, GuestAddress(CODE + 5))
            );
        }
        let short = 0x70 + index as u8;
        assert_eq!(
            decode_one(&code_space(CODE, &[0x66, short, 0]), GuestAddress(CODE))
                .unwrap()
                .operation(),
            &Operation::ConditionalJump {
                condition,
                target: GuestAddress(CODE + 3)
            }
        );
        for (bytes, target) in [
            (&[short, 0][..], CODE + 2),
            (&[0x0f, near, 0, 0, 0, 0][..], CODE + 6),
        ] {
            assert_eq!(
                decode_one(&code_space(CODE, bytes), GuestAddress(CODE))
                    .unwrap()
                    .operation(),
                &Operation::ConditionalJump {
                    condition,
                    target: GuestAddress(target)
                }
            );
        }
    }
    for (count, kind) in [
        (0xe0, CountBranchKind::LoopNotEqual),
        (0xe1, CountBranchKind::LoopEqual),
        (0xe2, CountBranchKind::Loop),
        (0xe3, CountBranchKind::EcxZero),
    ] {
        assert_eq!(
            decode_one(&code_space(CODE, &[0x66, count, 0]), GuestAddress(CODE))
                .unwrap()
                .operation(),
            &Operation::CountBranch {
                kind,
                target: GuestAddress(CODE + 3)
            }
        );
    }
}
