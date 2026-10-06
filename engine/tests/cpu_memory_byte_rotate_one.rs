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
    memory::{Access, FaultReason, GuestAddress, MemoryFault},
    process::{EngineInstance, HostError, ResidentInstallation},
};

fn admits(modrm: u8) {
    let mut engine = EngineInstance::new(1, 0x1234_5678_9abc_def0).unwrap();
    let bytes = [0xd0, modrm, 0xeb, 0];
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
        .expect("memory byte count-one rotate must admit without reading data");
}

#[test]
fn memory_byte_rol_one_admits_with_unmapped_data() {
    admits(0x03);
}

#[test]
fn memory_byte_ror_one_admits_with_unmapped_data() {
    admits(0x0b);
}

const CODE: u32 = 0x1000;
const KEEP: u32 = 0x3000;
const DATA: u32 = 0x5000;
const KEY: u64 = 0x1234_5678_9abc_def0;
const KINDS: [(u8, RotateKind); 2] = [(0, RotateKind::Left), (1, RotateKind::Right)];
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

fn encoding(kind: RotateKind, tail: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0xd0];
    bytes.extend_from_slice(tail);
    if kind == RotateKind::Right {
        bytes[1] |= 8;
    }
    bytes
}

fn forms(kind: RotateKind) -> Vec<(Vec<u8>, EffectiveAddress)> {
    let mut forms = Vec::new();
    for (index, base) in REGISTERS.into_iter().enumerate() {
        let tail = match base {
            Register32::Esp => vec![0x04, 0x24],
            Register32::Ebp => vec![0x45, 0],
            _ => vec![index as u8],
        };
        forms.push((encoding(kind, &tail), address(Some(base), None, 1, 0)));
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
        forms.push((encoding(kind, tail), expected));
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

#[test]
fn exact_memory_addresses_admit_in_small_bound_banks_and_refuse_standalone() {
    let limits = CompileLimits::default();
    assert_eq!(
        (limits.blocks, limits.instructions, limits.wasm_bytes),
        (8, 64, 65_536)
    );
    let mut rows = 0;
    let mut profiles = 0;
    let mut refusals = 0;
    for (_, kind) in KINDS {
        let bank = forms(kind);
        assert_eq!(bank.len(), 14);
        let mut bytes = Vec::new();
        let mut starts = Vec::new();
        let mut expected = Vec::new();
        for (instruction, address) in bank {
            let pc = CODE + bytes.len() as u32;
            starts.push(pc);
            expected.push((pc, instruction.len(), address));
            bytes.extend(instruction);
        }
        starts.push(CODE + bytes.len() as u32);
        bytes.extend([0xeb, 0]);
        assert_eq!((starts.len(), bytes.len()), (15, 53));
        let engine = code(CODE, &bytes);
        let memory = engine.memory().unwrap();
        for &(pc, length, address) in &expected {
            let decoded = decode_one(memory, GuestAddress(pc)).unwrap();
            assert_eq!(
                decoded.operation(),
                &Operation::MemoryByteRotateOne { kind, address }
            );
            assert_eq!(
                (decoded.pc(), decoded.length() as usize, decoded.next_pc()),
                (GuestAddress(pc), length, GuestAddress(pc + length as u32))
            );
            assert!(memory.is_code_current(decoded.code_snapshot()));
            rows += 1;
        }
        for resident in [false, true] {
            for entries in [false, true] {
                let mut engine = code(CODE, &bytes);
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
                let module = if resident {
                    engine.guard_resident(KEY, id).unwrap();
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
                    engine.resident_bytes(id).unwrap()
                } else {
                    engine.guard(KEY, id as u32).unwrap();
                    engine.artifact_bytes().unwrap()
                };
                assert_eq!(&module[..8], b"\0asm\x01\0\0\0");
                assert!(module.len() <= limits.wasm_bytes);
                assert_eq!(engine.arena(), before);
                assert_eq!(engine.arena_address(), pointer);
                let memory = engine.memory().unwrap();
                assert_eq!(memory.mapped_pages(), 1);
                assert!(memory.is_code_current(&snapshot));
                for data in [0, DATA] {
                    assert!(memory.resolve(GuestAddress(data), Access::Read).is_err());
                }
                profiles += 1;
            }
        }
        let (bytes, _) = forms(kind).remove(0);
        let engine = code(CODE, &bytes);
        let memory = engine.memory().unwrap();
        assert!(memory.resolve(GuestAddress(0), Access::Read).is_err());
        let error = CompileError::Instruction {
            pc: GuestAddress(CODE),
            cause: InstructionError::BackendUnsupported,
        };
        for observed in [
            compile_region(
                memory,
                &[BlockSpec {
                    entry: GuestAddress(CODE),
                    byte_length: bytes.len() as u32,
                }],
                limits,
            )
            .err(),
            compile_entry_region(memory, &[GuestAddress(CODE)], limits).err(),
        ] {
            assert_eq!(observed, Some(error), "{kind:?}");
            refusals += 1;
        }
    }
    assert_eq!((rows, profiles, refusals), (28, 8, 4));
}

#[test]
fn excluded_counts_prefixes_and_supported_neighbors_keep_exact_ir_boundaries() {
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
            for operand in [modrm, 0x03 | field << 3] {
                rejected(&[prefix, 0xd0, operand], expected);
                exclusions += 1;
            }
        }
        for count in [0, 1, 2, 8, 31, 32, 33, 65, 97, 129, 161, 193, 225, 255] {
            let mut bytes = vec![0xc0, modrm, count];
            if count & 31 == 1 {
                bytes.insert(0, 0x66);
            }
            rejected(&bytes, opcode);
            exclusions += 1;
        }
        for count in [0, 1, 2, 8, 31, 32, 33, 65, 97, 129, 161, 193, 225, 255] {
            let mut bytes = vec![0xc0, 0x03 | field << 3, count];
            if count & 31 == 1 {
                bytes.insert(0, 0x66);
            }
            rejected(&bytes, opcode);
            exclusions += 1;
        }
        for modrm in [modrm, 0x03 | field << 3] {
            rejected(&[0xd2, modrm], opcode);
            rejected(&[0x66, 0xd1, modrm], opcode);
            exclusions += 2;
        }
    }
    for field in [2, 3] {
        for modrm in [0xc4 | field << 3, 0x03 | field << 3] {
            let mut implicit = vec![0xd0, modrm];
            if modrm & 0xc0 == 0xc0 {
                implicit.insert(0, 0x66);
            }
            for bytes in [implicit, vec![0xc0, modrm, 1], vec![0xd2, modrm]] {
                rejected(&bytes, opcode);
                exclusions += 1;
            }
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
    for (field, kind) in KINDS {
        for (index, destination) in BYTE_REGISTERS.into_iter().enumerate() {
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
        for (index, destination) in BYTE_REGISTERS.into_iter().enumerate() {
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
    assert_eq!((exclusions, neighbors), (120, 64));
}

#[test]
fn exact_operand_fetch_snapshots_and_top_wrap_preserve_each_length_boundary() {
    let mut complete = 0;
    let mut truncated = 0;
    let mut permissions = 0;
    let mut invalidations = 0;
    for (_, kind) in KINDS {
        for (tail, expected_address) in [
            (&[0x03][..], address(Some(Register32::Ebx), None, 1, 0)),
            (
                &[0x43, 0x80],
                address(Some(Register32::Ebx), None, 1, 0xffff_ff80),
            ),
            (
                &[0x44, 0x90, 0xe0],
                address(Some(Register32::Eax), Some(Register32::Edx), 4, 0xffff_ffe0),
            ),
            (
                &[0x05, 0x10, 0x50, 0, 0],
                address(None, None, 1, DATA + 0x10),
            ),
            (
                &[0x84, 0xc2, 0x78, 0x56, 0x34, 0x12],
                address(Some(Register32::Edx), Some(Register32::Eax), 8, 0x1234_5678),
            ),
        ] {
            let bytes = encoding(kind, tail);
            let length = bytes.len() as u32;
            for pc in [0x2000 - length, u32::MAX - (length - 1), 0x1fff] {
                let mut engine = code(pc, &bytes);
                engine
                    .protect(pc & !0xfff, if pc == 0x1fff { 2 } else { 1 }, 4)
                    .unwrap();
                let decoded = decode_one(engine.memory().unwrap(), GuestAddress(pc)).unwrap();
                assert_eq!(
                    decoded.operation(),
                    &Operation::MemoryByteRotateOne {
                        kind,
                        address: expected_address,
                    }
                );
                assert_eq!(
                    (decoded.pc(), decoded.length(), decoded.next_pc()),
                    (
                        GuestAddress(pc),
                        length as u8,
                        GuestAddress(pc.wrapping_add(length))
                    )
                );
                assert!(
                    engine
                        .memory()
                        .unwrap()
                        .is_code_current(decoded.code_snapshot())
                );
                describe(&mut engine, pc, length, false);
                let before = engine.arena().to_vec();
                let generation = engine.compile(1).unwrap();
                let resident = engine.compile_resident(1).unwrap().get();
                engine.guard(KEY, generation).unwrap();
                engine.guard_resident(KEY, resident).unwrap();
                assert_eq!(engine.arena(), before);
                if pc == 0x2000 - length {
                    assert!(
                        engine
                            .memory()
                            .unwrap()
                            .resolve(GuestAddress(0x2000), Access::Execute)
                            .is_err()
                    );
                }
                if pc == u32::MAX - (length - 1) {
                    assert!(
                        engine
                            .memory()
                            .unwrap()
                            .resolve(GuestAddress(0), Access::Execute)
                            .is_err()
                    );
                }
                complete += 1;
            }
            for cut in 1..bytes.len() {
                for top in [false, true] {
                    let pc = if top {
                        ((1_u64 << 32) - cut as u64) as u32
                    } else {
                        0x2000 - cut as u32
                    };
                    let engine = code(pc, &bytes[..cut]);
                    let memory = engine.memory().unwrap();
                    let error = fetch_error(
                        pc,
                        if top { pc } else { 0x2000 },
                        cut as u32 + 1,
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
                                byte_length: length
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
            }
            let pc = 0x2000 - (length - 1);
            let mut engine = code(pc, &bytes);
            engine.protect(0x2000, 1, 3).unwrap();
            let memory = engine.memory().unwrap();
            let error = fetch_error(pc, 0x2000, length, FaultReason::Permission);
            assert_eq!(decode_one(memory, GuestAddress(pc)).err(), Some(error));
            for result in [
                compile_region(
                    memory,
                    &[BlockSpec {
                        entry: GuestAddress(pc),
                        byte_length: length,
                    }],
                    CompileLimits::default(),
                ),
                compile_entry_region(memory, &[GuestAddress(pc)], CompileLimits::default()),
            ] {
                assert_eq!(result.err(), Some(instruction_error(pc, error)));
            }
            permissions += 1;
        }
        let bytes = encoding(kind, &[0x84, 0xc2, 0x78, 0x56, 0x34, 0x12]);
        for changed in [0x1ffc, 0x2002] {
            let pc = 0x1ffc;
            let mut engine = code(pc, &bytes);
            let decoded = decode_one(engine.memory().unwrap(), GuestAddress(pc)).unwrap();
            describe(&mut engine, pc, bytes.len() as u32, false);
            let generation = engine.compile(1).unwrap();
            let resident = engine.compile_resident(1).unwrap().get();
            engine.guard(KEY, generation).unwrap();
            engine.guard_resident(KEY, resident).unwrap();
            upload(&mut engine, changed, &[bytes[(changed - pc) as usize]]);
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
            invalidations += 1;
        }
    }
    assert_eq!(
        (complete, truncated, permissions, invalidations),
        (30, 68, 10, 4)
    );
}

#[test]
fn data_only_changes_keep_both_memory_byte_rotate_owners_current() {
    let mut scenarios = 0;
    for (_, kind) in KINDS {
        let mut bytes = encoding(kind, &[0x05, 0x10, 0x50, 0, 0]);
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
        engine.write32(CODE + 2, DATA + 0x10).unwrap();
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
fn late_memory_byte_rotate_failures_preserve_both_publications_and_installation() {
    let opcode = DecodeError::Unsupported(UnsupportedFeature::Opcode);
    let mut refusals = 0;
    for (field, _) in KINDS {
        let modrm = 0x03 | field << 3;
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
