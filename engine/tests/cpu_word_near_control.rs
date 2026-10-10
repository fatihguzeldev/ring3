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
            ir::{BranchTarget, Location32, Operation, Value32},
        },
    },
    memory::{
        Access, AddressSpace, FaultReason, GuestAddress, MemoryFault, PageRange, Permissions,
    },
    process::{EngineInstance, HostError},
};

const CODE: u32 = 0x1000;
const KEY: u64 = 0x5743_414c_4c52_4554;

fn code(bytes: &[u8]) -> EngineInstance {
    let mut engine = EngineInstance::new(1, KEY).unwrap();
    engine.map(CODE, 1, 7).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(bytes);
    engine.upload(CODE, bytes.len() as u32).unwrap();
    engine.protect(CODE, 1, 4).unwrap();
    engine
}

fn decode_admission(bytes: &[u8]) {
    let engine = code(bytes);
    let before = engine.arena().to_vec();
    let decoded = decode_one(engine.memory().unwrap(), GuestAddress(CODE))
        .expect("WORD near control must decode without a mapped guest stack");
    assert_eq!(decoded.operation(), &expected_operation(CODE, bytes));
    assert_eq!(
        (decoded.length() as usize, decoded.next_pc()),
        (bytes.len(), GuestAddress(CODE + bytes.len() as u32))
    );
    assert!(
        engine
            .memory()
            .unwrap()
            .is_code_current(decoded.code_snapshot())
    );
    assert_eq!(engine.arena(), before);
    assert_eq!(engine.memory().unwrap().mapped_pages(), 1);
    assert!(
        engine
            .memory()
            .unwrap()
            .resolve(GuestAddress(0), Access::Read)
            .is_err()
    );
}

#[test]
fn word_direct_call_decodes_exact_rel16_form() {
    decode_admission(&[0x66, 0xe8, 0xfc, 0x6f]);
}

#[test]
fn word_near_return_decodes_exact_plain_form() {
    decode_admission(&[0x66, 0xc3]);
}

#[test]
fn word_near_return_decodes_exact_unsigned_adjustment_form() {
    decode_admission(&[0x66, 0xc2, 0x66, 0xf3]);
}

fn bound_admission(resident: bool, entries: bool) {
    let forms: [&[u8]; 3] = [
        &[0x66, 0xe8, 0xfc, 0x6f],
        &[0x66, 0xc3],
        &[0x66, 0xc2, 0x66, 0xf3],
    ];
    let mut bytes = vec![0xcc; 0x24];
    for (index, form) in forms.iter().enumerate() {
        bytes[index * 16..index * 16 + form.len()].copy_from_slice(form);
    }
    let blocks: Vec<_> = forms
        .iter()
        .enumerate()
        .map(|(index, form)| (CODE + index as u32 * 16, form.len() as u32))
        .collect();
    assert_bound_admission(&bytes, &blocks, resident, entries);
}

fn describe(engine: &mut EngineInstance, blocks: &[(u32, u32)], entries: bool) {
    let descriptor =
        &mut engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + blocks.len() * 8];
    for (index, &(pc, length)) in blocks.iter().enumerate() {
        let offset = index * if entries { 4 } else { 8 };
        descriptor[offset..offset + 4].copy_from_slice(&pc.to_le_bytes());
        if !entries {
            descriptor[offset + 4..offset + 8].copy_from_slice(&length.to_le_bytes());
        }
    }
}

fn compile(
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

fn assert_bound_admission(bytes: &[u8], blocks: &[(u32, u32)], resident: bool, entries: bool) {
    let mut engine = code(bytes);
    describe(&mut engine, blocks, entries);
    let before = engine.arena().to_vec();
    let id = compile(&mut engine, blocks, resident, entries)
        .expect("WORD direct CALL and near RET must compile bound without a mapped stack");
    let module = if resident {
        engine.resident_bytes(id).unwrap()
    } else {
        engine.artifact_bytes().unwrap()
    };
    assert_eq!(&module[..8], b"\0asm\x01\0\0\0");
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

#[test]
fn word_near_control_compiles_replacement_explicit() {
    bound_admission(false, false);
}

#[test]
fn word_near_control_compiles_replacement_entry() {
    bound_admission(false, true);
}

#[test]
fn word_near_control_compiles_resident_explicit() {
    bound_admission(true, false);
}

#[test]
fn word_near_control_compiles_resident_entry() {
    bound_admission(true, true);
}

const FORMS: [&[u8]; 3] = [
    &[0x66, 0xe8, 0xfc, 0x6f],
    &[0x66, 0xc3],
    &[0x66, 0xc2, 0x66, 0xf3],
];

fn code_space(pc: u32, bytes: &[u8]) -> AddressSpace {
    let pages = (u64::from(pc & 0xfff) + bytes.len() as u64).div_ceil(4096) as u32;
    let mut memory = AddressSpace::new(pages).unwrap();
    let range = PageRange::new(GuestAddress(pc & !0xfff), pages).unwrap();
    memory.map_zeroed(range, Permissions::ALL).unwrap();
    memory.write(GuestAddress(pc), bytes).unwrap();
    memory.protect(range, Permissions::EXECUTE).unwrap();
    memory
}

fn expected_operation(pc: u32, bytes: &[u8]) -> Operation {
    match bytes[1] {
        0xe8 => Operation::CallWord {
            target: GuestAddress(
                pc.wrapping_add(4)
                    .wrapping_add(i32::from(i16::from_le_bytes([bytes[2], bytes[3]])) as u32)
                    & 0xffff,
            ),
        },
        0xc3 => Operation::ReturnWord { stack_adjust: 0 },
        0xc2 => Operation::ReturnWord {
            stack_adjust: u16::from_le_bytes([bytes[2], bytes[3]]),
        },
        _ => unreachable!(),
    }
}

#[test]
fn word_call_rel16_targets_truncate_while_next_pc_keeps_full32_identity() {
    for (pc, displacement, target, next) in [
        (0x1234_fffc, 0_i16, 0, 0x1235_0000),
        (0x1234_0000, -5, 0xffff, 0x1234_0004),
        (0xffff_fffc, 1, 1, 0),
    ] {
        let bytes = [vec![0x66, 0xe8], displacement.to_le_bytes().to_vec()].concat();
        let decoded = decode_one(&code_space(pc, &bytes), GuestAddress(pc)).unwrap();
        assert_eq!(
            decoded.operation(),
            &Operation::CallWord {
                target: GuestAddress(target)
            }
        );
        assert_eq!(
            (decoded.length(), decoded.next_pc()),
            (4, GuestAddress(next))
        );
    }
    for pc in [CODE, 0xfffc, 0xffff, 0x1234_fffc, 0xffff_fffc] {
        for displacement in [0_i16, 1, -1, i16::MAX, i16::MIN, 0x6766, 0xf0f3_u16 as i16] {
            let bytes = [vec![0x66, 0xe8], displacement.to_le_bytes().to_vec()].concat();
            let memory = code_space(pc, &bytes);
            let decoded = decode_one(&memory, GuestAddress(pc)).unwrap();
            assert_eq!(decoded.operation(), &expected_operation(pc, &bytes));
            assert_eq!(
                (decoded.length(), decoded.next_pc()),
                (4, GuestAddress(pc.wrapping_add(4)))
            );
            assert!(memory.is_code_current(decoded.code_snapshot()));
        }
    }
}

#[test]
fn word_return_adjustment_is_unsigned_and_prefix_looking_immediates_are_payload() {
    for adjustment in [
        0_u16, 1, 0x7fff, 0x8000, 0xffff, 0x6666, 0x6767, 0xf0f0, 0xf2f2, 0xf3f3,
    ] {
        let bytes = [vec![0x66, 0xc2], adjustment.to_le_bytes().to_vec()].concat();
        let decoded = decode_one(&code_space(CODE, &bytes), GuestAddress(CODE)).unwrap();
        assert_eq!(
            decoded.operation(),
            &Operation::ReturnWord {
                stack_adjust: adjustment
            }
        );
        assert_eq!(
            (decoded.length(), decoded.next_pc()),
            (4, GuestAddress(CODE + 4))
        );
    }
    for form in FORMS {
        let payload = [0x66, 0xc3, 0xf0, 0xe8];
        let bytes = [vec![0xb8], payload.to_vec(), form.to_vec()].concat();
        let memory = code_space(CODE, &bytes);
        let first = decode_one(&memory, GuestAddress(CODE)).unwrap();
        assert_eq!(
            first.operation(),
            &Operation::Move {
                destination: Location32::Register(Register32::Eax),
                source: Value32::Immediate(u32::from_le_bytes(payload))
            }
        );
        assert_eq!(
            (first.length(), first.next_pc()),
            (5, GuestAddress(CODE + 5))
        );
        let next = decode_one(&memory, first.next_pc()).unwrap();
        assert_eq!(next.operation(), &expected_operation(CODE + 5, form));
        assert_eq!(
            (next.length() as usize, next.next_pc()),
            (form.len(), GuestAddress(CODE + 5 + form.len() as u32))
        );
    }
    for (bytes, expected) in [
        (
            &[0xe8, 0, 0, 0, 0][..],
            Operation::Call {
                target: BranchTarget::Direct(GuestAddress(CODE + 5)),
            },
        ),
        (&[0xc3][..], Operation::Return { stack_adjust: 0 }),
        (
            &[0xc2, 0xff, 0xff][..],
            Operation::Return {
                stack_adjust: 0xffff,
            },
        ),
    ] {
        assert_eq!(
            decode_one(&code_space(CODE, bytes), GuestAddress(CODE))
                .unwrap()
                .operation(),
            &expected
        );
    }
}

#[test]
fn word_near_control_strict_prefixes_indirect_far_and_other_word_branches_stay_closed() {
    let opcode = DecodeError::Unsupported(UnsupportedFeature::Opcode);
    for form in FORMS {
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
                [vec![prefix], form.to_vec()].concat(),
                [vec![0x66, prefix], form[1..].to_vec()].concat(),
            ] {
                assert_eq!(
                    decode_one(&code_space(CODE, &bytes), GuestAddress(CODE)).err(),
                    Some(expected),
                    "{bytes:02x?}"
                );
            }
        }
    }
    for bytes in [
        &[0x66, 0xff, 0xd0][..],
        &[0x66, 0xff, 0x13],
        &[0x66, 0xff, 0xe0],
        &[0x66, 0x66, 0xe9, 0, 0],
        &[0x66, 0x0f, 0x84, 0, 0],
        &[0x66, 0xcb],
        &[0x66, 0xca, 0, 0],
        &[0x66, 0x9a, 0, 0, 0, 0],
        &[0x66, 0xea, 0, 0, 0, 0],
    ] {
        assert_eq!(
            decode_one(&code_space(CODE, bytes), GuestAddress(CODE)).err(),
            Some(opcode),
            "{bytes:02x?}"
        );
    }
}

#[test]
fn word_near_control_is_backend_unsupported_in_both_standalone_profiles() {
    for form in FORMS {
        for trailing in [false, true] {
            let mut bytes = [vec![0x90], form.to_vec()].concat();
            if trailing {
                bytes.extend([0x0f, 0x0b]);
            }
            let memory = code_space(CODE, &bytes);
            let specs = [BlockSpec {
                entry: GuestAddress(CODE),
                byte_length: bytes.len() as u32,
            }];
            let expected = Some(CompileError::Instruction {
                pc: GuestAddress(CODE + 1),
                cause: InstructionError::BackendUnsupported,
            });
            assert_eq!(
                prepare_region(&memory, &specs, CompileLimits::default()).err(),
                expected
            );
            assert_eq!(
                compile_region(&memory, &specs, CompileLimits::default()).err(),
                expected
            );
            assert_eq!(
                prepare_entry_region(&memory, &[GuestAddress(CODE)], CompileLimits::default())
                    .err(),
                expected
            );
            assert_eq!(
                compile_entry_region(&memory, &[GuestAddress(CODE)], CompileLimits::default())
                    .err(),
                expected
            );
        }
    }
}

#[test]
fn word_near_control_terminates_bound_blocks_before_any_trailing_decode() {
    for form in FORMS {
        let bytes = [vec![0x90], form.to_vec(), vec![0x0f, 0x0b]].concat();
        for resident in [false, true] {
            let mut engine = code(&bytes);
            describe(&mut engine, &[(CODE, bytes.len() as u32)], false);
            let before = engine.arena().to_vec();
            let error = CompileError::Instruction {
                pc: GuestAddress(CODE + 1),
                cause: InstructionError::InvalidBlockEnd,
            };
            assert_eq!(
                compile(&mut engine, &[(CODE, bytes.len() as u32)], resident, false),
                Err(if resident {
                    HostError::Resident(RegistryError::Compile(error))
                } else {
                    HostError::Compile(error)
                })
            );
            assert_eq!(engine.arena(), before);
            assert_bound_admission(&bytes, &[(CODE, form.len() as u32 + 1)], resident, false);
            assert_bound_admission(&bytes, &[(CODE, 0)], resident, true);
        }
    }
}

fn fetch_error(pc: u32, address: u32, reason: FaultReason, length: usize) -> DecodeError {
    DecodeError::MemoryFault {
        pc: GuestAddress(pc),
        fault: MemoryFault {
            address: GuestAddress(address),
            access: Access::Execute,
            reason,
        },
        length: length as u32,
    }
}

#[test]
fn word_near_control_fetch_checks_exact_two_and_four_byte_page_top_execute_boundaries() {
    for form in FORMS {
        for pc in [0x1fff, u32::MAX - form.len() as u32 + 1] {
            let memory = code_space(pc, form);
            let decoded = decode_one(&memory, GuestAddress(pc)).unwrap();
            assert_eq!(decoded.operation(), &expected_operation(pc, form));
            assert_eq!(
                (decoded.length() as usize, decoded.next_pc()),
                (form.len(), GuestAddress(pc.wrapping_add(form.len() as u32)))
            );
        }
        for cut in 1..form.len() {
            let pc = 0x2000 - cut as u32;
            assert_eq!(
                decode_one(&code_space(pc, &form[..cut]), GuestAddress(pc)).err(),
                Some(fetch_error(pc, 0x2000, FaultReason::Unmapped, cut + 1))
            );
            let top = u32::MAX - cut as u32 + 1;
            assert_eq!(
                decode_one(&code_space(top, &form[..cut]), GuestAddress(top)).err(),
                Some(fetch_error(top, top, FaultReason::AddressOverflow, cut + 1))
            );
        }
        let pc = 0x2000 - form.len() as u32 + 1;
        let mut memory = code_space(pc, form);
        memory
            .protect(
                PageRange::new(GuestAddress(0x2000), 1).unwrap(),
                Permissions::READ,
            )
            .unwrap();
        assert_eq!(
            decode_one(&memory, GuestAddress(pc)).err(),
            Some(fetch_error(pc, 0x2000, FaultReason::Permission, form.len()))
        );
        let mut memory = code_space(CODE, form);
        memory
            .protect(
                PageRange::new(GuestAddress(CODE), 1).unwrap(),
                Permissions::READ,
            )
            .unwrap();
        assert_eq!(
            decode_one(&memory, GuestAddress(CODE)).err(),
            Some(fetch_error(CODE, CODE, FaultReason::Permission, 1))
        );
    }
}

#[test]
fn word_near_control_currency_tracks_consumed_code_without_stack_or_target_mapping() {
    for form in FORMS {
        let mut memory = AddressSpace::new(4).unwrap();
        memory
            .map_zeroed(
                PageRange::new(GuestAddress(CODE), 3).unwrap(),
                Permissions::ALL,
            )
            .unwrap();
        memory.write(GuestAddress(0x1fff), form).unwrap();
        let decoded = decode_one(&memory, GuestAddress(0x1fff)).unwrap();
        memory
            .map_zeroed(
                PageRange::new(GuestAddress(0x8000), 1).unwrap(),
                Permissions::ALL,
            )
            .unwrap();
        memory.write(GuestAddress(0x8000), &[0x90]).unwrap();
        assert!(memory.is_code_current(decoded.code_snapshot()));
        memory.write(GuestAddress(0x3000), &[0x90]).unwrap();
        assert!(memory.is_code_current(decoded.code_snapshot()));
        memory.write(GuestAddress(0x2000), &form[1..2]).unwrap();
        assert!(!memory.is_code_current(decoded.code_snapshot()));
    }
}

#[test]
fn word_near_control_mixed_64_instruction_three_block_unit_fits_four_bound_profiles() {
    let mut bytes = Vec::new();
    let mut blocks = Vec::new();
    for (index, form) in FORMS.into_iter().enumerate() {
        let offset = bytes.len();
        bytes.extend(std::iter::repeat_n(0x90, if index == 0 { 21 } else { 20 }));
        bytes.extend(form);
        blocks.push((CODE + offset as u32, (bytes.len() - offset) as u32));
    }
    for resident in [false, true] {
        for entries in [false, true] {
            assert_bound_admission(&bytes, &blocks, resident, entries);
        }
    }
}

#[test]
fn word_near_control_late_block_refusal_preserves_dirty_full_arena_and_both_owners() {
    let mut bytes = vec![0x90; 258];
    for (index, form) in FORMS.into_iter().enumerate() {
        bytes[index * 16..index * 16 + form.len()].copy_from_slice(form);
    }
    bytes[49..51].copy_from_slice(&[0x0f, 0x0b]);
    bytes[256..258].copy_from_slice(&[0xeb, 0]);
    let mut engine = code(&bytes);
    let replacement = compile(&mut engine, &[(CODE + 256, 2)], false, false).unwrap() as u32;
    let resident = compile(&mut engine, &[(CODE + 256, 2)], true, false).unwrap();
    let replacement_bytes = engine.artifact_bytes().unwrap().to_vec();
    let replacement_pointer = engine.artifact_bytes().unwrap().as_ptr();
    let resident_bytes = engine.resident_bytes(resident).unwrap().to_vec();
    let resident_pointer = engine.resident_bytes(resident).unwrap().as_ptr();
    let blocks = [(CODE, 4), (CODE + 16, 2), (CODE + 32, 4), (CODE + 48, 3)];
    let error = CompileError::Instruction {
        pc: GuestAddress(CODE + 49),
        cause: InstructionError::Decode(DecodeError::Unsupported(UnsupportedFeature::Opcode)),
    };
    for is_resident in [false, true] {
        for entries in [false, true] {
            for (index, byte) in engine.arena_mut().unwrap().iter_mut().enumerate() {
                *byte = (index.wrapping_mul(37).wrapping_add(11) & 0xff) as u8;
            }
            describe(&mut engine, &blocks, entries);
            let before = engine.arena().to_vec();
            assert_eq!(before.len(), 4364);
            assert_eq!(
                compile(&mut engine, &blocks, is_resident, entries),
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
