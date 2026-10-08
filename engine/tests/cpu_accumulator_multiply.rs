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
            ir::{MultiplyKind, Operation},
        },
    },
    memory::{Access, FaultReason, GuestAddress, MemoryFault},
    process::{EngineInstance, HostError},
};

const CODE: u32 = 0x1000;
const KEEP: u32 = 0x3000;
const KEY: u64 = 0x1234_5678_9abc_def0;
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
const FORMS: [(u8, MultiplyKind); 2] =
    [(0xe0, MultiplyKind::Unsigned), (0xe8, MultiplyKind::Signed)];

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
fn every_register_form_has_exact_ir_and_compiles_in_all_six_profiles() {
    let mut bytes = Vec::new();
    for (base, _) in FORMS {
        for source in 0..8 {
            bytes.extend_from_slice(&[0xf7, base | source]);
        }
    }
    bytes.extend_from_slice(&[0xeb, 0]);
    let engine = code(CODE, &bytes);
    let memory = engine.memory().unwrap();
    for (group, (_, kind)) in FORMS.into_iter().enumerate() {
        for (index, source) in REGISTERS.into_iter().enumerate() {
            let pc = CODE + (group * 16 + index * 2) as u32;
            let decoded = decode_one(memory, GuestAddress(pc)).unwrap();
            assert_eq!(
                decoded.operation(),
                &Operation::MultiplyAccumulator { kind, source }
            );
            assert_eq!(
                (decoded.pc(), decoded.length(), decoded.next_pc()),
                (GuestAddress(pc), 2, GuestAddress(pc + 2))
            );
            assert!(memory.is_code_current(decoded.code_snapshot()));
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
        compile_entry_region(memory, &[GuestAddress(CODE)], CompileLimits::default()).unwrap(),
    ] {
        assert_eq!(
            (unit.metadata().blocks, unit.metadata().instructions),
            (1, 17)
        );
        assert_eq!(&unit.wasm_bytes(memory).unwrap()[..8], b"\0asm\x01\0\0\0");
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
        }
    }
}

#[test]
fn strict_exclusions_and_two_byte_fetch_preserve_fault_and_wrap_boundaries() {
    let opcode = DecodeError::Unsupported(UnsupportedFeature::Opcode);
    let mut exclusions = 0;
    for (base, kind) in FORMS {
        for prefix in [
            0x66, 0x67, 0xf2, 0xf3, 0x26, 0x2e, 0x36, 0x3e, 0x64, 0x65, 0xf0,
        ] {
            let engine = code(CODE, &[prefix, 0xf7, base]);
            let expected = match prefix {
                0xf0 => DecodeError::InvalidEncoding,
                0x26 | 0x2e | 0x36 | 0x3e | 0x64 | 0x65 => {
                    DecodeError::Unsupported(UnsupportedFeature::Segment)
                }
                _ => opcode,
            };
            assert_eq!(
                decode_one(engine.memory().unwrap(), GuestAddress(CODE)).err(),
                Some(expected),
                "{prefix:02x} f7 {base:02x}"
            );
            exclusions += 1;
        }
        for tail in [
            &[0x20][..],
            &[0x60, 0x80],
            &[0xa0, 0x78, 0x56, 0x34, 0x12],
            &[0x25, 0x78, 0x56, 0x34, 0x12],
            &[0x24, 0x24],
            &[0x24, 0x8a],
            &[0x24, 0x8d, 0x78, 0x56, 0x34, 0x12],
        ] {
            let mut bytes = vec![0x66, 0xf7];
            bytes.extend_from_slice(tail);
            bytes[2] |= base & 8;
            let engine = code(CODE, &bytes);
            assert_eq!(
                decode_one(engine.memory().unwrap(), GuestAddress(CODE)).err(),
                Some(opcode),
                "{bytes:02x?}"
            );
            exclusions += 1;
        }
        for pc in [0x1ffe, u32::MAX - 1, 0x1fff] {
            let mut engine = code(pc, &[0xf7, base]);
            engine
                .protect(pc & !0xfff, if pc == 0x1fff { 2 } else { 1 }, 4)
                .unwrap();
            let memory = engine.memory().unwrap();
            let decoded = decode_one(memory, GuestAddress(pc)).unwrap();
            assert_eq!(
                decoded.operation(),
                &Operation::MultiplyAccumulator {
                    kind,
                    source: Register32::Eax
                }
            );
            assert_eq!(
                (decoded.length(), decoded.next_pc()),
                (2, GuestAddress(pc.wrapping_add(2)))
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
        }
        let mut engine = code(0x1fff, &[0xf7, base]);
        engine.protect(0x2000, 1, 3).unwrap();
        assert_eq!(
            decode_one(engine.memory().unwrap(), GuestAddress(0x1fff)).err(),
            Some(fetch_error(0x1fff, 0x2000, 2, FaultReason::Permission))
        );
        for changed in [0x1fff, 0x2000] {
            let mut engine = code(0x1fff, &[0xf7, base]);
            let decoded = decode_one(engine.memory().unwrap(), GuestAddress(0x1fff)).unwrap();
            upload(
                &mut engine,
                changed,
                &[if changed == 0x1fff { 0xf7 } else { base }],
            );
            assert!(
                !engine
                    .memory()
                    .unwrap()
                    .is_code_current(decoded.code_snapshot())
            );
        }
    }
    for bytes in [
        &[0x66, 0xf6, 0xe0][..],
        &[0x66, 0xf6, 0xe8],
        &[0xf7, 0xf2],
        &[0xf7, 0xfa],
        &[0xf7, 0x32],
        &[0xf7, 0x3a],
    ] {
        let engine = code(CODE, bytes);
        assert_eq!(
            decode_one(engine.memory().unwrap(), GuestAddress(CODE)).err(),
            Some(opcode),
            "{bytes:02x?}"
        );
        exclusions += 1;
    }
    assert_eq!(exclusions, 42);
    for (pc, address, reason) in [
        (0x1fff, 0x2000, FaultReason::Unmapped),
        (u32::MAX, u32::MAX, FaultReason::AddressOverflow),
    ] {
        let engine = code(pc, &[0xf7]);
        assert_eq!(
            decode_one(engine.memory().unwrap(), GuestAddress(pc)).err(),
            Some(fetch_error(pc, address, 2, reason))
        );
    }
}

#[test]
fn late_accumulator_multiply_failures_preserve_both_publications() {
    let opcode = DecodeError::Unsupported(UnsupportedFeature::Opcode);
    let mut refusals = 0;
    for (base, _) in FORMS {
        let other = base ^ 8;
        for (pc, bytes, declared, expected) in [
            (
                CODE,
                vec![0xf7, base, 0x0f, 0x0b],
                4,
                instruction_error(CODE + 2, opcode),
            ),
            (
                CODE,
                vec![0xf7, base, 0x66, 0xf7, other],
                5,
                instruction_error(CODE + 2, opcode),
            ),
            (
                0x1ffd,
                vec![0xf7, base, 0xf7],
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
