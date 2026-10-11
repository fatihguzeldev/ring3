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
            ir::{AsciiAdjustKind, DecimalAdjustKind, Location32, Operation, UnaryKind, Value32},
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
const ASCII_BANK: [u8; 4] = [0x37, 0x3f, 0xeb, 0];
const AAD_BANK: [u8; 20] = [
    0xd5, 0, 0xd5, 0x0a, 0xd5, 0x80, 0xd5, 0xff, 0xd5, 0x66, 0xd5, 0x67, 0xd5, 0xf0, 0xd5, 0xf2,
    0xd5, 0xf3, 0xeb, 0,
];

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
        .expect("exact accumulator adjust must decode");
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
    .expect("accumulator adjust forms must compile standalone");
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
        .expect("accumulator adjust forms must compile bound without mapped data");
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
    match opcode {
        0x27 | 0x2f => Operation::DecimalAdjust {
            kind: if opcode == 0x27 {
                DecimalAdjustKind::Add
            } else {
                DecimalAdjustKind::Subtract
            },
        },
        0x37 | 0x3f => Operation::AsciiAdjust {
            kind: if opcode == 0x37 {
                AsciiAdjustKind::Add
            } else {
                AsciiAdjustKind::Subtract
            },
        },
        _ => unreachable!(),
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
    let bytes = [
        0x48, 0x27, 0x2f, 0x37, 0x3f, 0xd5, 0xf0, 0xd5, 0xff, 0xb8, 0x27, 0x2f, 0x66, 0x67, 0xb9,
        0x37, 0x3f, 0x66, 0x67, 0x90,
    ];
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
        (3, 1, operation(0x37)),
        (4, 1, operation(0x3f)),
        (5, 2, Operation::AsciiAdjustBeforeDivision { base: 0xf0 }),
        (7, 2, Operation::AsciiAdjustBeforeDivision { base: 0xff }),
        (
            9,
            5,
            Operation::Move {
                destination: Location32::Register(Register32::Eax),
                source: Value32::Immediate(0x6766_2f27),
            },
        ),
        (
            14,
            5,
            Operation::Move {
                destination: Location32::Register(Register32::Ecx),
                source: Value32::Immediate(0x6766_3f37),
            },
        ),
        (19, 1, Operation::Nop),
    ] {
        let decoded = decode_one(memory, GuestAddress(CODE + offset)).unwrap();
        assert_eq!(decoded.operation(), &expected);
        assert_eq!(
            (decoded.length(), decoded.next_pc()),
            (length, GuestAddress(CODE + offset + u32::from(length)))
        );
    }
    let ordinary = [0x27, 0x2f, 0x37, 0x3f, 0xd5, 0xf0, 0xd5, 0xff];
    assert_standalone_admission(&ordinary, 6, false);
    for resident in [false, true] {
        assert_bound_admission(&ordinary, resident, false);
    }
}

#[test]
fn decimal_adjust_strict_prefixes_and_other_decimal_forms_keep_categories() {
    let opcode_error = DecodeError::Unsupported(UnsupportedFeature::Opcode);
    let segment_error = DecodeError::Unsupported(UnsupportedFeature::Segment);
    let mut cases = Vec::new();
    for form in [
        &[0x27][..],
        &[0x2f][..],
        &[0x37][..],
        &[0x3f][..],
        &[0xd5, 0x0a][..],
    ] {
        for prefix in [0x66, 0x67, 0xf2, 0xf3] {
            let mut bytes = vec![prefix];
            bytes.extend_from_slice(form);
            cases.push((bytes, opcode_error));
        }
        for prefix in [0x26, 0x2e, 0x36, 0x3e, 0x64, 0x65] {
            let mut bytes = vec![prefix];
            bytes.extend_from_slice(form);
            cases.push((bytes, segment_error));
        }
        let mut locked = vec![0xf0];
        locked.extend_from_slice(form);
        cases.push((locked, DecodeError::InvalidEncoding));
        let mut segment = vec![0x66, 0x64];
        segment.extend_from_slice(form);
        cases.push((segment, segment_error));
    }
    cases.push((vec![0xd4, 0x0a], opcode_error));
    assert_eq!(cases.len(), 61);
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
    for opcode in [0x27, 0x2f, 0x37, 0x3f] {
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
    for bank in [BANK.as_slice(), ASCII_BANK.as_slice(), AAD_BANK.as_slice()] {
        for identity_write in [false, true] {
            let mut engine = code_with_permissions(bank, !identity_write);
            let decoded = decode_one(engine.memory().unwrap(), GuestAddress(CODE)).unwrap();
            let standalone = compile_region(
                engine.memory().unwrap(),
                &[BlockSpec {
                    entry: GuestAddress(CODE),
                    byte_length: bank.len() as u32,
                }],
                CompileLimits::default(),
            )
            .unwrap();
            describe(&mut engine, CODE, bank.len() as u32, false);
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
                let offset = usize::from(decoded.length() - 1);
                engine.arena_mut().unwrap()[TRANSFER_OFFSET] = bank[offset];
                engine.upload(CODE + offset as u32, 1).unwrap();
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
}

#[test]
fn decimal_adjust_mixed_unit_fits_exactly_64_instructions_in_all_six_profiles() {
    let mut bytes = Vec::new();
    for instruction in 0..32 {
        if instruction % 5 == 4 {
            bytes.extend([0xd5, 0xff]);
        } else {
            bytes.push([0x27, 0x2f, 0x37, 0x3f][instruction % 5]);
        }
    }
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
    let mut bytes = vec![0x27, 0x2f, 0x37, 0x3f, 0xd5, 0xff];
    let refusal_pc = CODE + bytes.len() as u32;
    bytes.extend([0x0f, 0x0b]);
    let refusal_length = bytes.len() as u32;
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
        refusal_pc,
        DecodeError::Unsupported(UnsupportedFeature::Opcode),
    );
    for is_resident in [false, true] {
        for entries in [false, true] {
            for (index, byte) in engine.arena_mut().unwrap().iter_mut().enumerate() {
                *byte = (index.wrapping_mul(37).wrapping_add(11) & 0xff) as u8;
            }
            describe(&mut engine, CODE, refusal_length, entries);
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

#[test]
fn ascii_adjust_aaa_decodes_exact_one_byte_form() {
    decode_admission(0x37);
}

#[test]
fn ascii_adjust_aas_decodes_exact_one_byte_form() {
    decode_admission(0x3f);
}

#[test]
fn ascii_adjust_compiles_standalone_explicit() {
    assert_standalone_admission(&ASCII_BANK, 3, false);
}

#[test]
fn ascii_adjust_compiles_standalone_entry() {
    assert_standalone_admission(&ASCII_BANK, 3, true);
}

#[test]
fn ascii_adjust_compiles_replacement_explicit() {
    assert_bound_admission(&ASCII_BANK, false, false);
}

#[test]
fn ascii_adjust_compiles_replacement_entry() {
    assert_bound_admission(&ASCII_BANK, false, true);
}

#[test]
fn ascii_adjust_compiles_resident_explicit() {
    assert_bound_admission(&ASCII_BANK, true, false);
}

#[test]
fn ascii_adjust_compiles_resident_entry() {
    assert_bound_admission(&ASCII_BANK, true, true);
}

#[test]
fn aad_decodes_exact_immediate_form() {
    for base in 0..=u8::MAX {
        let engine = code(&[0xd5, base]);
        let before = engine.arena().to_vec();
        let memory = engine.memory().unwrap();
        let decoded =
            decode_one(memory, GuestAddress(CODE)).expect("exact AAD immediate form must decode");
        assert_eq!(
            decoded.operation(),
            &Operation::AsciiAdjustBeforeDivision { base }
        );
        assert_eq!(
            (decoded.length(), decoded.next_pc()),
            (2, GuestAddress(CODE + 2))
        );
        assert!(memory.is_code_current(decoded.code_snapshot()));
        assert_eq!(engine.arena(), before);
        assert_eq!(memory.mapped_pages(), 1);
    }
}

#[test]
fn aad_compiles_standalone_explicit() {
    assert_standalone_admission(&AAD_BANK, 10, false);
}

#[test]
fn aad_compiles_standalone_entry() {
    assert_standalone_admission(&AAD_BANK, 10, true);
}

#[test]
fn aad_compiles_replacement_explicit() {
    assert_bound_admission(&AAD_BANK, false, false);
}

#[test]
fn aad_compiles_replacement_entry() {
    assert_bound_admission(&AAD_BANK, false, true);
}

#[test]
fn aad_compiles_resident_explicit() {
    assert_bound_admission(&AAD_BANK, true, false);
}

#[test]
fn aad_compiles_resident_entry() {
    assert_bound_admission(&AAD_BANK, true, true);
}

#[test]
fn aad_two_byte_fetch_execute_and_top_boundaries_do_not_wrap() {
    for base in [0, 0x0a, 0x66, 0x67, 0xf0, 0xf2, 0xf3, 0xff] {
        for pc in [CODE, 0x1fff, u32::MAX - 1] {
            let mut memory = code_space(pc, &[0xd5, base]);
            if pc == u32::MAX - 1 {
                let zero = PageRange::new(GuestAddress(0), 1).unwrap();
                memory.map_zeroed(zero, Permissions::ALL).unwrap();
                memory.write(GuestAddress(0), &[0x0f, 0x0b]).unwrap();
                memory.protect(zero, Permissions::READ).unwrap();
            }
            let decoded = decode_one(&memory, GuestAddress(pc)).unwrap();
            assert_eq!(
                decoded.operation(),
                &Operation::AsciiAdjustBeforeDivision { base }
            );
            assert_eq!(
                (decoded.length(), decoded.next_pc()),
                (2, GuestAddress(pc.wrapping_add(2)))
            );
            assert!(memory.is_code_current(decoded.code_snapshot()));
        }

        let mut denied_first = code_space(0x1fff, &[0xd5, base]);
        denied_first
            .protect(
                PageRange::new(GuestAddress(CODE), 1).unwrap(),
                Permissions::READ,
            )
            .unwrap();
        assert_eq!(
            decode_one(&denied_first, GuestAddress(0x1fff)).err(),
            Some(fetch_error(0x1fff, 0x1fff, 1, FaultReason::Permission))
        );

        let mut denied_second = code_space(0x1fff, &[0xd5, base]);
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

        let mut incomplete_top = code_space(u32::MAX, &[0xd5]);
        let zero = PageRange::new(GuestAddress(0), 1).unwrap();
        incomplete_top.map_zeroed(zero, Permissions::ALL).unwrap();
        incomplete_top
            .write(GuestAddress(0), &[base, 0xeb, 0])
            .unwrap();
        incomplete_top.protect(zero, Permissions::EXECUTE).unwrap();
        assert_eq!(
            decode_one(&incomplete_top, GuestAddress(u32::MAX)).err(),
            Some(fetch_error(
                u32::MAX,
                u32::MAX,
                2,
                FaultReason::AddressOverflow,
            ))
        );
    }

    let incomplete_page = code_space(0x1fff, &[0xd5]);
    assert_eq!(
        decode_one(&incomplete_page, GuestAddress(0x1fff)).err(),
        Some(fetch_error(0x1fff, 0x2000, 2, FaultReason::Unmapped))
    );
}
