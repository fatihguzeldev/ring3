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
            ir::{ByteRegister, EffectiveAddress, Location32, Operation, ShiftCount, ShiftKind},
        },
    },
    memory::{Access, AddressSpace, FaultReason, GuestAddress, MemoryFault},
    process::{EngineInstance, HostError},
};

fn admits(bytes: &[u8]) {
    let mut engine = EngineInstance::new(1, 0x1234_5678_9abc_def0).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(bytes);
    engine.upload(0x1000, bytes.len() as u32).unwrap();
    assert_eq!(engine.memory().unwrap().mapped_pages(), 1);
    let request = &mut engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8];
    request[..4].copy_from_slice(&0x1000_u32.to_le_bytes());
    request[4..].copy_from_slice(&(bytes.len() as u32).to_le_bytes());
    engine
        .compile(1)
        .expect("masked-one byte memory shift must admit with data unmapped");
}

#[test]
fn memory_shl_immediate_one_admits_with_unmapped_data() {
    admits(&[0xc0, 0x23, 1, 0xeb, 0]);
}

#[test]
fn memory_sar_immediate_thirty_three_admits_with_unmapped_data() {
    admits(&[0xc0, 0x3b, 33, 0xeb, 0]);
}

const CODE: u32 = 0x1000;
const KEEP: u32 = 0x3000;
const DATA: u32 = 0x5000;
const KEY: u64 = 0x1234_5678_9abc_def0;
const KINDS: [(ShiftKind, u8); 3] = [
    (ShiftKind::Shl, 4),
    (ShiftKind::Shr, 5),
    (ShiftKind::Sar, 7),
];
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
    let mut engine = EngineInstance::new(pages + 1, KEY).unwrap();
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

fn field(kind: ShiftKind) -> u8 {
    match kind {
        ShiftKind::Shl => 4,
        ShiftKind::Shr => 5,
        ShiftKind::Sar => 7,
    }
}

fn encoding(kind: ShiftKind, tail: &[u8], count: u8) -> Vec<u8> {
    let mut bytes = vec![0xc0];
    bytes.extend_from_slice(tail);
    bytes[1] |= field(kind) << 3;
    bytes.push(count);
    bytes
}

fn memory_forms(kind: ShiftKind, count: u8) -> Vec<(Vec<u8>, EffectiveAddress)> {
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

fn register_bytes(kind: ShiftKind, count: u8) -> [u8; 3] {
    [0xc0, 0xc4 | field(kind) << 3, count]
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
    assert!(starts.len() <= 64);
    let mut profiles = 0;
    for resident in [false, true] {
        for entries in [false, true] {
            let mut engine = code(CODE, bytes);
            engine.protect(CODE, 1, 4).unwrap();
            describe(&mut engine, CODE, bytes.len() as u32, entries);
            let before = engine.arena().to_vec();
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
fn register_byte_fixed_memory_and_wide_shift_neighbors_keep_original_count_ir() {
    let mut engine = code(CODE, &[0x90]);
    let counts = [0, 1, 2, 32, 33, 65, 97, 129, 161, 193, 225, 255];
    let ea = address(Some(Register32::Ebx), None, 1, 0);
    let mut neighbors = 0;
    for (kind, extension) in KINDS {
        for count in counts {
            upload(&mut engine, CODE, &register_bytes(kind, count));
            assert_instruction(
                engine.memory().unwrap(),
                CODE,
                3,
                Operation::ShiftByte {
                    kind,
                    destination: ByteRegister::Ah,
                    count: ShiftCount::Immediate(count),
                },
            );
            neighbors += 1;
            for memory in [false, true] {
                let modrm = (if memory { 3 } else { 0xc0 }) | extension << 3;
                upload(&mut engine, CODE, &[0xc1, modrm, count]);
                let destination = if memory {
                    Location32::Memory(ea)
                } else {
                    Location32::Register(Register32::Eax)
                };
                assert_instruction(
                    engine.memory().unwrap(),
                    CODE,
                    3,
                    Operation::Shift {
                        kind,
                        destination,
                        count: ShiftCount::Immediate(count),
                    },
                );
                neighbors += 1;
            }
        }
        upload(&mut engine, CODE, &[0xd0, 3 | extension << 3]);
        assert_instruction(
            engine.memory().unwrap(),
            CODE,
            2,
            Operation::MemoryShiftByte { kind, address: ea },
        );
        neighbors += 1;
    }
    assert_eq!(neighbors, 111);
}

#[test]
fn all_memory_immediates_and_addresses_have_exact_ir_in_bound_profiles() {
    let mut forms = 0;
    let mut profiles = 0;
    for (kind, _) in KINDS {
        for counts in COUNTS.chunks(2) {
            let mut bytes = Vec::new();
            let mut expected = Vec::new();
            let mut starts = Vec::new();
            for &count in counts {
                for (form, address) in memory_forms(kind, count) {
                    let pc = CODE + bytes.len() as u32;
                    starts.push(pc);
                    expected.push((pc, form.len(), Operation::MemoryShiftByte { kind, address }));
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
    for (kind, _) in KINDS {
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
    assert_eq!((forms, profiles, refusals), (336, 48, 6));
}

#[test]
fn raw_immediate_domain_and_strict_memory_neighbors_preserve_decode_boundaries() {
    let mut engine = code(CODE, &[0x90]);
    let mut accepted = 0;
    let mut promoted = 0;
    let opcode = DecodeError::Unsupported(UnsupportedFeature::Opcode);
    let ea = address(Some(Register32::Ebx), None, 1, 0);
    for (kind, _) in KINDS {
        for count in 0..=u8::MAX {
            let bytes = encoding(kind, &[0x03], count);
            upload(&mut engine, CODE, &bytes);
            if COUNTS.contains(&count) {
                assert_instruction(
                    engine.memory().unwrap(),
                    CODE,
                    3,
                    Operation::MemoryShiftByte { kind, address: ea },
                );
                accepted += 1;
            } else {
                assert_instruction(
                    engine.memory().unwrap(),
                    CODE,
                    3,
                    Operation::MemoryShiftByteImmediate {
                        kind,
                        address: ea,
                        count,
                    },
                );
                promoted += 1;
            }
        }
    }
    assert_eq!((accepted, promoted), (24, 744));
    assert_eq!(accepted + promoted, 768);
    let mut exclusions = 0;
    for (kind, extension) in KINDS {
        let modrm = 3 | extension << 3;
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
            rejected(&[prefix, 0xc0, modrm, 225], expected);
            exclusions += 1;
        }
        for count in [0, 2, 32, 255] {
            let mut bytes = encoding(kind, &[0x03], count);
            bytes.insert(0, 0x66);
            rejected(&bytes, opcode);
            exclusions += 1;
        }
        rejected(&[0x66, 0xd2, modrm], opcode);
        rejected(&[0x66, 0xc1, modrm, 33], opcode);
        exclusions += 2;
    }
    for extension in [0, 1, 2, 3, 6] {
        for count in COUNTS {
            let mut bytes = vec![0xc0, 3 | extension << 3, count];
            if matches!(extension, 0..=3) {
                bytes.insert(0, 0x66);
            }
            rejected(&bytes, opcode);
            exclusions += 1;
        }
    }
    assert_eq!(exclusions, 91);
}

#[test]
fn final_immediate_fetch_and_count_snapshots_preserve_page_and_wrap_boundaries() {
    let mut complete = 0;
    let mut truncated = 0;
    let mut permissions = 0;
    let mut invalidations = 0;
    for (kind, _) in KINDS {
        for (bytes, operation) in [
            (
                encoding(kind, &[0x03], 225),
                Operation::MemoryShiftByte {
                    kind,
                    address: address(Some(Register32::Ebx), None, 1, 0),
                },
            ),
            (
                encoding(kind, &[0x05, 0x10, 0x50, 0, 0], 33),
                Operation::MemoryShiftByte {
                    kind,
                    address: address(None, None, 1, DATA + 0x10),
                },
            ),
            (
                encoding(kind, &[0x84, 0xc2, 0x78, 0x56, 0x34, 0x12], 129),
                Operation::MemoryShiftByte {
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
                engine.guard(KEY, generation).unwrap();
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
                assert_eq!(
                    decode_one(engine.memory().unwrap(), GuestAddress(pc)).err(),
                    Some(fetch_error(
                        pc,
                        if top { pc } else { 0x2000 },
                        bytes.len() as u32,
                        if top {
                            FaultReason::AddressOverflow
                        } else {
                            FaultReason::Unmapped
                        }
                    ))
                );
                truncated += 1;
            }
            let pc = 0x2001 - bytes.len() as u32;
            let mut engine = code(pc, &bytes);
            engine.protect(0x2000, 1, 3).unwrap();
            assert_eq!(
                decode_one(engine.memory().unwrap(), GuestAddress(pc)).err(),
                Some(fetch_error(
                    pc,
                    0x2000,
                    bytes.len() as u32,
                    FaultReason::Permission
                ))
            );
            permissions += 1;
        }
        for (pc, bytes) in [
            (0x1ffd, encoding(kind, &[0x03], 1)),
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
        (27, 18, 9, 12)
    );
}

#[test]
fn data_only_changes_keep_immediate_owners_current_and_count_writes_invalidate() {
    let mut bytes = encoding(ShiftKind::Shl, &[0x05, 0x10, 0x50, 0, 0], 1);
    bytes.extend_from_slice(&encoding(ShiftKind::Shr, &[0x05, 0x10, 0x50, 0, 0], 225));
    bytes.extend_from_slice(&encoding(ShiftKind::Sar, &[0x05, 0x10, 0x50, 0, 0], 33));
    bytes.extend_from_slice(&[0xeb, 0]);
    let mut engine = code(CODE, &bytes);
    let snapshots: Vec<_> = [CODE, CODE + 7, CODE + 14]
        .into_iter()
        .map(|pc| decode_one(engine.memory().unwrap(), GuestAddress(pc)).unwrap())
        .collect();
    describe(&mut engine, CODE, bytes.len() as u32, false);
    let generation = engine.compile(1).unwrap();
    let id = engine.compile_resident(1).unwrap().get();
    let before = (
        engine.artifact_bytes().unwrap().to_vec(),
        engine.resident_bytes(id).unwrap().to_vec(),
    );
    engine.map(DATA, 1, 3).unwrap();
    engine.write32(DATA + 0x10, 0x9234_5678).unwrap();
    engine.protect(DATA, 1, 1).unwrap();
    for phase in 0..3 {
        if phase == 1 {
            engine.unmap(DATA, 1).unwrap();
        } else if phase == 2 {
            engine.map(DATA, 1, 3).unwrap();
            engine.write32(DATA + 0x10, 0x9234_5679).unwrap();
            engine.protect(DATA, 1, 1).unwrap();
        }
        engine.guard(KEY, generation).unwrap();
        engine.guard_resident(KEY, id).unwrap();
        assert_eq!(
            (
                engine.artifact_bytes().unwrap().to_vec(),
                engine.resident_bytes(id).unwrap().to_vec()
            ),
            before
        );
        for decoded in &snapshots {
            assert!(
                engine
                    .memory()
                    .unwrap()
                    .is_code_current(decoded.code_snapshot())
            );
        }
    }
    upload(&mut engine, CODE + 6, &[1]);
    for decoded in snapshots {
        assert!(
            !engine
                .memory()
                .unwrap()
                .is_code_current(decoded.code_snapshot())
        );
    }
    assert_eq!(
        engine.guard(KEY, generation),
        Err(HostError::CodeInvalidated)
    );
    assert_eq!(
        engine.guard_resident(KEY, id),
        Err(HostError::Resident(RegistryError::CodeInvalidated))
    );
}

#[test]
fn late_immediate_byte_shift_failures_preserve_both_publications() {
    let opcode = DecodeError::Unsupported(UnsupportedFeature::Opcode);
    let mut refusals = 0;
    for (kind, _) in KINDS {
        let modrm = field(kind) << 3 | 3;
        for (bytes, expected) in [
            (
                vec![0xc0, modrm, 33, 0x0f, 0x0b],
                instruction_error(CODE + 3, opcode),
            ),
            (
                vec![0xc0, modrm, 33, 0x66, 0xc0, modrm, 2],
                instruction_error(CODE + 3, opcode),
            ),
        ] {
            let pc = CODE;
            let declared = bytes.len() as u32;
            for resident in [false, true] {
                for entries in [false, true] {
                    let mut engine = code(pc, &bytes);
                    engine.map(KEEP, 1, 7).unwrap();
                    upload(&mut engine, KEEP, &[0x90, 0xeb, 0]);
                    describe(&mut engine, KEEP, 3, false);
                    let generation = engine.compile(1).unwrap();
                    let keep = engine.compile_resident(1).unwrap().get();
                    describe(&mut engine, pc, declared, entries);
                    let before = (
                        engine.arena().to_vec(),
                        engine.generation(),
                        engine.artifact_bytes().unwrap().to_vec(),
                        engine.resident_bytes(keep).unwrap().to_vec(),
                        engine.artifact_bytes().unwrap().as_ptr() as usize,
                        engine.resident_bytes(keep).unwrap().as_ptr() as usize,
                    );
                    let expected = if resident {
                        HostError::Resident(RegistryError::Compile(expected))
                    } else {
                        HostError::Compile(expected)
                    };
                    assert_eq!(compile(&mut engine, resident, entries), Err(expected));
                    assert_eq!(
                        (
                            engine.arena().to_vec(),
                            engine.generation(),
                            engine.artifact_bytes().unwrap().to_vec(),
                            engine.resident_bytes(keep).unwrap().to_vec(),
                            engine.artifact_bytes().unwrap().as_ptr() as usize,
                            engine.resident_bytes(keep).unwrap().as_ptr() as usize
                        ),
                        before
                    );
                    engine.guard(KEY, generation).unwrap();
                    engine.guard_resident(KEY, keep).unwrap();
                    assert_eq!(engine.lookup_resident(KEEP).unwrap().get(), keep);
                    assert!(engine.lookup_resident(pc).is_err());
                    refusals += 1;
                }
            }
        }
    }
    assert_eq!(refusals, 24);
}
