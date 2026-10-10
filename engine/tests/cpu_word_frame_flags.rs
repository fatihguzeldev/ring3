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
            ir::{Location32, Operation, UnaryKind, Value32},
        },
    },
    memory::{
        Access, AddressSpace, FaultReason, GuestAddress, MemoryFault, PageRange, Permissions,
    },
    process::{EngineInstance, HostError},
};

const CODE: u32 = 0x1000;
const KEY: u64 = 0x5746_5241_4d45_3136;

fn code(bytes: &[u8]) -> EngineInstance {
    let mut engine = EngineInstance::new(1, KEY).unwrap();
    engine.map(CODE, 1, 7).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(bytes);
    engine.upload(CODE, bytes.len() as u32).unwrap();
    engine.protect(CODE, 1, 4).unwrap();
    engine
}

fn decode_admission(opcode: u8) {
    let engine = code(&[0x66, opcode]);
    let before = engine.arena().to_vec();
    let decoded = decode_one(engine.memory().unwrap(), GuestAddress(CODE))
        .expect("WORD PUSHF and LEAVE must decode without stack or frame access");
    assert_eq!(decoded.operation(), &operation(opcode));
    assert_eq!(
        (decoded.length(), decoded.next_pc()),
        (2, GuestAddress(CODE + 2))
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
fn pushfw_decodes_exact_word_flags_form() {
    decode_admission(0x9c);
}

#[test]
fn leavew_decodes_exact_word_frame_form() {
    decode_admission(0xc9);
}

fn bound_admission(resident: bool, entries: bool) {
    let bytes = [0x66, 0x9c, 0x66, 0xc9, 0xeb, 0];
    assert_bound_admission(&bytes, resident, entries);
}

fn assert_bound_admission(bytes: &[u8], resident: bool, entries: bool) {
    let mut engine = code(bytes);
    let descriptor = &mut engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8];
    descriptor[..4].copy_from_slice(&CODE.to_le_bytes());
    descriptor[4..]
        .copy_from_slice(&if entries { 0_u32 } else { bytes.len() as u32 }.to_le_bytes());
    let before = engine.arena().to_vec();
    let id = match (resident, entries) {
        (false, false) => engine.compile(1).map(u64::from),
        (false, true) => engine.compile_entries(1, 0).map(u64::from),
        (true, false) => engine.compile_resident(1).map(|id| id.get()),
        (true, true) => engine.compile_resident_entries(1, 0).map(|id| id.get()),
    }
    .expect("WORD PUSHF and LEAVE must compile bound without data mapping");
    let module = if resident {
        engine.resident_bytes(id).unwrap()
    } else {
        engine.artifact_bytes().unwrap()
    };
    assert_eq!(&module[..8], b"\0asm\x01\0\0\0");
    assert_eq!(engine.arena(), before);
    assert_eq!(engine.memory().unwrap().mapped_pages(), 1);
}

#[test]
fn word_frame_flags_compiles_replacement_explicit() {
    bound_admission(false, false);
}

#[test]
fn word_frame_flags_compiles_replacement_entry() {
    bound_admission(false, true);
}

#[test]
fn word_frame_flags_compiles_resident_explicit() {
    bound_admission(true, false);
}

#[test]
fn word_frame_flags_compiles_resident_entry() {
    bound_admission(true, true);
}

fn operation(opcode: u8) -> Operation {
    match opcode {
        0x9c => Operation::PushFlagsWord,
        0xc9 => Operation::LeaveWord,
        _ => unreachable!(),
    }
}

fn code_space(pc: u32, bytes: &[u8]) -> AddressSpace {
    let pages = (u64::from(pc & 0xfff) + bytes.len() as u64).div_ceil(4096) as u32;
    let mut memory = AddressSpace::new(pages).unwrap();
    let range = PageRange::new(GuestAddress(pc & !0xfff), pages).unwrap();
    memory.map_zeroed(range, Permissions::ALL).unwrap();
    memory.write(GuestAddress(pc), bytes).unwrap();
    memory.protect(range, Permissions::EXECUTE).unwrap();
    memory
}

#[test]
fn word_frame_flags_exact_ir_fixed32_and_prefix_payload_adjacency_are_literal() {
    for opcode in [0x9c, 0xc9] {
        let bytes = [0x48, 0x66, opcode];
        let memory = code_space(CODE, &bytes);
        let first = decode_one(&memory, GuestAddress(CODE)).unwrap();
        assert_eq!(
            first.operation(),
            &Operation::Unary {
                kind: UnaryKind::Dec,
                destination: Location32::Register(Register32::Eax)
            }
        );
        assert_eq!(
            (first.length(), first.next_pc()),
            (1, GuestAddress(CODE + 1))
        );
        let next = decode_one(&memory, first.next_pc()).unwrap();
        assert_eq!(next.operation(), &operation(opcode));
        assert_eq!((next.length(), next.next_pc()), (2, GuestAddress(CODE + 3)));
        for payload in [[0x66, 0x67, 0xf0, 0xf3], [0xf2, 0x9c, 0x66, 0xc9]] {
            let bytes = [vec![0xb8], payload.to_vec(), vec![0x66, opcode]].concat();
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
            assert_eq!(next.operation(), &operation(opcode));
            assert_eq!((next.length(), next.next_pc()), (2, GuestAddress(CODE + 7)));
            assert!(memory.is_code_current(next.code_snapshot()));
        }
        let memory = code_space(CODE, &[opcode]);
        let old = decode_one(&memory, GuestAddress(CODE)).unwrap();
        assert_eq!(
            old.operation(),
            &if opcode == 0x9c {
                Operation::PushFlags
            } else {
                Operation::Leave
            }
        );
        assert_eq!((old.length(), old.next_pc()), (1, GuestAddress(CODE + 1)));
    }
}

#[test]
fn word_frame_flags_prefixes_lock_and_closed_stack_neighbors_stay_precise() {
    for opcode in [0x9c, 0xc9] {
        for prefix in [
            0x66, 0x67, 0xf2, 0xf3, 0xf0, 0x26, 0x2e, 0x36, 0x3e, 0x64, 0x65,
        ] {
            let expected = match prefix {
                0xf0 => DecodeError::InvalidEncoding,
                0x26 | 0x2e | 0x36 | 0x3e | 0x64 | 0x65 => {
                    DecodeError::Unsupported(UnsupportedFeature::Segment)
                }
                _ => DecodeError::Unsupported(UnsupportedFeature::Opcode),
            };
            for bytes in [[prefix, 0x66, opcode], [0x66, prefix, opcode]] {
                assert_eq!(
                    decode_one(&code_space(CODE, &bytes), GuestAddress(CODE)).err(),
                    Some(expected),
                    "{bytes:02x?}"
                );
            }
        }
    }
    for bytes in [
        &[0x9d][..],
        &[0x66, 0x9d],
        &[0xc8, 0, 0, 0],
        &[0x66, 0xc8, 0, 0, 0],
        &[0x66, 0x60],
    ] {
        assert_eq!(
            decode_one(&code_space(CODE, bytes), GuestAddress(CODE)).err(),
            Some(DecodeError::Unsupported(UnsupportedFeature::Opcode)),
            "{bytes:02x?}"
        );
    }
    for bytes in [
        &[0x66, 0x06][..],
        &[0x66, 0x07],
        &[0x66, 0x0f, 0xa0],
        &[0x66, 0x0f, 0xa1],
    ] {
        assert_eq!(
            decode_one(&code_space(CODE, bytes), GuestAddress(CODE)).err(),
            Some(DecodeError::Unsupported(UnsupportedFeature::Segment)),
            "{bytes:02x?}"
        );
    }
}

#[test]
fn word_frame_flags_single_and_repeated_banks_compile_in_four_bound_profiles() {
    for bytes in [
        &[0x66, 0x9c, 0x90, 0xeb, 0][..],
        &[0x66, 0xc9, 0x90, 0xeb, 0],
        &[0x66, 0x9c, 0x66, 0x9c, 0x66, 0xc9, 0x66, 0xc9, 0xeb, 0],
    ] {
        for resident in [false, true] {
            for entries in [false, true] {
                assert_bound_admission(bytes, resident, entries);
            }
        }
    }
}

#[test]
fn decoded_word_frame_flags_stay_closed_in_all_four_standalone_apis() {
    for opcode in [0x9c, 0xc9] {
        let bytes = [0x90, 0x66, opcode, 0xeb, 0];
        let memory = code_space(CODE, &bytes);
        let snapshot = memory
            .snapshot_code(GuestAddress(CODE), bytes.len())
            .unwrap();
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
            prepare_entry_region(&memory, &[GuestAddress(CODE)], CompileLimits::default()).err(),
            expected
        );
        assert_eq!(
            compile_entry_region(&memory, &[GuestAddress(CODE)], CompileLimits::default()).err(),
            expected
        );
        assert!(memory.is_code_current(&snapshot));
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
fn word_frame_flags_fetch_checks_exact_two_byte_page_top_and_execute_boundaries() {
    for opcode in [0x9c, 0xc9] {
        let bytes = [0x66, opcode];
        for pc in [CODE, 0x1fff, u32::MAX - 1] {
            let memory = code_space(pc, &bytes);
            let decoded = decode_one(&memory, GuestAddress(pc)).unwrap();
            assert_eq!(decoded.operation(), &operation(opcode));
            assert_eq!(
                (decoded.length(), decoded.next_pc()),
                (2, GuestAddress(pc.wrapping_add(2)))
            );
            assert!(memory.is_code_current(decoded.code_snapshot()));
        }
        assert_eq!(
            decode_one(&code_space(0x1fff, &[0x66]), GuestAddress(0x1fff)).err(),
            Some(fetch_error(0x1fff, 0x2000, FaultReason::Unmapped, 2))
        );
        assert_eq!(
            decode_one(&code_space(u32::MAX, &[0x66]), GuestAddress(u32::MAX)).err(),
            Some(fetch_error(
                u32::MAX,
                u32::MAX,
                FaultReason::AddressOverflow,
                2
            ))
        );
        let mut memory = code_space(0x1fff, &bytes);
        memory
            .protect(
                PageRange::new(GuestAddress(0x2000), 1).unwrap(),
                Permissions::READ,
            )
            .unwrap();
        assert_eq!(
            decode_one(&memory, GuestAddress(0x1fff)).err(),
            Some(fetch_error(0x1fff, 0x2000, FaultReason::Permission, 2))
        );
        let mut memory = code_space(CODE, &bytes);
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
fn word_frame_flags_mixed_64_instruction_block_fits_four_bound_profiles() {
    let mut bytes = Vec::new();
    for _ in 0..8 {
        bytes.extend([0x66, 0x9c, 0x66, 0xc9]);
    }
    bytes.extend(std::iter::repeat_n(0x90, 47));
    bytes.extend([0xeb, 0]);
    for resident in [false, true] {
        for entries in [false, true] {
            assert_bound_admission(&bytes, resident, entries);
        }
    }
}

#[test]
fn word_frame_flags_late_refusal_preserves_full_arena_and_both_owners() {
    let mut bytes = vec![0x66, 0x9c, 0x66, 0xc9];
    let refused_pc = CODE + bytes.len() as u32;
    bytes.extend([0x0f, 0x0b]);
    let refused_length = bytes.len() as u32;
    bytes.resize(256, 0x90);
    bytes.extend([0xeb, 0]);
    let mut engine = code(&bytes);
    let descriptor = &mut engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8];
    descriptor[..4].copy_from_slice(&(CODE + 256).to_le_bytes());
    descriptor[4..].copy_from_slice(&2_u32.to_le_bytes());
    let replacement = engine.compile(1).unwrap();
    let resident = engine.compile_resident(1).unwrap().get();
    let replacement_bytes = engine.artifact_bytes().unwrap().to_vec();
    let replacement_pointer = engine.artifact_bytes().unwrap().as_ptr();
    let resident_bytes = engine.resident_bytes(resident).unwrap().to_vec();
    let resident_pointer = engine.resident_bytes(resident).unwrap().as_ptr();
    let error = CompileError::Instruction {
        pc: GuestAddress(refused_pc),
        cause: InstructionError::Decode(DecodeError::Unsupported(UnsupportedFeature::Opcode)),
    };
    for is_resident in [false, true] {
        for entries in [false, true] {
            let descriptor = &mut engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8];
            descriptor[..4].copy_from_slice(&CODE.to_le_bytes());
            descriptor[4..]
                .copy_from_slice(&if entries { 0 } else { refused_length }.to_le_bytes());
            let before = engine.arena().to_vec();
            assert_eq!(before.len(), 4364);
            let result = match (is_resident, entries) {
                (false, false) => engine.compile(1).map(u64::from),
                (false, true) => engine.compile_entries(1, 0).map(u64::from),
                (true, false) => engine.compile_resident(1).map(|id| id.get()),
                (true, true) => engine.compile_resident_entries(1, 0).map(|id| id.get()),
            };
            assert_eq!(
                result,
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
