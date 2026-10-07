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
            ir::{ByteRegister, EffectiveAddress, Operation, RotateKind, ShiftCount, ShiftKind},
        },
    },
    memory::{
        Access, AddressSpace, FaultReason, GuestAddress, MemoryFault, PageRange, Permissions,
    },
    process::{EngineInstance, HostError, ResidentInstallation},
};

fn admits(modrm: u8) {
    let mut memory = AddressSpace::new(1).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    let bytes = [0xd0, modrm, 0xeb, 0];
    memory.write(GuestAddress(0x1000), &bytes).unwrap();
    compile_region(
        &memory,
        &[BlockSpec {
            entry: GuestAddress(0x1000),
            byte_length: bytes.len() as u32,
        }],
        CompileLimits::default(),
    )
    .expect("register byte count-one carry rotate must compile");
}

#[test]
fn rcl_ah_one_compiles_before_jump() {
    admits(0xd4);
}

#[test]
fn rcr_bl_one_compiles_before_jump() {
    admits(0xdb);
}

const CODE: u32 = 0x1000;
const KEEP: u32 = 0x3000;
const KEY: u64 = 0x1234_5678_9abc_def0;
const KINDS: [(u8, RotateKind); 2] = [(2, RotateKind::Left), (3, RotateKind::Right)];
const DATA: u32 = 0x5000;
const REGISTERS: [ByteRegister; 8] = [
    ByteRegister::Al,
    ByteRegister::Cl,
    ByteRegister::Dl,
    ByteRegister::Bl,
    ByteRegister::Ah,
    ByteRegister::Ch,
    ByteRegister::Dh,
    ByteRegister::Bh,
];

fn upload(engine: &mut EngineInstance, pc: u32, bytes: &[u8]) {
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(bytes);
    engine.upload(pc, bytes.len() as u32).unwrap();
}

fn code(pc: u32, bytes: &[u8]) -> EngineInstance {
    let base = pc & !0xfff;
    let pages = (u64::from(pc - base) + bytes.len() as u64).div_ceil(4096) as u32;
    let mut engine = EngineInstance::new(pages + 2, KEY).unwrap();
    engine.map(base, pages, 7).unwrap();
    upload(&mut engine, pc, bytes);
    engine
}

fn describe(engine: &mut EngineInstance, pc: u32, length: u32, entries: bool) {
    let request = &mut engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8];
    request[..4].copy_from_slice(&pc.to_le_bytes());
    request[4..].copy_from_slice(&if entries { 0 } else { length }.to_le_bytes());
}

fn compile(engine: &mut EngineInstance, resident: bool, entries: bool) -> Result<u64, HostError> {
    match (resident, entries) {
        (false, false) => engine.compile(1).map(u64::from),
        (false, true) => engine.compile_entries(1, 0).map(u64::from),
        (true, false) => engine.compile_resident(1).map(|id| id.get()),
        (true, true) => engine.compile_resident_entries(1, 0).map(|id| id.get()),
    }
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

fn rejected(bytes: &[u8], expected: DecodeError) {
    let engine = code(CODE, bytes);
    let memory = engine.memory().unwrap();
    assert_eq!(
        decode_one(memory, GuestAddress(CODE)).err(),
        Some(expected),
        "{bytes:02x?}"
    );
    let error = instruction_error(CODE, expected);
    assert_eq!(
        compile_region(
            memory,
            &[BlockSpec {
                entry: GuestAddress(CODE),
                byte_length: bytes.len() as u32,
            }],
            CompileLimits::default(),
        )
        .err(),
        Some(error),
        "{bytes:02x?}"
    );
    assert_eq!(
        compile_entry_region(memory, &[GuestAddress(CODE)], CompileLimits::default()).err(),
        Some(error),
        "{bytes:02x?}"
    );
}

#[test]
fn all_sixteen_forms_have_exact_ir_and_compile_in_all_six_profiles() {
    let mut bytes = Vec::new();
    for (field, _) in KINDS {
        for destination in 0..8 {
            bytes.extend_from_slice(&[0xd0, 0xc0 | field << 3 | destination]);
        }
    }
    bytes.extend_from_slice(&[0xeb, 0]);
    assert_eq!(bytes.len(), 34);
    let limits = CompileLimits::default();
    assert_eq!(
        (limits.blocks, limits.instructions, limits.wasm_bytes),
        (8, 64, 65_536)
    );
    let mut engine = code(CODE, &bytes);
    engine.protect(CODE, 1, 4).unwrap();
    let memory = engine.memory().unwrap();
    assert!(memory.resolve(GuestAddress(CODE), Access::Read).is_err());
    for address in [0, DATA] {
        assert!(memory.resolve(GuestAddress(address), Access::Read).is_err());
    }
    let mut forms = 0;
    for (field, kind) in KINDS {
        for (destination, register) in REGISTERS.into_iter().enumerate() {
            let pc = CODE + u32::from(field - 2) * 16 + destination as u32 * 2;
            let decoded = decode_one(memory, GuestAddress(pc)).unwrap();
            assert_eq!(
                decoded.operation(),
                &Operation::ByteRotateThroughCarryOne {
                    kind,
                    destination: register,
                }
            );
            assert_eq!(
                (decoded.pc(), decoded.length(), decoded.next_pc()),
                (GuestAddress(pc), 2, GuestAddress(pc + 2))
            );
            assert!(memory.is_code_current(decoded.code_snapshot()));
            forms += 1;
        }
    }
    let mut profiles = 0;
    for unit in [
        compile_region(
            memory,
            &[BlockSpec {
                entry: GuestAddress(CODE),
                byte_length: bytes.len() as u32,
            }],
            CompileLimits::default(),
        )
        .unwrap(),
        compile_entry_region(memory, &[GuestAddress(CODE)], CompileLimits::default()).unwrap(),
    ] {
        assert_eq!(
            (unit.metadata().blocks, unit.metadata().instructions),
            (1, 17)
        );
        assert_eq!(&unit.wasm_bytes(memory).unwrap()[..8], b"\0asm\x01\0\0\0");
        profiles += 1;
    }
    for resident in [false, true] {
        for entries in [false, true] {
            let mut engine = code(CODE, &bytes);
            engine.protect(CODE, 1, 4).unwrap();
            describe(&mut engine, CODE, bytes.len() as u32, entries);
            let before = engine.arena().to_vec();
            let arena_address = engine.arena_address();
            let id = compile(&mut engine, resident, entries).unwrap();
            if resident {
                engine.guard_resident(KEY, id).unwrap();
                assert_eq!(&engine.resident_bytes(id).unwrap()[..8], b"\0asm\x01\0\0\0");
                for offset in (0..34).step_by(2) {
                    assert_eq!(engine.lookup_resident(CODE + offset).unwrap().get(), id);
                    assert!(engine.lookup_resident(CODE + offset + 1).is_err());
                }
                assert!(engine.lookup_resident(CODE + 34).is_err());
            } else {
                engine.guard(KEY, id as u32).unwrap();
                assert_eq!(&engine.artifact_bytes().unwrap()[..8], b"\0asm\x01\0\0\0");
            }
            assert_eq!(engine.arena(), before);
            assert_eq!(engine.arena_address(), arena_address);
            assert_eq!((engine.is_open(), engine.key()), (true, KEY));
            assert_eq!(engine.memory().unwrap().mapped_pages(), 1);
            for address in [0, DATA] {
                assert!(
                    engine
                        .memory()
                        .unwrap()
                        .resolve(GuestAddress(address), Access::Read)
                        .is_err()
                );
            }
            profiles += 1;
        }
    }
    assert_eq!((forms, profiles), (16, 6));
}

#[test]
fn excluded_byte_forms_and_supported_neighbors_keep_exact_ir_boundaries() {
    let opcode = DecodeError::Unsupported(UnsupportedFeature::Opcode);
    let mut exclusions = 0;
    for (field, _) in KINDS {
        let modrm = 0xc4 | field << 3;
        for prefix in [
            0x66, 0x67, 0xf2, 0xf3, 0x26, 0x2e, 0x36, 0x3e, 0x64, 0x65, 0xf0,
        ] {
            let expected = match prefix {
                0xf0 => DecodeError::InvalidEncoding,
                0x26 | 0x2e | 0x36 | 0x3e | 0x64 | 0x65 => {
                    DecodeError::Unsupported(UnsupportedFeature::Segment)
                }
                _ => opcode,
            };
            rejected(&[prefix, 0xd0, modrm], expected);
            exclusions += 1;
        }
        for tail in [
            &[0x00][..],
            &[0x40, 0x80],
            &[0x80, 0x78, 0x56, 0x34, 0x12],
            &[0x05, 0x78, 0x56, 0x34, 0x12],
            &[0x04, 0x24],
            &[0x04, 0x8a],
            &[0x04, 0x8d, 0x78, 0x56, 0x34, 0x12],
        ] {
            let mut bytes = vec![0xd0];
            bytes.extend_from_slice(tail);
            bytes[1] |= field << 3;
            bytes.insert(0, 0x66);
            rejected(&bytes, opcode);
            exclusions += 1;
        }
        for count in [0, 1, 2, 8, 31, 32, 33, 65, 97, 129, 161, 193, 225, 255] {
            let mut bytes = vec![0xc0, modrm, count];
            if count & 31 == 1 {
                bytes.insert(0, 0x66);
            }
            rejected(&bytes, opcode);
            exclusions += 1;
        }
        for count in [1, 33] {
            rejected(&[0x66, 0xc0, 0x03 | field << 3, count], opcode);
            exclusions += 1;
        }
        for modrm in [modrm, 0x03 | field << 3] {
            let mut cl = vec![0xd2, modrm];
            if modrm & 0xc0 == 0xc0 {
                cl.insert(0, 0x66);
            }
            rejected(&cl, opcode);
            rejected(&[0x66, 0xd1, modrm], opcode);
            exclusions += 2;
        }
    }
    let mut engine = code(CODE, &[0x90]);
    let ea = EffectiveAddress {
        base: Some(Register32::Ebx),
        index: None,
        scale: 1,
        displacement: 0,
    };
    let mut neighbors = 0;
    for (field, kind) in [(0, RotateKind::Left), (1, RotateKind::Right)] {
        for (index, destination) in REGISTERS.into_iter().enumerate() {
            let bytes = [0xd0, 0xc0 | field << 3 | index as u8];
            upload(&mut engine, CODE, &bytes);
            let decoded = decode_one(engine.memory().unwrap(), GuestAddress(CODE)).unwrap();
            assert_eq!(
                decoded.operation(),
                &Operation::ByteRotateOne { kind, destination }
            );
            assert_eq!(
                (decoded.length(), decoded.next_pc()),
                (2, GuestAddress(CODE + 2))
            );
            neighbors += 1;
        }
        let bytes = [0xd0, 0x03 | field << 3];
        upload(&mut engine, CODE, &bytes);
        let decoded = decode_one(engine.memory().unwrap(), GuestAddress(CODE)).unwrap();
        assert_eq!(
            decoded.operation(),
            &Operation::MemoryByteRotateOne { kind, address: ea }
        );
        assert_eq!(
            (decoded.length(), decoded.next_pc()),
            (2, GuestAddress(CODE + 2))
        );
        neighbors += 1;
        for memory in [false, true] {
            let modrm = (if memory { 3 } else { 0xc3 }) | field << 3;
            let operation = if memory {
                Operation::MemoryByteRotateOne { kind, address: ea }
            } else {
                Operation::ByteRotateOne {
                    kind,
                    destination: ByteRegister::Bl,
                }
            };
            for count in [1, 33] {
                let bytes = [0xc0, modrm, count];
                upload(&mut engine, CODE, &bytes);
                let decoded = decode_one(engine.memory().unwrap(), GuestAddress(CODE)).unwrap();
                assert_eq!(decoded.operation(), &operation, "{bytes:02x?}");
                assert_eq!(
                    (decoded.length(), decoded.next_pc()),
                    (3, GuestAddress(CODE + 3))
                );
                neighbors += 1;
            }
        }
    }
    for (field, kind) in [
        (0, RotateKind::Left),
        (1, RotateKind::Right),
        (2, RotateKind::Left),
        (3, RotateKind::Right),
    ] {
        for memory in [false, true] {
            let modrm = (if memory { 3 } else { 0xc3 }) | field << 3;
            let operation = match (field < 2, memory) {
                (true, false) => Operation::RotateOne {
                    kind,
                    destination: Register32::Ebx,
                },
                (true, true) => Operation::MemoryRotateOne { kind, address: ea },
                (false, false) => Operation::RotateThroughCarryOne {
                    kind,
                    destination: Register32::Ebx,
                },
                (false, true) => Operation::MemoryRotateThroughCarryOne { kind, address: ea },
            };
            for bytes in [
                vec![0xd1, modrm],
                vec![0xc1, modrm, 1],
                vec![0xc1, modrm, 33],
            ] {
                upload(&mut engine, CODE, &bytes);
                let decoded = decode_one(engine.memory().unwrap(), GuestAddress(CODE)).unwrap();
                assert_eq!(decoded.operation(), &operation, "{bytes:02x?}");
                assert_eq!(decoded.length() as usize, bytes.len());
                assert_eq!(decoded.next_pc(), GuestAddress(CODE + bytes.len() as u32));
                neighbors += 1;
            }
        }
    }
    for (field, kind) in [
        (4, ShiftKind::Shl),
        (5, ShiftKind::Shr),
        (7, ShiftKind::Sar),
    ] {
        for (index, destination) in REGISTERS.into_iter().enumerate() {
            let bytes = [0xd0, 0xc0 | field << 3 | index as u8];
            upload(&mut engine, CODE, &bytes);
            let decoded = decode_one(engine.memory().unwrap(), GuestAddress(CODE)).unwrap();
            assert_eq!(
                decoded.operation(),
                &Operation::ShiftByte {
                    kind,
                    destination,
                    count: ShiftCount::Immediate(1)
                }
            );
            assert_eq!(
                (decoded.length(), decoded.next_pc()),
                (2, GuestAddress(CODE + 2))
            );
            neighbors += 1;
        }
    }
    assert_eq!((exclusions, neighbors), (76, 74));
}

#[test]
fn exact_two_byte_fetch_and_same_byte_snapshots_preserve_page_and_wrap_boundaries() {
    let mut complete = 0;
    let mut truncated = 0;
    let mut permissions = 0;
    let mut invalidations = 0;
    for (field, kind) in KINDS {
        let bytes = [0xd0, 0xc4 | field << 3];
        for pc in [0x1ffe, u32::MAX - 1, 0x1fff] {
            let mut engine = code(pc, &bytes);
            engine
                .protect(pc & !0xfff, if pc == 0x1fff { 2 } else { 1 }, 4)
                .unwrap();
            let memory = engine.memory().unwrap();
            let decoded = decode_one(memory, GuestAddress(pc)).unwrap();
            assert_eq!(
                decoded.operation(),
                &Operation::ByteRotateThroughCarryOne {
                    kind,
                    destination: ByteRegister::Ah
                }
            );
            assert_eq!(
                (decoded.pc(), decoded.length(), decoded.next_pc()),
                (GuestAddress(pc), 2, GuestAddress(pc.wrapping_add(2)))
            );
            assert!(memory.is_code_current(decoded.code_snapshot()));
            assert_eq!(
                compile_region(
                    memory,
                    &[BlockSpec {
                        entry: GuestAddress(pc),
                        byte_length: 2
                    }],
                    CompileLimits::default()
                )
                .unwrap()
                .metadata()
                .instructions,
                1
            );
            if pc == 0x1ffe {
                assert!(
                    memory
                        .resolve(GuestAddress(0x2000), Access::Execute)
                        .is_err()
                );
            }
            if pc == u32::MAX - 1 {
                assert!(memory.resolve(GuestAddress(0), Access::Execute).is_err());
            }
            complete += 1;
        }
        for (pc, address, reason) in [
            (0x1fff, 0x2000, FaultReason::Unmapped),
            (u32::MAX, u32::MAX, FaultReason::AddressOverflow),
        ] {
            let engine = code(pc, &[0xd0]);
            let memory = engine.memory().unwrap();
            let error = fetch_error(pc, address, 2, reason);
            assert_eq!(decode_one(memory, GuestAddress(pc)).err(), Some(error));
            assert_eq!(
                compile_region(
                    memory,
                    &[BlockSpec {
                        entry: GuestAddress(pc),
                        byte_length: 2,
                    }],
                    CompileLimits::default(),
                )
                .err(),
                Some(if pc == u32::MAX {
                    CompileError::InvalidBlocks
                } else {
                    instruction_error(pc, error)
                })
            );
            assert_eq!(
                compile_entry_region(memory, &[GuestAddress(pc)], CompileLimits::default()).err(),
                Some(instruction_error(pc, error))
            );
            truncated += 1;
        }
        let mut engine = code(0x1fff, &bytes);
        engine.protect(0x2000, 1, 3).unwrap();
        let memory = engine.memory().unwrap();
        let error = fetch_error(0x1fff, 0x2000, 2, FaultReason::Permission);
        assert_eq!(decode_one(memory, GuestAddress(0x1fff)).err(), Some(error));
        for result in [
            compile_region(
                memory,
                &[BlockSpec {
                    entry: GuestAddress(0x1fff),
                    byte_length: 2,
                }],
                CompileLimits::default(),
            ),
            compile_entry_region(memory, &[GuestAddress(0x1fff)], CompileLimits::default()),
        ] {
            assert_eq!(result.err(), Some(instruction_error(0x1fff, error)));
        }
        permissions += 1;
        for changed in [0x1fff, 0x2000] {
            let mut engine = code(0x1fff, &bytes);
            let decoded = decode_one(engine.memory().unwrap(), GuestAddress(0x1fff)).unwrap();
            describe(&mut engine, 0x1fff, 2, false);
            let generation = engine.compile(1).unwrap();
            let id = engine.compile_resident(1).unwrap().get();
            engine.guard(KEY, generation).unwrap();
            engine.guard_resident(KEY, id).unwrap();
            upload(
                &mut engine,
                changed,
                &[if changed == 0x1fff { 0xd0 } else { bytes[1] }],
            );
            assert!(
                !engine
                    .memory()
                    .unwrap()
                    .is_code_current(decoded.code_snapshot())
            );
            assert_eq!(
                engine.guard(KEY, generation),
                Err(HostError::CodeInvalidated)
            );
            assert_eq!(
                engine.guard_resident(KEY, id),
                Err(HostError::Resident(RegistryError::CodeInvalidated))
            );
            invalidations += 1;
        }
    }
    assert_eq!(
        (complete, truncated, permissions, invalidations),
        (6, 4, 2, 4)
    );
}

#[derive(Debug, PartialEq, Eq)]
struct Publication {
    arena: Vec<u8>,
    pointer: usize,
    generation: u32,
    authority: (bool, u64),
    entries: [u64; 2],
    modules: [(Vec<u8>, usize); 3],
    pages: [Vec<u8>; 3],
    mapped: u32,
    installed: ResidentInstallation,
}

fn publication(engine: &EngineInstance, id: u64) -> Publication {
    Publication {
        arena: engine.arena().to_vec(),
        pointer: engine.arena_address(),
        generation: engine.generation(),
        authority: (engine.is_open(), engine.key()),
        entries: [KEEP, KEEP + 1].map(|pc| engine.lookup_resident(pc).unwrap().get()),
        modules: [
            engine.artifact_bytes().unwrap(),
            engine.resident_bytes(id).unwrap(),
            engine.dispatcher_bytes(KEY).unwrap(),
        ]
        .map(|bytes| (bytes.to_vec(), bytes.as_ptr() as usize)),
        pages: [CODE, KEEP, DATA].map(|pc| {
            let mut bytes = vec![0; 4096];
            engine
                .memory()
                .unwrap()
                .read(GuestAddress(pc), &mut bytes)
                .unwrap();
            bytes
        }),
        mapped: engine.memory().unwrap().mapped_pages(),
        installed: engine.lookup_installed_resident(KEY, KEEP).unwrap(),
    }
}

#[test]
fn late_byte_carry_rotate_failures_preserve_both_publications_and_installation() {
    let opcode = DecodeError::Unsupported(UnsupportedFeature::Opcode);
    let mut refusals = 0;
    for (field, _) in KINDS {
        let modrm = 0xc4 | field << 3;
        for (pc, bytes, declared, expected) in [
            (
                CODE,
                vec![0xd0, modrm, 0x0f, 0x0b],
                4,
                instruction_error(CODE + 2, opcode),
            ),
            (
                CODE,
                vec![0xd0, modrm, 0x66, 0xd0, modrm ^ 8],
                5,
                instruction_error(CODE + 2, opcode),
            ),
            (
                0x1ffd,
                vec![0xd0, modrm, 0xd0],
                4,
                instruction_error(
                    0x1fff,
                    fetch_error(0x1fff, 0x2000, 2, FaultReason::Unmapped),
                ),
            ),
        ] {
            for resident in [false, true] {
                for entries in [false, true] {
                    let mut engine = code(pc, &bytes);
                    engine.map(KEEP, 1, 7).unwrap();
                    upload(&mut engine, KEEP, &[0x90, 0xeb, 0]);
                    engine.map(DATA, 1, 3).unwrap();
                    engine.write32(DATA + 0x10, 0x9234_5678).unwrap();
                    describe(&mut engine, KEEP, 3, false);
                    engine.compile(1).unwrap();
                    let keep = engine.compile_resident(1).unwrap().get();
                    engine
                        .acknowledge_resident_installation(KEY, keep, 3)
                        .unwrap();
                    describe(&mut engine, pc, declared, entries);
                    let snapshot = engine
                        .memory()
                        .unwrap()
                        .snapshot_code(GuestAddress(CODE), 4096)
                        .unwrap();
                    let before = publication(&engine, keep);
                    let expected = if resident {
                        HostError::Resident(RegistryError::Compile(expected))
                    } else {
                        HostError::Compile(expected)
                    };
                    assert_eq!(compile(&mut engine, resident, entries), Err(expected));
                    assert_eq!(publication(&engine, keep), before);
                    assert!(engine.memory().unwrap().is_code_current(&snapshot));
                    engine.guard(KEY, before.generation).unwrap();
                    engine.guard_resident(KEY, keep).unwrap();
                    assert_eq!(engine.lookup_resident(KEEP).unwrap().get(), keep);
                    assert!(engine.lookup_resident(KEEP + 2).is_err());
                    assert!(engine.lookup_resident(pc).is_err());
                    refusals += 1;
                }
            }
        }
    }
    assert_eq!(refusals, 24);
}
