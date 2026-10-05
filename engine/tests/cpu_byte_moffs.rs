use ring3_engine::{
    cpu::{
        UnsupportedFeature,
        dbt::{
            BlockSpec, CompileError, CompileLimits, InstructionError, RegistryError,
            compile_entry_region, compile_region, prepare_entry_region, prepare_region,
        },
        x86::{
            decode::{DecodeError, decode_one},
            ir::{ByteRegister, ByteValue, EffectiveAddress, Operation},
        },
    },
    memory::{Access, FaultReason, GuestAddress, MemoryFault},
    process::{EngineInstance, HostError},
};

#[test]
fn canonical_al_moffs_decode_and_bound_profiles_reuse_byte_operations() {
    let mut engine = EngineInstance::new(1, KEY).unwrap();
    let code = [0xa0, 0, 0x50, 0, 0, 0xeb, 0];
    engine.map(CODE, 1, 7).unwrap();
    upload(&mut engine, CODE, &code);
    engine.protect(CODE, 1, 4).unwrap();
    assert!(
        engine
            .memory()
            .unwrap()
            .resolve(GuestAddress(DATA), Access::Read)
            .is_err()
    );
    describe(&mut engine, CODE, code.len(), false);
    let before = engine.arena().to_vec();
    engine
        .compile(1)
        .expect("bound accumulator byte load must admit without reading guest data");
    assert_eq!(engine.arena(), before);

    for opcode in [0xa0, 0xa2] {
        for address in [0, DATA + 0x10, 0x8000_0000, u32::MAX] {
            let bytes = program(opcode, address);
            for resident in [false, true] {
                for entries in [false, true] {
                    let mut engine = code_at(CODE, &bytes);
                    engine.protect(CODE, 1, 4).unwrap();
                    let decoded = decode_one(engine.memory().unwrap(), GuestAddress(CODE)).unwrap();
                    let ea = EffectiveAddress {
                        base: None,
                        index: None,
                        scale: 1,
                        displacement: address,
                    };
                    let expected = if opcode == 0xa0 {
                        Operation::LoadByte {
                            destination: ByteRegister::Al,
                            address: ea,
                        }
                    } else {
                        Operation::StoreByte {
                            address: ea,
                            source: ByteValue::Register(ByteRegister::Al),
                        }
                    };
                    assert_eq!(decoded.operation(), &expected);
                    assert_eq!(
                        (decoded.length(), decoded.next_pc()),
                        (5, GuestAddress(CODE + 5))
                    );
                    describe(&mut engine, CODE, bytes.len(), entries);
                    let before = engine.arena().to_vec();
                    let id = publish(&mut engine, resident, entries).unwrap();
                    assert_eq!(engine.arena(), before);
                    assert_eq!(engine.memory().unwrap().mapped_pages(), 1);
                    assert!(
                        engine
                            .memory()
                            .unwrap()
                            .resolve(
                                GuestAddress(address),
                                if opcode == 0xa0 {
                                    Access::Read
                                } else {
                                    Access::Write
                                }
                            )
                            .is_err()
                    );
                    assert_module(module(&engine, resident, id), resident, opcode);
                    guard(&engine, resident, id).unwrap();
                    if resident {
                        assert_eq!(engine.lookup_resident(CODE + 5).unwrap().get(), id);
                    }
                }
            }
            let engine = code_at(CODE, &bytes);
            let memory = engine.memory().unwrap();
            let specs = [BlockSpec {
                entry: GuestAddress(CODE),
                byte_length: 7,
            }];
            let entries = [GuestAddress(CODE)];
            let expected = CompileError::Instruction {
                pc: GuestAddress(CODE),
                cause: InstructionError::BackendUnsupported,
            };
            assert_eq!(
                prepare_region(memory, &specs, CompileLimits::default()).err(),
                Some(expected)
            );
            assert_eq!(
                compile_region(memory, &specs, CompileLimits::default()).err(),
                Some(expected)
            );
            assert_eq!(
                prepare_entry_region(memory, &entries, CompileLimits::default()).err(),
                Some(expected)
            );
            assert_eq!(
                compile_entry_region(memory, &entries, CompileLimits::default()).err(),
                Some(expected)
            );
        }
    }
}

const CODE: u32 = 0x1000;
const KEEP: u32 = 0x3000;
const DATA: u32 = 0x5000;
const KEY: u64 = 0x1234_5678_9abc_def0;

fn program(opcode: u8, address: u32) -> Vec<u8> {
    let mut bytes = vec![opcode];
    bytes.extend(address.to_le_bytes());
    bytes.extend([0xeb, 0]);
    bytes
}
fn upload(engine: &mut EngineInstance, pc: u32, bytes: &[u8]) {
    engine.arena_mut().unwrap()[140..140 + bytes.len()].copy_from_slice(bytes);
    engine.upload(pc, bytes.len() as u32).unwrap();
}
fn code_at(pc: u32, bytes: &[u8]) -> EngineInstance {
    let base = pc & !0xfff;
    let pages = (u64::from(pc - base) + bytes.len() as u64).div_ceil(4096) as u32;
    let mut engine = EngineInstance::new(pages + 1, KEY).unwrap();
    engine.map(base, pages, 7).unwrap();
    upload(&mut engine, pc, bytes);
    engine
}
fn describe(engine: &mut EngineInstance, pc: u32, length: usize, entries: bool) {
    let transfer = &mut engine.arena_mut().unwrap()[140..];
    transfer.fill(0xa5);
    transfer[..4].copy_from_slice(&pc.to_le_bytes());
    if !entries {
        transfer[4..8].copy_from_slice(&(length as u32).to_le_bytes());
    }
}
fn publish(engine: &mut EngineInstance, resident: bool, entries: bool) -> Result<u64, HostError> {
    match (resident, entries) {
        (false, false) => engine.compile(1).map(u64::from),
        (false, true) => engine.compile_entries(1, 0).map(u64::from),
        (true, false) => engine.compile_resident(1).map(|id| id.get()),
        (true, true) => engine.compile_resident_entries(1, 0).map(|id| id.get()),
    }
}
fn module(engine: &EngineInstance, resident: bool, id: u64) -> &[u8] {
    if resident {
        engine.resident_bytes(id).unwrap()
    } else {
        engine.artifact_bytes().unwrap()
    }
}
fn guard(engine: &EngineInstance, resident: bool, id: u64) -> Result<(), HostError> {
    if resident {
        engine.guard_resident(KEY, id)
    } else {
        engine.guard(KEY, id as u32)
    }
}
fn compile_error(resident: bool, error: CompileError) -> HostError {
    if resident {
        HostError::Resident(RegistryError::Compile(error))
    } else {
        HostError::Compile(error)
    }
}
fn unsigned(bytes: &mut &[u8]) -> u32 {
    let mut value = 0;
    for shift in (0..35).step_by(7) {
        let next = bytes[0];
        *bytes = &bytes[1..];
        value |= u32::from(next & 0x7f) << shift;
        if next & 0x80 == 0 {
            return value;
        }
    }
    panic!("unterminated unsigned LEB");
}
fn section(wasm: &[u8], kind: u8) -> &[u8] {
    assert_eq!(&wasm[..8], b"\0asm\x01\0\0\0");
    let mut remaining = &wasm[8..];
    while !remaining.is_empty() {
        let id = remaining[0];
        remaining = &remaining[1..];
        let length = unsigned(&mut remaining) as usize;
        let (payload, rest) = remaining.split_at(length);
        remaining = rest;
        if id == kind {
            return payload;
        }
    }
    panic!("missing section {kind}");
}
fn assert_module(wasm: &[u8], resident: bool, opcode: u8) {
    let helper = if opcode == 0xa0 {
        "read8"
    } else if resident {
        "store_resident8"
    } else {
        "store8"
    };
    let mut types = vec![3];
    for arity in [
        4,
        if resident { 7 } else { 6 },
        if opcode == 0xa0 {
            1
        } else if resident {
            6
        } else {
            2
        },
    ] {
        types.extend([0x60, arity]);
        types.extend(std::iter::repeat_n(0x7f, arity as usize));
        types.extend([1, 0x7f]);
    }
    assert_eq!(section(wasm, 1), types);
    let mut imports = vec![3];
    for (namespace, name, tail) in [
        ("env", "memory", &[2, 0, 1][..]),
        (
            "ring3",
            if resident { "guard_resident" } else { "guard" },
            &[0, 1][..],
        ),
        ("ring3", helper, &[0, 2][..]),
    ] {
        for text in [namespace, name] {
            imports.push(text.len() as u8);
            imports.extend(text.bytes());
        }
        imports.extend(tail);
    }
    assert_eq!(section(wasm, 2), imports);
    assert_eq!(section(wasm, 3), [1, 0]);
    assert_eq!(section(wasm, 7), [1, 3, b'r', b'u', b'n', 0, 2]);
    let mut bodies = section(wasm, 10);
    assert_eq!(unsigned(&mut bodies), 1);
    let length = unsigned(&mut bodies) as usize;
    assert_eq!(bodies.len(), length);
    assert_eq!(&bodies[..7], &[3, 16, 0x7f, 1, 0x7e, 6, 0x7f]);
    assert!(wasm.len() <= 65_536);
}

#[test]
fn five_byte_fetch_and_consumed_page_currency_keep_exact_boundaries() {
    for opcode in [0xa0, 0xa2] {
        let bytes = program(opcode, DATA + 0x10);
        for prefix in [0x66, 0x67, 0xf2, 0xf3, 0x64, 0xf0] {
            let mut excluded = vec![prefix];
            excluded.extend(&bytes[..5]);
            let engine = code_at(CODE, &excluded);
            let expected = match prefix {
                0x64 => DecodeError::Unsupported(UnsupportedFeature::Segment),
                0xf0 => DecodeError::InvalidEncoding,
                _ => DecodeError::Unsupported(UnsupportedFeature::Opcode),
            };
            assert_eq!(
                decode_one(engine.memory().unwrap(), GuestAddress(CODE)).err(),
                Some(expected)
            );
        }
        for cut in 1..5 {
            for reason in [
                FaultReason::Unmapped,
                FaultReason::Permission,
                FaultReason::AddressOverflow,
            ] {
                let pc = if reason == FaultReason::AddressOverflow {
                    u32::MAX - cut as u32 + 1
                } else {
                    0x2000 - cut as u32
                };
                let mut engine = code_at(pc, &bytes[..cut]);
                if reason == FaultReason::Permission {
                    engine.map(0x2000, 1, 3).unwrap();
                }
                let before = engine.arena().to_vec();
                assert_eq!(
                    decode_one(engine.memory().unwrap(), GuestAddress(pc)).err(),
                    Some(DecodeError::MemoryFault {
                        pc: GuestAddress(pc),
                        fault: MemoryFault {
                            address: GuestAddress(if reason == FaultReason::AddressOverflow {
                                pc
                            } else {
                                0x2000
                            }),
                            access: Access::Execute,
                            reason
                        },
                        length: cut as u32 + 1
                    })
                );
                assert_eq!(engine.arena(), before);
            }
        }
        for pc in [0x1ffb, u32::MAX - 4] {
            let engine = code_at(pc, &bytes[..5]);
            let decoded = decode_one(engine.memory().unwrap(), GuestAddress(pc)).unwrap();
            assert_eq!(
                (decoded.length(), decoded.next_pc()),
                (5, GuestAddress(pc.wrapping_add(5)))
            );
        }
        for resident in [false, true] {
            for entries in [false, true] {
                let mut engine = code_at(0x1ffd, &bytes);
                let decoded = decode_one(engine.memory().unwrap(), GuestAddress(0x1ffd)).unwrap();
                let snapshot = engine
                    .memory()
                    .unwrap()
                    .snapshot_code(GuestAddress(0x1ffd), 7)
                    .unwrap();
                describe(&mut engine, 0x1ffd, bytes.len(), entries);
                let id = publish(&mut engine, resident, entries).unwrap();
                let old = module(&engine, resident, id).to_vec();
                engine.map(DATA, 1, 3).unwrap();
                engine.write8(DATA + 0x10, 0xff).unwrap();
                engine.protect(DATA, 1, 2).unwrap();
                engine.unmap(DATA, 1).unwrap();
                assert!(
                    engine
                        .memory()
                        .unwrap()
                        .is_code_current(decoded.code_snapshot())
                );
                assert!(engine.memory().unwrap().is_code_current(&snapshot));
                assert_eq!(module(&engine, resident, id), old);
                guard(&engine, resident, id).unwrap();
                engine.write8(0x2000, u32::from(bytes[3])).unwrap();
                assert!(
                    !engine
                        .memory()
                        .unwrap()
                        .is_code_current(decoded.code_snapshot())
                );
                assert!(!engine.memory().unwrap().is_code_current(&snapshot));
                assert_eq!(
                    guard(&engine, resident, id),
                    Err(if resident {
                        HostError::Resident(RegistryError::CodeInvalidated)
                    } else {
                        HostError::CodeInvalidated
                    })
                );
            }
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Saved {
    bytes: [Vec<u8>; 4],
    pointers: [usize; 4],
    pages: [Vec<u8>; 3],
    generation: u32,
    mapped: u32,
}
fn saved(engine: &EngineInstance, keeper: u64) -> Saved {
    let values = [
        engine.arena(),
        engine.artifact_bytes().unwrap(),
        engine.resident_bytes(keeper).unwrap(),
        engine.dispatcher_bytes(KEY).unwrap(),
    ];
    Saved {
        bytes: values.map(<[u8]>::to_vec),
        pointers: values.map(|bytes| bytes.as_ptr() as usize),
        pages: [CODE, KEEP, DATA].map(|pc| {
            let mut bytes = vec![0; 4096];
            engine
                .memory()
                .unwrap()
                .read(GuestAddress(pc), &mut bytes)
                .unwrap();
            bytes
        }),
        generation: engine.generation(),
        mapped: engine.memory().unwrap().mapped_pages(),
    }
}
fn prior() -> (EngineInstance, u64) {
    let mut engine = EngineInstance::new(3, KEY).unwrap();
    for pc in [CODE, KEEP, DATA] {
        engine.map(pc, 1, 7).unwrap();
    }
    upload(&mut engine, KEEP, &[0x90, 0xeb, 0]);
    upload(&mut engine, DATA, &[0x7f, 0x80, 0xff]);
    describe(&mut engine, KEEP, 3, false);
    engine.compile(1).unwrap();
    describe(&mut engine, KEEP, 3, false);
    let keeper = engine.compile_resident(1).unwrap().get();
    engine
        .acknowledge_resident_installation(KEY, keeper, 0)
        .unwrap();
    (engine, keeper)
}

#[test]
fn helper_bindings_caps_and_failed_publication_preserve_existing_owners() {
    assert_eq!(
        CompileLimits::default(),
        CompileLimits {
            blocks: 8,
            instructions: 64,
            wasm_bytes: 65_536
        }
    );
    for opcode in [0xa0, 0xa2] {
        for resident in [false, true] {
            for entries in [false, true] {
                let mut accepted = program(opcode, DATA);
                accepted.splice(5..5, [0x90; 62]);
                let mut engine = code_at(CODE, &accepted);
                describe(&mut engine, CODE, accepted.len(), entries);
                let id = publish(&mut engine, resident, entries).unwrap();
                assert_module(module(&engine, resident, id), resident, opcode);
                guard(&engine, resident, id).unwrap();
                let mut too_many = program(opcode, DATA);
                too_many.splice(5..5, [0x90; 63]);
                let mut poison = program(opcode, DATA);
                poison.splice(5..7, [0x0f, 0x06]);
                let mut failures = vec![
                    (too_many, CompileError::InstructionLimit),
                    (
                        poison,
                        CompileError::Instruction {
                            pc: GuestAddress(CODE + 5),
                            cause: InstructionError::Decode(DecodeError::Unsupported(
                                UnsupportedFeature::Privileged,
                            )),
                        },
                    ),
                ];
                if !entries {
                    failures.push((
                        program(opcode, DATA)[..4].to_vec(),
                        CompileError::Instruction {
                            pc: GuestAddress(CODE),
                            cause: InstructionError::InvalidBlockEnd,
                        },
                    ));
                }
                for (input, expected) in failures {
                    let (mut engine, keeper) = prior();
                    upload(&mut engine, CODE, &input);
                    describe(&mut engine, CODE, input.len(), entries);
                    let before = saved(&engine, keeper);
                    assert_eq!(
                        publish(&mut engine, resident, entries),
                        Err(compile_error(resident, expected))
                    );
                    assert_eq!(saved(&engine, keeper), before);
                    engine.guard(KEY, before.generation).unwrap();
                    engine.guard_resident(KEY, keeper).unwrap();
                    assert_eq!(
                        engine.lookup_installed_resident(KEY, KEEP).unwrap().unit_id,
                        keeper
                    );
                    assert!(engine.lookup_resident(CODE).is_err());
                }
            }
        }
    }
}
