use ring3_engine::{
    abi::arena::TRANSFER_OFFSET,
    cpu::{
        UnsupportedFeature,
        dbt::{
            BlockSpec, CompileError, CompileLimits, InstructionError, RegistryError,
            compile_entry_region, compile_region,
        },
        x86::{
            Register32,
            decode::{DecodeError, decode_one},
            ir::{DecimalAdjustKind, Location32, Operation, UnaryKind, Value32},
        },
    },
    memory::{
        Access, AddressSpace, FaultReason, GuestAddress, MemoryFault, PageRange, Permissions,
    },
    process::{EngineInstance, HostError},
};

const CODE: u32 = 0x1000;
const KEY: u64 = 0x4445_4349_4d41_4c31;
const BANK: [u8; 4] = [0x27, 0x2f, 0xeb, 0];

fn code(bytes: &[u8]) -> EngineInstance {
    code_with_permissions(bytes, true)
}

fn code_with_permissions(bytes: &[u8], execute_only: bool) -> EngineInstance {
    let mut engine = EngineInstance::new(2, KEY).unwrap();
    engine.map(CODE, 1, 7).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(bytes);
    engine.upload(CODE, bytes.len() as u32).unwrap();
    if execute_only {
        engine.protect(CODE, 1, 4).unwrap();
    }
    engine
}

fn decode_admission(opcode: u8) {
    let engine = code(&[opcode]);
    let before = engine.arena().to_vec();
    let decoded = decode_one(engine.memory().unwrap(), GuestAddress(CODE))
        .expect("exact decimal adjust must decode");
    assert_eq!(decoded.operation(), &operation(opcode));
    assert_eq!(
        (decoded.length(), decoded.next_pc()),
        (1, GuestAddress(CODE + 1))
    );
    assert!(
        engine
            .memory()
            .unwrap()
            .is_code_current(decoded.code_snapshot())
    );
    assert_eq!(engine.arena(), before);
    assert_eq!(engine.memory().unwrap().mapped_pages(), 1);
}

#[test]
fn daa_decodes_exact_one_byte_form() {
    decode_admission(0x27);
}

#[test]
fn das_decodes_exact_one_byte_form() {
    decode_admission(0x2f);
}

fn standalone_admission(entries: bool) {
    assert_standalone_admission(&BANK, 3, entries);
}

fn assert_standalone_admission(bytes: &[u8], instructions: usize, entries: bool) {
    let engine = code(bytes);
    let memory = engine.memory().unwrap();
    let compiled = if entries {
        compile_entry_region(memory, &[GuestAddress(CODE)], CompileLimits::default())
    } else {
        compile_region(
            memory,
            &[BlockSpec {
                entry: GuestAddress(CODE),
                byte_length: bytes.len() as u32,
            }],
            CompileLimits::default(),
        )
    }
    .expect("both decimal adjust forms must compile standalone");
    assert_eq!(
        (compiled.metadata().blocks, compiled.metadata().instructions),
        (1, instructions)
    );
    assert_eq!(
        &compiled.wasm_bytes(memory).unwrap()[..8],
        b"\0asm\x01\0\0\0"
    );
    assert_helper_free(compiled.wasm_bytes(memory).unwrap());
    assert_eq!(memory.mapped_pages(), 1);
}

#[test]
fn decimal_adjust_compiles_standalone_explicit() {
    standalone_admission(false);
}

#[test]
fn decimal_adjust_compiles_standalone_entry() {
    standalone_admission(true);
}

fn bound_admission(resident: bool, entries: bool) {
    assert_bound_admission(&BANK, resident, entries);
}

fn assert_bound_admission(bytes: &[u8], resident: bool, entries: bool) {
    let mut engine = code(bytes);
    describe(&mut engine, CODE, bytes.len() as u32, entries);
    let before = engine.arena().to_vec();
    let id = bound_compile(&mut engine, resident, entries)
        .expect("both decimal adjust forms must compile bound without mapped data");
    let module = if resident {
        engine.resident_bytes(id).unwrap()
    } else {
        engine.artifact_bytes().unwrap()
    };
    assert_eq!(&module[..8], b"\0asm\x01\0\0\0");
    assert_helper_free(module);
    assert_eq!(engine.arena(), before);
    assert_eq!(engine.memory().unwrap().mapped_pages(), 1);
}

fn describe(engine: &mut EngineInstance, pc: u32, length: u32, entries: bool) {
    let descriptor = &mut engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8];
    descriptor[..4].copy_from_slice(&pc.to_le_bytes());
    descriptor[4..].copy_from_slice(&if entries { 0_u32 } else { length }.to_le_bytes());
}

fn bound_compile(
    engine: &mut EngineInstance,
    resident: bool,
    entries: bool,
) -> Result<u64, HostError> {
    match (resident, entries) {
        (false, false) => engine.compile(1).map(u64::from),
        (false, true) => engine.compile_entries(1, 0).map(u64::from),
        (true, false) => engine.compile_resident(1).map(|id| id.get()),
        (true, true) => engine.compile_resident_entries(1, 0).map(|id| id.get()),
    }
}

#[test]
fn decimal_adjust_compiles_replacement_explicit() {
    bound_admission(false, false);
}

#[test]
fn decimal_adjust_compiles_replacement_entry() {
    bound_admission(false, true);
}

#[test]
fn decimal_adjust_compiles_resident_explicit() {
    bound_admission(true, false);
}

#[test]
fn decimal_adjust_compiles_resident_entry() {
    bound_admission(true, true);
}

fn operation(opcode: u8) -> Operation {
    Operation::DecimalAdjust {
        kind: match opcode {
            0x27 => DecimalAdjustKind::Add,
            0x2f => DecimalAdjustKind::Subtract,
            _ => unreachable!(),
        },
    }
}

fn assert_helper_free(module: &[u8]) {
    for name in [b"read".as_slice(), b"store", b"dispatch"] {
        assert!(!module.windows(name.len()).any(|bytes| bytes == name));
    }
}

fn code_space(pc: u32, bytes: &[u8]) -> AddressSpace {
    let pages = (u64::from(pc & 0xfff) + bytes.len() as u64).div_ceil(4096) as u32;
    let mut memory = AddressSpace::new(pages + 1).unwrap();
    let range = PageRange::new(GuestAddress(pc & !0xfff), pages).unwrap();
    memory.map_zeroed(range, Permissions::ALL).unwrap();
    memory.write(GuestAddress(pc), bytes).unwrap();
    memory.protect(range, Permissions::EXECUTE).unwrap();
    memory
}

fn instruction_error(pc: u32, error: DecodeError) -> CompileError {
    CompileError::Instruction {
        pc: GuestAddress(pc),
        cause: InstructionError::Decode(error),
    }
}

fn fetch_error(pc: u32, address: u32, length: u32, reason: FaultReason) -> DecodeError {
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
fn decimal_adjust_ordinary_fallthrough_and_fixed32_payload_adjacency_are_exact() {
    let bytes = [0x48, 0x27, 0x2f, 0xb8, 0x27, 0x2f, 0x66, 0x67, 0x90];
    let engine = code(&bytes);
    let memory = engine.memory().unwrap();
    for (offset, length, expected) in [
        (
            0,
            1,
            Operation::Unary {
                kind: UnaryKind::Dec,
                destination: Location32::Register(Register32::Eax),
            },
        ),
        (1, 1, operation(0x27)),
        (2, 1, operation(0x2f)),
        (
            3,
            5,
            Operation::Move {
                destination: Location32::Register(Register32::Eax),
                source: Value32::Immediate(0x6766_2f27),
            },
        ),
        (8, 1, Operation::Nop),
    ] {
        let decoded = decode_one(memory, GuestAddress(CODE + offset)).unwrap();
        assert_eq!(decoded.operation(), &expected);
        assert_eq!(
            (decoded.length(), decoded.next_pc()),
            (length, GuestAddress(CODE + offset + u32::from(length)))
        );
    }
    assert_standalone_admission(&[0x27, 0x2f], 2, false);
    for resident in [false, true] {
        assert_bound_admission(&[0x27, 0x2f], resident, false);
    }
}

#[test]
fn decimal_adjust_strict_prefixes_and_other_decimal_forms_keep_categories() {
    let opcode_error = DecodeError::Unsupported(UnsupportedFeature::Opcode);
    let segment_error = DecodeError::Unsupported(UnsupportedFeature::Segment);
    let mut cases = Vec::new();
    for opcode in [0x27, 0x2f] {
        for prefix in [0x66, 0x67, 0xf2, 0xf3] {
            cases.push((vec![prefix, opcode], opcode_error));
        }
        for prefix in [0x26, 0x2e, 0x36, 0x3e, 0x64, 0x65] {
            cases.push((vec![prefix, opcode], segment_error));
        }
        cases.push((vec![0xf0, opcode], DecodeError::InvalidEncoding));
        cases.push((vec![0x66, 0x64, opcode], segment_error));
    }
    for bytes in [vec![0x37], vec![0x3f], vec![0xd4, 0x0a], vec![0xd5, 0x0a]] {
        cases.push((bytes, opcode_error));
    }
    assert_eq!(cases.len(), 28);
    for (mut bytes, error) in cases {
        bytes.extend([0xeb, 0]);
        let mut engine = code(&bytes);
        let memory = engine.memory().unwrap();
        assert_eq!(decode_one(memory, GuestAddress(CODE)).err(), Some(error));
        let expected = instruction_error(CODE, error);
        assert_eq!(
            compile_region(
                memory,
                &[BlockSpec {
                    entry: GuestAddress(CODE),
                    byte_length: bytes.len() as u32
                }],
                CompileLimits::default(),
            )
            .err(),
            Some(expected),
        );
        assert_eq!(
            compile_entry_region(memory, &[GuestAddress(CODE)], CompileLimits::default()).err(),
            Some(expected),
        );
        for resident in [false, true] {
            for entries in [false, true] {
                describe(&mut engine, CODE, bytes.len() as u32, entries);
                assert_eq!(
                    bound_compile(&mut engine, resident, entries),
                    Err(if resident {
                        HostError::Resident(RegistryError::Compile(expected))
                    } else {
                        HostError::Compile(expected)
                    }),
                );
            }
        }
    }
}

#[test]
fn decimal_adjust_one_byte_fetch_execute_and_top_boundaries_do_not_wrap() {
    for opcode in [0x27, 0x2f] {
        for pc in [CODE, 0x1fff, u32::MAX] {
            let mut memory = code_space(pc, &[opcode]);
            if pc == u32::MAX {
                let zero = PageRange::new(GuestAddress(0), 1).unwrap();
                memory.map_zeroed(zero, Permissions::ALL).unwrap();
                memory.write(GuestAddress(0), &[0x0f, 0x0b]).unwrap();
                memory.protect(zero, Permissions::READ).unwrap();
            }
            let decoded = decode_one(&memory, GuestAddress(pc)).unwrap();
            assert_eq!(decoded.operation(), &operation(opcode));
            assert_eq!(
                (decoded.length(), decoded.next_pc()),
                (1, GuestAddress(pc.wrapping_add(1)))
            );
            assert!(memory.is_code_current(decoded.code_snapshot()));
        }
        let mut denied = code_space(CODE, &[opcode]);
        denied
            .protect(
                PageRange::new(GuestAddress(CODE), 1).unwrap(),
                Permissions::READ,
            )
            .unwrap();
        assert_eq!(
            decode_one(&denied, GuestAddress(CODE)).err(),
            Some(fetch_error(CODE, CODE, 1, FaultReason::Permission))
        );

        let prefix_end = code_space(0x1fff, &[0x66]);
        assert_eq!(
            decode_one(&prefix_end, GuestAddress(0x1fff)).err(),
            Some(fetch_error(0x1fff, 0x2000, 2, FaultReason::Unmapped))
        );
        let mut denied_second = code_space(0x1fff, &[0x66, opcode]);
        denied_second
            .protect(
                PageRange::new(GuestAddress(0x2000), 1).unwrap(),
                Permissions::READ,
            )
            .unwrap();
        assert_eq!(
            decode_one(&denied_second, GuestAddress(0x1fff)).err(),
            Some(fetch_error(0x1fff, 0x2000, 2, FaultReason::Permission))
        );

        let mut prefix_top = code_space(u32::MAX, &[0x66]);
        let zero = PageRange::new(GuestAddress(0), 1).unwrap();
        prefix_top.map_zeroed(zero, Permissions::ALL).unwrap();
        prefix_top.write(GuestAddress(0), &[opcode]).unwrap();
        prefix_top.protect(zero, Permissions::EXECUTE).unwrap();
        assert_eq!(
            decode_one(&prefix_top, GuestAddress(u32::MAX)).err(),
            Some(fetch_error(
                u32::MAX,
                u32::MAX,
                2,
                FaultReason::AddressOverflow
            ))
        );
    }
    let missing = AddressSpace::new(1).unwrap();
    assert_eq!(
        decode_one(&missing, GuestAddress(CODE)).err(),
        Some(fetch_error(CODE, CODE, 1, FaultReason::Unmapped))
    );
}

#[test]
fn decimal_adjust_source_currentness_separates_data_writes_from_source_mutations() {
    for identity_write in [false, true] {
        let mut engine = code_with_permissions(&BANK, !identity_write);
        let decoded = decode_one(engine.memory().unwrap(), GuestAddress(CODE)).unwrap();
        let standalone = compile_region(
            engine.memory().unwrap(),
            &[BlockSpec {
                entry: GuestAddress(CODE),
                byte_length: BANK.len() as u32,
            }],
            CompileLimits::default(),
        )
        .unwrap();
        describe(&mut engine, CODE, BANK.len() as u32, false);
        let replacement = bound_compile(&mut engine, false, false).unwrap() as u32;
        let resident = bound_compile(&mut engine, true, false).unwrap();

        engine
            .memory()
            .unwrap()
            .resolve(GuestAddress(0x5000), Access::Read)
            .unwrap_err();
        engine.map(0x5000, 1, 3).unwrap();
        engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 2]
            .copy_from_slice(&[0x27, 0x2f]);
        engine.upload(0x5000, 2).unwrap();
        engine.protect(0x5000, 1, 1).unwrap();
        engine.unmap(0x5000, 1).unwrap();
        assert!(
            engine
                .memory()
                .unwrap()
                .is_code_current(decoded.code_snapshot())
        );
        assert!(standalone.wasm_bytes(engine.memory().unwrap()).is_ok());
        engine.guard(KEY, replacement).unwrap();
        engine.guard_resident(KEY, resident).unwrap();

        if identity_write {
            engine.arena_mut().unwrap()[TRANSFER_OFFSET] = 0x27;
            engine.upload(CODE, 1).unwrap();
        } else {
            engine.protect(CODE, 1, 4).unwrap();
        }
        assert!(
            !engine
                .memory()
                .unwrap()
                .is_code_current(decoded.code_snapshot())
        );
        assert!(standalone.wasm_bytes(engine.memory().unwrap()).is_err());
        assert!(engine.guard(KEY, replacement).is_err());
        assert!(engine.guard_resident(KEY, resident).is_err());
        assert!(engine.artifact_bytes().is_err());
        assert!(engine.resident_bytes(resident).is_err());
    }
}

#[test]
fn decimal_adjust_mixed_unit_fits_exactly_64_instructions_in_all_six_profiles() {
    let mut bytes = [0x27, 0x2f].repeat(16);
    bytes.extend(std::iter::repeat_n(0x90, 31));
    bytes.extend([0xeb, 0]);
    for entries in [false, true] {
        assert_standalone_admission(&bytes, 64, entries);
        for resident in [false, true] {
            assert_bound_admission(&bytes, resident, entries);
        }
    }
}

#[test]
fn decimal_adjust_late_refusal_preserves_dirty_full_arena_and_both_owners() {
    let mut bytes = vec![0x27, 0x2f, 0x0f, 0x0b];
    bytes.resize(256, 0x90);
    bytes.extend([0xeb, 0]);
    let mut engine = code(&bytes);
    describe(&mut engine, CODE + 256, 2, false);
    let replacement = bound_compile(&mut engine, false, false).unwrap() as u32;
    let resident = bound_compile(&mut engine, true, false).unwrap();
    let replacement_bytes = engine.artifact_bytes().unwrap().to_vec();
    let replacement_pointer = engine.artifact_bytes().unwrap().as_ptr();
    let resident_bytes = engine.resident_bytes(resident).unwrap().to_vec();
    let resident_pointer = engine.resident_bytes(resident).unwrap().as_ptr();
    let error = instruction_error(
        CODE + 2,
        DecodeError::Unsupported(UnsupportedFeature::Opcode),
    );
    for is_resident in [false, true] {
        for entries in [false, true] {
            for (index, byte) in engine.arena_mut().unwrap().iter_mut().enumerate() {
                *byte = (index.wrapping_mul(37).wrapping_add(11) & 0xff) as u8;
            }
            describe(&mut engine, CODE, 4, entries);
            let before = engine.arena().to_vec();
            assert_eq!(before.len(), 4364);
            assert_eq!(
                bound_compile(&mut engine, is_resident, entries),
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
