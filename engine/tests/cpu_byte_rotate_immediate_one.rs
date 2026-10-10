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

#[test]
fn register_byte_rol_immediate_one_compiles_before_jump() {
    let mut memory = AddressSpace::new(1).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    let bytes = [0xc0, 0xc4, 1, 0xeb, 0];
    memory.write(GuestAddress(0x1000), &bytes).unwrap();
    compile_region(
        &memory,
        &[BlockSpec {
            entry: GuestAddress(0x1000),
            byte_length: bytes.len() as u32,
        }],
        CompileLimits::default(),
    )
    .expect("register byte rotate with masked-one immediate must compile");
}

#[test]
fn memory_byte_ror_immediate_thirty_three_admits_with_unmapped_data() {
    let mut engine = EngineInstance::new(1, 0x1234_5678_9abc_def0).unwrap();
    let bytes = [0xc0, 0x0b, 0x21, 0xeb, 0];
    engine.map(0x1000, 1, 7).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(&bytes);
    engine.upload(0x1000, bytes.len() as u32).unwrap();
    assert!(
        engine
            .memory()
            .unwrap()
            .resolve(GuestAddress(0), Access::Read)
            .is_err()
    );
    let request = &mut engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8];
    request[..4].copy_from_slice(&0x1000_u32.to_le_bytes());
    request[4..].copy_from_slice(&(bytes.len() as u32).to_le_bytes());
    engine
        .compile(1)
        .expect("memory byte rotate with masked-one immediate must admit without reading data");
}

const CODE: u32 = 0x1000;
const KEEP: u32 = 0x3000;
const DATA: u32 = 0x5000;
const KEY: u64 = 0x1234_5678_9abc_def0;
const KINDS: [RotateKind; 2] = [RotateKind::Left, RotateKind::Right];
const REGISTERS: [Register32; 8] = [
    Register32::Eax,
    Register32::Ecx,
    Register32::Edx,
    Register32::Ebx,
    Register32::Esp,
    Register32::Ebp,
    Register32::Esi,
    Register32::Edi,
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

fn address(
    base: Option<Register32>,
    index: Option<Register32>,
    scale: u8,
    displacement: u32,
) -> EffectiveAddress {
    EffectiveAddress {
        base,
        index,
        scale,
        displacement,
    }
}

fn encoding(kind: RotateKind, tail: &[u8], count: u8) -> Vec<u8> {
    let mut bytes = vec![0xc0];
    bytes.extend_from_slice(tail);
    if kind == RotateKind::Right {
        bytes[1] |= 8;
    }
    bytes.push(count);
    bytes
}

fn memory_forms(kind: RotateKind, count: u8) -> Vec<(Vec<u8>, EffectiveAddress)> {
    let mut forms = Vec::new();
    for (index, base) in REGISTERS.into_iter().enumerate() {
        let tail = match base {
            Register32::Esp => vec![0x04, 0x24],
            Register32::Ebp => vec![0x45, 0],
            _ => vec![index as u8],
        };
        forms.push((
            encoding(kind, &tail, count),
            address(Some(base), None, 1, 0),
        ));
    }
    for (tail, expected) in [
        (
            &[0x40, 0x80][..],
            address(Some(Register32::Eax), None, 1, 0xffff_ff80),
        ),
        (
            &[0x82, 0x78, 0x56, 0x34, 0x12],
            address(Some(Register32::Edx), None, 1, 0x1234_5678),
        ),
        (
            &[0x44, 0x90, 0xe0],
            address(Some(Register32::Eax), Some(Register32::Edx), 4, 0xffff_ffe0),
        ),
        (
            &[0x84, 0xc2, 0x78, 0x56, 0x34, 0x12],
            address(Some(Register32::Edx), Some(Register32::Eax), 8, 0x1234_5678),
        ),
        (
            &[0x04, 0x85, 0x10, 0x50, 0, 0],
            address(None, Some(Register32::Eax), 4, DATA + 0x10),
        ),
        (
            &[0x05, 0x10, 0x50, 0, 0],
            address(None, None, 1, DATA + 0x10),
        ),
    ] {
        forms.push((encoding(kind, tail, count), expected));
    }
    forms
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

const COUNTS: [u8; 8] = [1, 33, 65, 97, 129, 161, 193, 225];

const BYTE_REGISTERS: [ByteRegister; 8] = [
    ByteRegister::Al,
    ByteRegister::Cl,
    ByteRegister::Dl,
    ByteRegister::Bl,
    ByteRegister::Ah,
    ByteRegister::Ch,
    ByteRegister::Dh,
    ByteRegister::Bh,
];

fn register_bytes(kind: RotateKind, destination: ByteRegister, count: u8) -> [u8; 3] {
    let index = BYTE_REGISTERS
        .iter()
        .position(|register| *register == destination)
        .unwrap();
    [
        0xc0,
        0xc0 | (u8::from(kind == RotateKind::Right) << 3) | index as u8,
        count,
    ]
}

fn assert_instruction(memory: &AddressSpace, pc: u32, length: usize, operation: Operation) {
    let decoded = decode_one(memory, GuestAddress(pc)).unwrap();
    assert_eq!(decoded.operation(), &operation);
    assert_eq!(
        (decoded.pc(), decoded.length() as usize, decoded.next_pc()),
        (GuestAddress(pc), length, GuestAddress(pc + length as u32))
    );
    assert!(memory.is_code_current(decoded.code_snapshot()));
}

fn bound_bank(bytes: &[u8], starts: &[u32]) -> usize {
    assert!(starts.len() <= CompileLimits::default().instructions);
    let mut profiles = 0;
    for resident in [false, true] {
        for entries in [false, true] {
            let mut engine = code(CODE, bytes);
            engine.protect(CODE, 1, 4).unwrap();
            describe(&mut engine, CODE, bytes.len() as u32, entries);
            let before = engine.arena().to_vec();
            let pointer = engine.arena_address();
            let snapshot = engine
                .memory()
                .unwrap()
                .snapshot_code(GuestAddress(CODE), bytes.len())
                .unwrap();
            let id = compile(&mut engine, resident, entries).unwrap();
            if resident {
                engine.guard_resident(KEY, id).unwrap();
                let module = engine.resident_bytes(id).unwrap();
                assert_eq!(&module[..8], b"\0asm\x01\0\0\0");
                assert!(module.len() <= CompileLimits::default().wasm_bytes);
                for (index, &pc) in starts.iter().enumerate() {
                    assert_eq!(engine.lookup_resident(pc).unwrap().get(), id);
                    let end = starts
                        .get(index + 1)
                        .copied()
                        .unwrap_or(CODE + bytes.len() as u32);
                    for interior in pc + 1..end {
                        assert!(engine.lookup_resident(interior).is_err());
                    }
                }
                assert!(engine.lookup_resident(CODE + bytes.len() as u32).is_err());
            } else {
                engine.guard(KEY, id as u32).unwrap();
                let module = engine.artifact_bytes().unwrap();
                assert_eq!(&module[..8], b"\0asm\x01\0\0\0");
                assert!(module.len() <= CompileLimits::default().wasm_bytes);
            }
            assert_eq!(engine.arena(), before);
            assert_eq!(engine.arena_address(), pointer);
            assert!(engine.memory().unwrap().is_code_current(&snapshot));
            assert_eq!(engine.memory().unwrap().mapped_pages(), 1);
            for data in [0, DATA] {
                assert!(
                    engine
                        .memory()
                        .unwrap()
                        .resolve(GuestAddress(data), Access::Read)
                        .is_err()
                );
            }
            profiles += 1;
        }
    }
    profiles
}

#[test]
fn all_register_byte_immediates_and_destinations_have_exact_ir_in_six_profiles() {
    let limits = CompileLimits::default();
    assert_eq!(
        (limits.blocks, limits.instructions, limits.wasm_bytes),
        (8, 64, 65_536)
    );
    let mut forms = 0;
    let mut profiles = 0;
    for kind in KINDS {
        for counts in COUNTS.chunks(4) {
            let mut bytes = Vec::new();
            let mut expected = Vec::new();
            let mut starts = Vec::new();
            for &count in counts {
                for destination in BYTE_REGISTERS {
                    let pc = CODE + bytes.len() as u32;
                    starts.push(pc);
                    expected.push((pc, Operation::ByteRotateOne { kind, destination }));
                    bytes.extend_from_slice(&register_bytes(kind, destination, count));
                }
            }
            assert_eq!(expected.len(), 32);
            starts.push(CODE + bytes.len() as u32);
            bytes.extend_from_slice(&[0xeb, 0]);
            assert_eq!((starts.len(), bytes.len()), (33, 98));
            let mut engine = code(CODE, &bytes);
            engine.protect(CODE, 1, 4).unwrap();
            let memory = engine.memory().unwrap();
            for (pc, operation) in expected {
                assert_instruction(memory, pc, 3, operation);
                forms += 1;
            }
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
                compile_entry_region(memory, &[GuestAddress(CODE)], CompileLimits::default())
                    .unwrap(),
            ] {
                assert_eq!(
                    (unit.metadata().blocks, unit.metadata().instructions),
                    (1, 33)
                );
                assert_eq!(&unit.wasm_bytes(memory).unwrap()[..8], b"\0asm\x01\0\0\0");
                assert!(unit.wasm_bytes(memory).unwrap().len() <= limits.wasm_bytes);
                profiles += 1;
            }
            profiles += bound_bank(&bytes, &starts);
        }
    }
    assert_eq!((forms, profiles), (128, 24));
}

#[test]
fn all_memory_byte_immediates_and_addresses_have_exact_ir_in_bound_profiles() {
    let mut forms = 0;
    let mut profiles = 0;
    for kind in KINDS {
        for counts in COUNTS.chunks(2) {
            let mut bytes = Vec::new();
            let mut expected = Vec::new();
            let mut starts = Vec::new();
            for &count in counts {
                for (form, address) in memory_forms(kind, count) {
                    let pc = CODE + bytes.len() as u32;
                    starts.push(pc);
                    expected.push((
                        pc,
                        form.len(),
                        Operation::MemoryByteRotateOne { kind, address },
                    ));
                    bytes.extend_from_slice(&form);
                }
            }
            assert_eq!(expected.len(), 28);
            starts.push(CODE + bytes.len() as u32);
            bytes.extend_from_slice(&[0xeb, 0]);
            assert_eq!((starts.len(), bytes.len()), (29, 132));
            let engine = code(CODE, &bytes);
            for (pc, length, operation) in expected {
                assert_instruction(engine.memory().unwrap(), pc, length, operation);
                forms += 1;
            }
            profiles += bound_bank(&bytes, &starts);
        }
    }
    let unsupported = CompileError::Instruction {
        pc: GuestAddress(CODE),
        cause: InstructionError::BackendUnsupported,
    };
    let mut refusals = 0;
    for kind in KINDS {
        let bytes = encoding(kind, &[0x03], 33);
        let engine = code(CODE, &bytes);
        let memory = engine.memory().unwrap();
        assert!(memory.resolve(GuestAddress(0), Access::Read).is_err());
        for observed in [
            compile_region(
                memory,
                &[BlockSpec {
                    entry: GuestAddress(CODE),
                    byte_length: bytes.len() as u32,
                }],
                CompileLimits::default(),
            )
            .err(),
            compile_entry_region(memory, &[GuestAddress(CODE)], CompileLimits::default()).err(),
        ] {
            assert_eq!(observed, Some(unsupported), "{kind:?}");
            refusals += 1;
        }
    }
    assert_eq!((forms, profiles, refusals), (224, 32, 4));
}

#[test]
fn raw_byte_immediate_domain_and_strict_neighbors_preserve_decode_boundaries() {
    let mut engine = code(CODE, &[0x90]);
    let mut accepted = 0;
    let refused = 0;
    let (mut old, mut new) = (0, 0);
    let opcode = DecodeError::Unsupported(UnsupportedFeature::Opcode);
    let ea = address(Some(Register32::Ebx), None, 1, 0);
    for kind in KINDS {
        for memory in [false, true] {
            for count in 0..=u8::MAX {
                let bytes = if memory {
                    encoding(kind, &[0x03], count)
                } else {
                    register_bytes(kind, ByteRegister::Ah, count).to_vec()
                };
                upload(&mut engine, CODE, &bytes);
                if memory || COUNTS.contains(&count) {
                    let operation = match (memory, COUNTS.contains(&count)) {
                        (true, true) => Operation::MemoryByteRotateOne { kind, address: ea },
                        (true, false) => Operation::MemoryByteRotate {
                            kind,
                            address: ea,
                            count: ShiftCount::Immediate(count),
                        },
                        (false, _) => Operation::ByteRotateOne {
                            kind,
                            destination: ByteRegister::Ah,
                        },
                    };
                    assert_instruction(engine.memory().unwrap(), CODE, 3, operation);
                    accepted += 1;
                    old += 1;
                } else {
                    assert_instruction(
                        engine.memory().unwrap(),
                        CODE,
                        3,
                        Operation::ByteRotateImmediate {
                            kind,
                            destination: ByteRegister::Ah,
                            count,
                        },
                    );
                    accepted += 1;
                    new += 1;
                }
            }
        }
    }
    assert_eq!((accepted, refused, old, new), (1024, 0, 528, 496));
    let mut exclusions = 0;
    for kind in KINDS {
        let field = u8::from(kind == RotateKind::Right) << 3;
        for memory in [false, true] {
            let modrm = (if memory { 3 } else { 0xc4 }) | field;
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
                rejected(&[prefix, 0xc0, modrm, 33], expected);
                exclusions += 1;
            }
            let mut cl = vec![0xd2, modrm];
            cl.insert(0, 0x66);
            rejected(&cl, opcode);
            let word = vec![0x66, 0x66, 0xc1, modrm, 1];
            rejected(&word, opcode);
            exclusions += 2;
        }
    }
    for field in [2, 3] {
        for tail in [&[0xc0][..], &[0xc4], &[0x03], &[0x04, 0x24]] {
            for count in COUNTS {
                let mut bytes = vec![0xc0];
                bytes.extend_from_slice(tail);
                bytes[1] |= field << 3;
                bytes.push(count);
                bytes.insert(0, 0x66);
                rejected(&bytes, opcode);
                exclusions += 1;
            }
        }
    }
    let mut neighbors = 0;
    for kind in KINDS {
        for destination in BYTE_REGISTERS {
            let mut bytes = register_bytes(kind, destination, 1).to_vec();
            assert_eq!(bytes.pop(), Some(1));
            bytes[0] = 0xd0;
            upload(&mut engine, CODE, &bytes);
            assert_instruction(
                engine.memory().unwrap(),
                CODE,
                2,
                Operation::ByteRotateOne { kind, destination },
            );
            neighbors += 1;
        }
        let bytes = [0xd0, 0x03 | (u8::from(kind == RotateKind::Right) << 3)];
        upload(&mut engine, CODE, &bytes);
        assert_instruction(
            engine.memory().unwrap(),
            CODE,
            2,
            Operation::MemoryByteRotateOne { kind, address: ea },
        );
        neighbors += 1;
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
                assert_instruction(engine.memory().unwrap(), CODE, bytes.len(), operation);
                neighbors += 1;
            }
        }
    }
    for (field, kind) in [
        (4, ShiftKind::Shl),
        (5, ShiftKind::Shr),
        (7, ShiftKind::Sar),
    ] {
        for (index, destination) in BYTE_REGISTERS.into_iter().enumerate() {
            let bytes = [0xd0, 0xc0 | field << 3 | index as u8];
            upload(&mut engine, CODE, &bytes);
            assert_instruction(
                engine.memory().unwrap(),
                CODE,
                2,
                Operation::ShiftByte {
                    kind,
                    destination,
                    count: ShiftCount::Immediate(1),
                },
            );
            neighbors += 1;
        }
    }
    assert_eq!((exclusions, neighbors), (116, 66));
}

#[test]
fn final_byte_immediate_fetch_and_count_snapshots_preserve_page_and_wrap_boundaries() {
    let mut complete = 0;
    let mut truncated = 0;
    let mut permissions = 0;
    let mut invalidations = 0;
    for kind in KINDS {
        for (bytes, operation) in [
            (
                register_bytes(kind, ByteRegister::Ah, 225).to_vec(),
                Operation::ByteRotateOne {
                    kind,
                    destination: ByteRegister::Ah,
                },
            ),
            (
                encoding(kind, &[0x05, 0x10, 0x50, 0, 0], 33),
                Operation::MemoryByteRotateOne {
                    kind,
                    address: address(None, None, 1, DATA + 0x10),
                },
            ),
            (
                encoding(kind, &[0x84, 0xc2, 0x78, 0x56, 0x34, 0x12], 129),
                Operation::MemoryByteRotateOne {
                    kind,
                    address: address(Some(Register32::Edx), Some(Register32::Eax), 8, 0x1234_5678),
                },
            ),
        ] {
            for pc in [
                0x2000 - bytes.len() as u32,
                u32::MAX - (bytes.len() as u32 - 1),
                0x1fff,
            ] {
                let mut engine = code(pc, &bytes);
                engine
                    .protect(pc & !0xfff, if pc == 0x1fff { 2 } else { 1 }, 4)
                    .unwrap();
                let memory = engine.memory().unwrap();
                let decoded = decode_one(memory, GuestAddress(pc)).unwrap();
                assert_eq!(decoded.operation(), &operation);
                assert_eq!(
                    (decoded.length() as usize, decoded.next_pc()),
                    (
                        bytes.len(),
                        GuestAddress(pc.wrapping_add(bytes.len() as u32))
                    )
                );
                assert!(memory.is_code_current(decoded.code_snapshot()));
                if pc == 0x2000 - bytes.len() as u32 {
                    assert!(
                        memory
                            .resolve(GuestAddress(0x2000), Access::Execute)
                            .is_err()
                    );
                }
                if pc == u32::MAX - (bytes.len() as u32 - 1) {
                    assert!(memory.resolve(GuestAddress(0), Access::Execute).is_err());
                }
                describe(&mut engine, pc, bytes.len() as u32, false);
                let before = engine.arena().to_vec();
                let generation = engine.compile(1).unwrap();
                let resident = engine.compile_resident(1).unwrap().get();
                engine.guard(KEY, generation).unwrap();
                engine.guard_resident(KEY, resident).unwrap();
                assert_eq!(engine.arena(), before);
                complete += 1;
            }
            let missing = &bytes[..bytes.len() - 1];
            for top in [false, true] {
                let pc = if top {
                    ((1_u64 << 32) - missing.len() as u64) as u32
                } else {
                    0x2000 - missing.len() as u32
                };
                let engine = code(pc, missing);
                let memory = engine.memory().unwrap();
                let error = fetch_error(
                    pc,
                    if top { pc } else { 0x2000 },
                    bytes.len() as u32,
                    if top {
                        FaultReason::AddressOverflow
                    } else {
                        FaultReason::Unmapped
                    },
                );
                assert_eq!(decode_one(memory, GuestAddress(pc)).err(), Some(error));
                assert_eq!(
                    compile_region(
                        memory,
                        &[BlockSpec {
                            entry: GuestAddress(pc),
                            byte_length: bytes.len() as u32
                        }],
                        CompileLimits::default()
                    )
                    .err(),
                    Some(if top {
                        CompileError::InvalidBlocks
                    } else {
                        instruction_error(pc, error)
                    })
                );
                assert_eq!(
                    compile_entry_region(memory, &[GuestAddress(pc)], CompileLimits::default())
                        .err(),
                    Some(instruction_error(pc, error))
                );
                truncated += 1;
            }
            let pc = 0x2001 - bytes.len() as u32;
            let mut engine = code(pc, &bytes);
            engine.protect(0x2000, 1, 3).unwrap();
            let memory = engine.memory().unwrap();
            let error = fetch_error(pc, 0x2000, bytes.len() as u32, FaultReason::Permission);
            assert_eq!(decode_one(memory, GuestAddress(pc)).err(), Some(error));
            for result in [
                compile_region(
                    memory,
                    &[BlockSpec {
                        entry: GuestAddress(pc),
                        byte_length: bytes.len() as u32,
                    }],
                    CompileLimits::default(),
                ),
                compile_entry_region(memory, &[GuestAddress(pc)], CompileLimits::default()),
            ] {
                assert_eq!(result.err(), Some(instruction_error(pc, error)));
            }
            permissions += 1;
        }
        for (pc, bytes) in [
            (0x1ffd, register_bytes(kind, ByteRegister::Ah, 1).to_vec()),
            (0x1ffa, encoding(kind, &[0x05, 0x10, 0x50, 0, 0], 1)),
        ] {
            for new_count in [1, 33] {
                let mut engine = code(pc, &bytes);
                let decoded = decode_one(engine.memory().unwrap(), GuestAddress(pc)).unwrap();
                describe(&mut engine, pc, bytes.len() as u32, false);
                let generation = engine.compile(1).unwrap();
                let id = engine.compile_resident(1).unwrap().get();
                engine.guard(KEY, generation).unwrap();
                engine.guard_resident(KEY, id).unwrap();
                upload(&mut engine, pc + bytes.len() as u32 - 1, &[new_count]);
                let fresh = decode_one(engine.memory().unwrap(), GuestAddress(pc)).unwrap();
                assert_eq!(fresh.operation(), decoded.operation());
                assert!(
                    engine
                        .memory()
                        .unwrap()
                        .is_code_current(fresh.code_snapshot())
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
    }
    assert_eq!(
        (complete, truncated, permissions, invalidations),
        (18, 12, 6, 8)
    );
}

#[test]
fn data_only_changes_keep_both_byte_immediate_owners_current() {
    let mut scenarios = 0;
    for kind in KINDS {
        let mut bytes = encoding(kind, &[0x05, 0x10, 0x50, 0, 0], 33);
        bytes.extend([0xeb, 0]);
        let mut engine = code(CODE, &bytes);
        let decoded = decode_one(engine.memory().unwrap(), GuestAddress(CODE)).unwrap();
        describe(&mut engine, CODE, bytes.len() as u32, false);
        let generation = engine.compile(1).unwrap();
        let resident = engine.compile_resident(1).unwrap().get();
        let before = (
            engine.artifact_bytes().unwrap().to_vec(),
            engine.resident_bytes(resident).unwrap().to_vec(),
        );
        engine.map(DATA, 1, 3).unwrap();
        engine.write8(DATA + 0x10, 0x80).unwrap();
        engine.protect(DATA, 1, 1).unwrap();
        for phase in 0..3 {
            if phase == 1 {
                engine.unmap(DATA, 1).unwrap();
            } else if phase == 2 {
                engine.map(DATA, 1, 3).unwrap();
                engine.write8(DATA + 0x10, 0x81).unwrap();
                engine.protect(DATA, 1, 1).unwrap();
            }
            engine.guard(KEY, generation).unwrap();
            engine.guard_resident(KEY, resident).unwrap();
            assert_eq!(
                (
                    engine.artifact_bytes().unwrap().to_vec(),
                    engine.resident_bytes(resident).unwrap().to_vec()
                ),
                before
            );
            assert!(
                engine
                    .memory()
                    .unwrap()
                    .is_code_current(decoded.code_snapshot())
            );
        }
        upload(&mut engine, CODE + 6, &[1]);
        let fresh = decode_one(engine.memory().unwrap(), GuestAddress(CODE)).unwrap();
        assert_eq!(fresh.operation(), decoded.operation());
        assert!(
            engine
                .memory()
                .unwrap()
                .is_code_current(fresh.code_snapshot())
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
            engine.guard_resident(KEY, resident),
            Err(HostError::Resident(RegistryError::CodeInvalidated))
        );
        scenarios += 1;
    }
    assert_eq!(scenarios, 2);
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
fn late_byte_immediate_failures_preserve_both_publications_and_installation() {
    let opcode = DecodeError::Unsupported(UnsupportedFeature::Opcode);
    let mut refusals = 0;
    for kind in KINDS {
        let field = u8::from(kind == RotateKind::Right) << 3;
        let register = 0xc4 | field;
        let memory = 0x03 | field;
        for (pc, bytes, declared, expected) in [
            (
                CODE,
                vec![0xc0, memory, 33, 0x0f, 0x0b],
                5,
                instruction_error(CODE + 3, opcode),
            ),
            (
                CODE,
                vec![0xc0, register, 33, 0x66, 0xc0, register ^ 8, 1],
                7,
                instruction_error(CODE + 3, opcode),
            ),
            (
                0x1ffb,
                vec![0xc0, register, 33, 0xc0, register ^ 8],
                6,
                instruction_error(
                    0x1ffe,
                    fetch_error(0x1ffe, 0x2000, 3, FaultReason::Unmapped),
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
