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
            ir::{BitScanKind, Operation},
        },
    },
    memory::{
        Access, AddressSpace, FaultReason, GuestAddress, MemoryFault, PageRange, Permissions,
    },
    process::{EngineInstance, HostError},
};

fn admits(opcode: u8) {
    let mut memory = AddressSpace::new(1).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    let bytes = [0x0f, opcode, 0xc3, 0xeb, 0];
    memory.write(GuestAddress(0x1000), &bytes).unwrap();
    compile_region(
        &memory,
        &[BlockSpec {
            entry: GuestAddress(0x1000),
            byte_length: bytes.len() as u32,
        }],
        CompileLimits::default(),
    )
    .expect("register bit scan must compile");
}

#[test]
fn bsf_compiles_before_jump() {
    admits(0xbc);
}

#[test]
fn bsr_compiles_before_jump() {
    admits(0xbd);
}

const CODE: u32 = 0x1000;
const KEEP: u32 = 0x3000;
const KEY: u64 = 0x1234_5678_9abc_def0;
const FORMS: [(u8, BitScanKind); 2] = [(0xbc, BitScanKind::Forward), (0xbd, BitScanKind::Reverse)];
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
fn every_kind_and_register_pair_has_exact_ir_and_compiles_in_all_six_profiles() {
    let mut decoded_forms = 0;
    let mut profiles = 0;
    for (opcode, kind) in FORMS {
        for first_destination in [0, 4] {
            let mut bytes = Vec::new();
            for destination in first_destination..first_destination + 4 {
                for source in 0..8 {
                    bytes.extend_from_slice(&[0x0f, opcode, 0xc0 | (destination << 3) | source]);
                }
            }
            bytes.extend_from_slice(&[0xeb, 0]);
            let engine = code(CODE, &bytes);
            let memory = engine.memory().unwrap();
            for destination in first_destination..first_destination + 4 {
                for source in 0..8 {
                    let offset = ((destination - first_destination) * 8 + source) * 3;
                    let pc = CODE + u32::from(offset);
                    let decoded = decode_one(memory, GuestAddress(pc)).unwrap();
                    assert_eq!(
                        decoded.operation(),
                        &Operation::BitScan {
                            kind,
                            destination: REGISTERS[usize::from(destination)],
                            source: REGISTERS[usize::from(source)]
                        }
                    );
                    assert_eq!(
                        (decoded.pc(), decoded.length(), decoded.next_pc()),
                        (GuestAddress(pc), 3, GuestAddress(pc + 3))
                    );
                    assert!(memory.is_code_current(decoded.code_snapshot()));
                    decoded_forms += 1;
                }
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
                profiles += 1;
            }
            for resident in [false, true] {
                for entries in [false, true] {
                    let mut engine = code(CODE, &bytes);
                    engine.protect(CODE, 1, 4).unwrap();
                    describe(&mut engine, CODE, bytes.len() as u32, entries);
                    let before = engine.arena().to_vec();
                    let id = compile(&mut engine, resident, entries).unwrap();
                    if resident {
                        engine.guard_resident(KEY, id).unwrap();
                        assert_eq!(&engine.resident_bytes(id).unwrap()[..8], b"\0asm\x01\0\0\0");
                        for offset in (0..96).step_by(3) {
                            assert_eq!(engine.lookup_resident(CODE + offset).unwrap().get(), id);
                            for interior in [1, 2] {
                                assert!(engine.lookup_resident(CODE + offset + interior).is_err());
                            }
                        }
                        assert_eq!(engine.lookup_resident(CODE + 96).unwrap().get(), id);
                        assert!(engine.lookup_resident(CODE + 97).is_err());
                        assert!(engine.lookup_resident(CODE + 98).is_err());
                    } else {
                        engine.guard(KEY, id as u32).unwrap();
                        assert_eq!(&engine.artifact_bytes().unwrap()[..8], b"\0asm\x01\0\0\0");
                    }
                    assert_eq!(engine.arena(), before);
                    profiles += 1;
                }
            }
        }
    }
    assert_eq!((decoded_forms, profiles), (128, 24));
}

#[test]
fn excluded_forms_and_three_byte_fetch_preserve_fault_and_snapshot_boundaries() {
    let opcode_error = DecodeError::Unsupported(UnsupportedFeature::Opcode);
    let mut exclusions = 0;
    for (opcode, kind) in FORMS {
        for prefix in [0x66, 0x67, 0xf2, 0x26, 0x2e, 0x36, 0x3e, 0x64, 0x65, 0xf0] {
            let engine = code(CODE, &[prefix, 0x0f, opcode, 0xc3]);
            let expected = match prefix {
                0xf0 => DecodeError::InvalidEncoding,
                0x26 | 0x2e | 0x36 | 0x3e | 0x64 | 0x65 => {
                    DecodeError::Unsupported(UnsupportedFeature::Segment)
                }
                _ => opcode_error,
            };
            assert_eq!(
                decode_one(engine.memory().unwrap(), GuestAddress(CODE)).err(),
                Some(expected),
                "{prefix:02x} 0f {opcode:02x} c3"
            );
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
            let bytes = [vec![0x0f, opcode], tail.to_vec()].concat();
            let engine = code(CODE, &bytes);
            assert_eq!(
                decode_one(engine.memory().unwrap(), GuestAddress(CODE)).err(),
                Some(opcode_error),
                "{bytes:02x?}"
            );
            exclusions += 1;
        }
        let bytes = [0x0f, opcode, 0xc3];
        for pc in [0x1ffd, u32::MAX - 2, 0x1ffe] {
            let mut engine = code(pc, &bytes);
            engine
                .protect(pc & !0xfff, if pc == 0x1ffe { 2 } else { 1 }, 4)
                .unwrap();
            let memory = engine.memory().unwrap();
            let decoded = decode_one(memory, GuestAddress(pc)).unwrap();
            assert_eq!(
                decoded.operation(),
                &Operation::BitScan {
                    kind,
                    destination: Register32::Eax,
                    source: Register32::Ebx
                }
            );
            assert_eq!(
                (decoded.length(), decoded.next_pc()),
                (3, GuestAddress(pc.wrapping_add(3)))
            );
            assert!(memory.is_code_current(decoded.code_snapshot()));
            assert_eq!(
                compile_region(
                    memory,
                    &[BlockSpec {
                        entry: GuestAddress(pc),
                        byte_length: 3
                    }],
                    CompileLimits::default()
                )
                .unwrap()
                .metadata()
                .instructions,
                1
            );
            if pc == 0x1ffd {
                assert!(
                    memory
                        .resolve(GuestAddress(0x2000), Access::Execute)
                        .is_err()
                );
            }
        }
        for length in [1, 2] {
            let missing = &bytes[..length];
            for top in [false, true] {
                let pc = if top {
                    ((1_u64 << 32) - length as u64) as u32
                } else {
                    0x2000 - length as u32
                };
                let engine = code(pc, missing);
                assert_eq!(
                    decode_one(engine.memory().unwrap(), GuestAddress(pc)).err(),
                    Some(fetch_error(
                        pc,
                        if top { pc } else { 0x2000 },
                        length as u32 + 1,
                        if top {
                            FaultReason::AddressOverflow
                        } else {
                            FaultReason::Unmapped
                        }
                    ))
                );
            }
        }
        let mut engine = code(0x1ffe, &bytes);
        engine.protect(0x2000, 1, 3).unwrap();
        assert_eq!(
            decode_one(engine.memory().unwrap(), GuestAddress(0x1ffe)).err(),
            Some(fetch_error(0x1ffe, 0x2000, 3, FaultReason::Permission))
        );
        for changed in [0x1ffe, 0x2000] {
            let mut engine = code(0x1ffe, &bytes);
            let decoded = decode_one(engine.memory().unwrap(), GuestAddress(0x1ffe)).unwrap();
            describe(&mut engine, 0x1ffe, 3, false);
            let generation = engine.compile(1).unwrap();
            let id = engine.compile_resident(1).unwrap().get();
            engine.guard(KEY, generation).unwrap();
            engine.guard_resident(KEY, id).unwrap();
            upload(
                &mut engine,
                changed,
                &[if changed == 0x1ffe { 0x0f } else { 0xc3 }],
            );
            assert!(
                !engine
                    .memory()
                    .unwrap()
                    .is_code_current(decoded.code_snapshot())
            );
            assert!(engine.guard(KEY, generation).is_err());
            assert!(engine.guard_resident(KEY, id).is_err());
        }
    }
    for bytes in [[0xf3, 0x0f, 0xbc, 0xc3], [0xf3, 0x0f, 0xbd, 0xc3]] {
        let engine = code(CODE, &bytes);
        assert_eq!(
            decode_one(engine.memory().unwrap(), GuestAddress(CODE)).err(),
            Some(opcode_error),
            "{bytes:02x?}"
        );
        exclusions += 1;
    }
    assert_eq!(exclusions, 36);
}

#[test]
fn late_bit_scan_failures_preserve_both_publications() {
    let opcode_error = DecodeError::Unsupported(UnsupportedFeature::Opcode);
    let mut refusals = 0;
    for first in [0xbc, 0xbd] {
        for (pc, bytes, declared, expected) in [
            (
                CODE,
                vec![0x0f, first, 0xc3, 0x0f, 0x0b],
                5,
                instruction_error(CODE + 3, opcode_error),
            ),
            (
                CODE,
                vec![0x0f, first, 0xc3, 0x66, 0x0f, first ^ 1, 0xc3],
                7,
                instruction_error(CODE + 3, opcode_error),
            ),
            (
                0x1ffc,
                vec![0x0f, first, 0xc3, 0x0f],
                5,
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
                    describe(&mut engine, KEEP, 3, false);
                    let generation = engine.compile(1).unwrap();
                    let keep = engine.compile_resident(1).unwrap().get();
                    describe(&mut engine, pc, declared, entries);
                    let before = (
                        engine.arena().to_vec(),
                        engine.generation(),
                        engine.artifact_bytes().unwrap().to_vec(),
                        engine.resident_bytes(keep).unwrap().to_vec(),
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
                            engine.resident_bytes(keep).unwrap().to_vec()
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
