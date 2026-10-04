use ring3_engine::{
    memory::{Access, GuestAddress},
    process::EngineInstance,
};

#[test]
fn bound_byte_mov_admits_without_reading_unmapped_data() {
    let mut engine = EngineInstance::new(1, 0x1234_5678_9abc_def0).unwrap();
    let code = [0x88, 0x05, 0x10, 0x50, 0, 0, 0xeb, 0];
    engine.map(0x1000, 1, 7).unwrap();
    engine.arena_mut().unwrap()[140..148].copy_from_slice(&code);
    engine.upload(0x1000, 8).unwrap();
    engine.protect(0x1000, 1, 5).unwrap();
    assert!(
        engine
            .memory()
            .unwrap()
            .resolve(GuestAddress(0x5010), Access::Write)
            .is_err()
    );
    let transfer = &mut engine.arena_mut().unwrap()[140..148];
    transfer[..4].copy_from_slice(&0x1000_u32.to_le_bytes());
    transfer[4..8].copy_from_slice(&8_u32.to_le_bytes());
    engine
        .compile(1)
        .expect("bound byte MOV must admit without accessing guest data");
}

use ring3_engine::{
    cpu::{
        UnsupportedFeature,
        dbt::{
            BlockSpec, CompileError, CompileLimits, InstructionError, RegistryError,
            compile_entry_region, compile_region, prepare_entry_region, prepare_region,
        },
        x86::{
            Register32,
            decode::{DecodeError, decode_one},
            ir::{ByteRegister, ByteValue, EffectiveAddress, Operation},
        },
    },
    memory::{FaultReason, MemoryFault},
    process::{HostError, StoreCompletion},
};

const CODE: u32 = 0x1000;
const KEEP: u32 = 0x3000;
const DATA: u32 = 0x5000;
const KEY: u64 = 0x1234_5678_9abc_def0;
const SOURCES: [ByteRegister; 8] = [
    ByteRegister::Al,
    ByteRegister::Cl,
    ByteRegister::Dl,
    ByteRegister::Bl,
    ByteRegister::Ah,
    ByteRegister::Ch,
    ByteRegister::Dh,
    ByteRegister::Bh,
];

fn absolute(source: u8) -> Vec<u8> {
    vec![0x88, 0x05 | source << 3, 0x10, 0x50, 0, 0]
}

fn forms() -> Vec<Vec<u8>> {
    (0..8)
        .map(absolute)
        .chain([0, 0x80, 0xff].map(|value| vec![0xc6, 0x05, 0x10, 0x50, 0, 0, value]))
        .collect()
}
#[derive(Clone, Copy, Debug)]
enum Owner {
    Replacement,
    Resident,
}

fn upload(engine: &mut EngineInstance, pc: u32, bytes: &[u8]) {
    engine.arena_mut().unwrap()[140..140 + bytes.len()].copy_from_slice(bytes);
    engine.upload(pc, bytes.len() as u32).unwrap();
}

fn code(pc: u32, bytes: &[u8], execute_only: bool) -> EngineInstance {
    let base = pc & !0xfff;
    let pages = (u64::from(pc - base) + bytes.len() as u64).div_ceil(4096) as u32;
    let mut engine = EngineInstance::new(pages + 2, KEY).unwrap();
    engine.map(base, pages, 7).unwrap();
    upload(&mut engine, pc, bytes);
    if execute_only {
        engine.protect(base, pages, 4).unwrap();
    }
    engine
}

fn describe(engine: &mut EngineInstance, specs: &[(u32, usize)], entries: bool) {
    let transfer = &mut engine.arena_mut().unwrap()[140..];
    transfer.fill(0xa5);
    let stride = if entries { 4 } else { 8 };
    for (index, (pc, length)) in specs.iter().enumerate() {
        let offset = index * stride;
        transfer[offset..offset + 4].copy_from_slice(&pc.to_le_bytes());
        if !entries {
            transfer[offset + 4..offset + 8].copy_from_slice(&(*length as u32).to_le_bytes());
        }
    }
}

fn compile(
    engine: &mut EngineInstance,
    owner: Owner,
    entries: bool,
    count: u32,
) -> Result<u64, HostError> {
    match (owner, entries) {
        (Owner::Replacement, false) => engine.compile(count).map(u64::from),
        (Owner::Replacement, true) => engine.compile_entries(count, 0).map(u64::from),
        (Owner::Resident, false) => engine.compile_resident(count).map(|id| id.get()),
        (Owner::Resident, true) => engine.compile_resident_entries(count, 0).map(|id| id.get()),
    }
}

fn module(engine: &EngineInstance, owner: Owner, id: u64) -> &[u8] {
    match owner {
        Owner::Replacement => engine.artifact_bytes().unwrap(),
        Owner::Resident => engine.resident_bytes(id).unwrap(),
    }
}

fn guard(engine: &EngineInstance, owner: Owner, id: u64) -> Result<(), HostError> {
    match owner {
        Owner::Replacement => engine.guard(KEY, id as u32),
        Owner::Resident => engine.guard_resident(KEY, id),
    }
}

fn compile_error(owner: Owner, error: CompileError) -> HostError {
    match owner {
        Owner::Replacement => HostError::Compile(error),
        Owner::Resident => HostError::Resident(RegistryError::Compile(error)),
    }
}

fn instruction_error(pc: u32, cause: InstructionError) -> CompileError {
    CompileError::Instruction {
        pc: GuestAddress(pc),
        cause,
    }
}

fn byte(bytes: &mut &[u8]) -> u8 {
    let (&value, remaining) = bytes.split_first().unwrap();
    *bytes = remaining;
    value
}

fn unsigned(bytes: &mut &[u8]) -> u32 {
    let mut value = 0;
    for index in 0..5 {
        let next = byte(bytes);
        if index == 4 {
            assert_eq!(next & 0xf0, 0);
        }
        value |= u32::from(next & 0x7f) << (index * 7);
        if next & 0x80 == 0 {
            return value;
        }
    }
    panic!("unterminated unsigned LEB");
}

fn signed_word(bytes: &mut &[u8]) -> u32 {
    let mut value = 0_i64;
    for index in 0..5 {
        let next = byte(bytes);
        let bits = (index + 1) * 7;
        value |= i64::from(next & 0x7f) << (index * 7);
        if next & 0x80 == 0 {
            if next & 0x40 != 0 {
                value -= 1_i64 << bits;
            }
            assert!(i32::try_from(value).is_ok());
            return value as i32 as u32;
        }
    }
    panic!("unterminated signed LEB");
}

fn take<'a>(bytes: &mut &'a [u8], length: usize) -> &'a [u8] {
    let (value, remaining) = bytes.split_at(length);
    *bytes = remaining;
    value
}

fn name<'a>(bytes: &mut &'a [u8]) -> &'a str {
    let length = unsigned(bytes) as usize;
    std::str::from_utf8(take(bytes, length)).unwrap()
}

fn section(wasm: &[u8], requested: u8) -> &[u8] {
    assert_eq!(&wasm[..8], b"\0asm\x01\0\0\0");
    let mut bytes = &wasm[8..];
    while !bytes.is_empty() {
        let kind = byte(&mut bytes);
        let length = unsigned(&mut bytes) as usize;
        let value = take(&mut bytes, length);
        if kind == requested {
            return value;
        }
    }
    panic!("missing section {requested}");
}

#[derive(Debug, PartialEq, Eq)]
struct Saved {
    arena: Vec<u8>,
    arena_pointer: usize,
    generation: u32,
    replacement: Vec<u8>,
    replacement_pointer: usize,
    resident: Vec<u8>,
    resident_pointer: usize,
    mapped_pages: u32,
}

fn saved(engine: &EngineInstance, keep: u64) -> Saved {
    let replacement = engine.artifact_bytes().unwrap();
    let resident = engine.resident_bytes(keep).unwrap();
    Saved {
        arena: engine.arena().to_vec(),
        arena_pointer: engine.arena_address(),
        generation: engine.generation(),
        replacement: replacement.to_vec(),
        replacement_pointer: replacement.as_ptr() as usize,
        resident: resident.to_vec(),
        resident_pointer: resident.as_ptr() as usize,
        mapped_pages: engine.memory().unwrap().mapped_pages(),
    }
}

fn prior_owners() -> (EngineInstance, u64) {
    let mut engine = EngineInstance::new(2, KEY).unwrap();
    engine.map(CODE, 1, 7).unwrap();
    engine.map(KEEP, 1, 7).unwrap();
    upload(&mut engine, KEEP, &[0x90, 0xeb, 0]);
    describe(&mut engine, &[(KEEP, 3)], false);
    engine.compile(1).unwrap();
    describe(&mut engine, &[(KEEP, 3)], false);
    let id = engine.compile_resident(1).unwrap().get();
    (engine, id)
}

fn assert_retained(engine: &EngineInstance, keep: u64, before: &Saved) {
    assert_eq!(&saved(engine, keep), before);
    engine.guard(KEY, before.generation).unwrap();
    engine.guard_resident(KEY, keep).unwrap();
    assert_eq!(engine.lookup_resident(KEEP).unwrap().get(), keep);
    assert!(engine.lookup_resident(CODE).is_err());
}

fn assert_module(engine: &EngineInstance, owner: Owner, id: u64, mixed: bool) {
    let wasm = module(engine, owner, id);
    let resident = matches!(owner, Owner::Resident);
    let mut types = section(wasm, 1);
    let mut arities = vec![4, if resident { 7 } else { 6 }];
    if mixed {
        arities.push(1);
    }
    arities.push(if resident { 6 } else { 2 });
    assert_eq!(unsigned(&mut types) as usize, arities.len());
    for arity in arities {
        assert_eq!(byte(&mut types), 0x60);
        assert_eq!(unsigned(&mut types), arity);
        assert_eq!(take(&mut types, arity as usize), vec![0x7f; arity as usize]);
        assert_eq!((unsigned(&mut types), byte(&mut types)), (1, 0x7f));
    }
    assert!(types.is_empty());
    let store_name = if resident {
        "store_resident8"
    } else {
        "store8"
    };
    let mut functions = vec![(if resident { "guard_resident" } else { "guard" }, 1)];
    if mixed {
        functions.extend([
            ("read32", 2),
            (
                if resident {
                    "store_resident32"
                } else {
                    "store32"
                },
                3,
            ),
            ("read8", 2),
            ("read16", 2),
        ]);
    }
    functions.push((store_name, if mixed { 3 } else { 2 }));
    let mut imports = section(wasm, 2);
    assert_eq!(unsigned(&mut imports) as usize, functions.len() + 1);
    assert_eq!((name(&mut imports), name(&mut imports)), ("env", "memory"));
    assert_eq!(byte(&mut imports), 2);
    assert_eq!((unsigned(&mut imports), unsigned(&mut imports)), (0, 1));
    for (expected_name, expected_type) in &functions {
        assert_eq!(
            (name(&mut imports), name(&mut imports)),
            ("ring3", *expected_name)
        );
        assert_eq!(
            (byte(&mut imports), unsigned(&mut imports)),
            (0, *expected_type)
        );
    }
    assert!(imports.is_empty());
    let mut exports = section(wasm, 7);
    assert_eq!(unsigned(&mut exports), 1);
    assert_eq!(name(&mut exports), "run");
    assert_eq!(
        (byte(&mut exports), unsigned(&mut exports)),
        (0, functions.len() as u32)
    );
    assert!(exports.is_empty());
    let mut bodies = section(wasm, 10);
    assert_eq!(unsigned(&mut bodies), 1);
    let length = unsigned(&mut bodies) as usize;
    let mut body = take(&mut bodies, length);
    assert!(bodies.is_empty());
    assert_eq!(unsigned(&mut body), 3);
    for expected in [(16, 0x7f), (1, 0x7e), (6, 0x7f)] {
        assert_eq!((unsigned(&mut body), byte(&mut body)), expected);
    }
    let mut constants = vec![KEY as u32, (KEY >> 32) as u32, id as u32];
    if resident {
        constants.push((id >> 32) as u32);
    }
    for value in constants {
        assert_eq!(byte(&mut body), 0x41);
        assert_eq!(signed_word(&mut body), value);
    }
    for local in [0, 1, 3] {
        assert_eq!((byte(&mut body), unsigned(&mut body)), (0x20, local));
    }
    assert_eq!((byte(&mut body), unsigned(&mut body)), (0x10, 0));
}

#[test]
fn all_legacy_sources_and_immediate_bits_admit_all_four_bound_paths_without_data() {
    for (index, instruction) in forms().into_iter().enumerate() {
        let mut bytes = instruction.clone();
        bytes.extend([0xeb, 0]);
        for owner in [Owner::Replacement, Owner::Resident] {
            for entries in [false, true] {
                let mut engine = code(CODE, &bytes, true);
                let decoded = decode_one(engine.memory().unwrap(), GuestAddress(CODE)).unwrap();
                let source = if index < 8 {
                    ByteValue::Register(SOURCES[index])
                } else {
                    ByteValue::Immediate([0, 0x80, 0xff][index - 8])
                };
                assert_eq!(
                    decoded.operation(),
                    &Operation::StoreByte {
                        address: EffectiveAddress {
                            base: None,
                            index: None,
                            scale: 1,
                            displacement: DATA + 0x10
                        },
                        source,
                    }
                );
                assert_eq!(decoded.length() as usize, instruction.len());
                assert_eq!(
                    decoded.next_pc(),
                    GuestAddress(CODE + instruction.len() as u32)
                );
                describe(&mut engine, &[(CODE, bytes.len())], entries);
                let before = engine.arena().to_vec();
                let snapshot = engine
                    .memory()
                    .unwrap()
                    .snapshot_code(GuestAddress(CODE), bytes.len())
                    .unwrap();
                let id = compile(&mut engine, owner, entries, 1).unwrap();
                assert_eq!(engine.arena(), before);
                assert_eq!(engine.memory().unwrap().mapped_pages(), 1);
                assert!(engine.memory().unwrap().is_code_current(&snapshot));
                assert!(
                    engine
                        .memory()
                        .unwrap()
                        .resolve(GuestAddress(DATA + 0x10), Access::Write)
                        .is_err()
                );
                assert!(
                    engine
                        .memory()
                        .unwrap()
                        .resolve(GuestAddress(CODE), Access::Read)
                        .is_err()
                );
                assert_module(&engine, owner, id, false);
                guard(&engine, owner, id).unwrap();
                if matches!(owner, Owner::Resident) {
                    for pc in [CODE, CODE + instruction.len() as u32] {
                        assert_eq!(engine.lookup_resident(pc).unwrap().get(), id);
                    }
                }
            }
        }
    }
}

#[test]
fn original_parent_aliases_sib_and_signed_displacements_have_exact_descriptors() {
    let cases: &[(&[u8], EffectiveAddress, ByteValue)] = &[
        (
            &[0x88, 0x20],
            EffectiveAddress {
                base: Some(Register32::Eax),
                index: None,
                scale: 1,
                displacement: 0,
            },
            ByteValue::Register(ByteRegister::Ah),
        ),
        (
            &[0x88, 0xac, 0x8b, 0xe6, 0x32, 0x77, 0xbb],
            EffectiveAddress {
                base: Some(Register32::Ebx),
                index: Some(Register32::Ecx),
                scale: 4,
                displacement: 0xbb77_32e6,
            },
            ByteValue::Register(ByteRegister::Ch),
        ),
        (
            &[0x88, 0x7c, 0x24, 0xff],
            EffectiveAddress {
                base: Some(Register32::Esp),
                index: None,
                scale: 1,
                displacement: u32::MAX,
            },
            ByteValue::Register(ByteRegister::Bh),
        ),
        (
            &[0x88, 0x14, 0x4d, 0x80, 0xb9, 0xbb, 0xdd],
            EffectiveAddress {
                base: None,
                index: Some(Register32::Ecx),
                scale: 2,
                displacement: 0xddbb_b980,
            },
            ByteValue::Register(ByteRegister::Dl),
        ),
        (
            &[0xc6, 0x46, 0x10, 0xff],
            EffectiveAddress {
                base: Some(Register32::Esi),
                index: None,
                scale: 1,
                displacement: 0x10,
            },
            ByteValue::Immediate(0xff),
        ),
    ];
    for &(instruction, address, source) in cases {
        let mut bytes = instruction.to_vec();
        bytes.extend([0xeb, 0]);
        let engine = code(CODE, &bytes, true);
        let decoded = decode_one(engine.memory().unwrap(), GuestAddress(CODE)).unwrap();
        assert_eq!(
            decoded.operation(),
            &Operation::StoreByte { address, source }
        );
        assert_eq!(decoded.length() as usize, instruction.len());
        for owner in [Owner::Replacement, Owner::Resident] {
            for entries in [false, true] {
                let mut engine = code(CODE, &bytes, true);
                describe(&mut engine, &[(CODE, bytes.len())], entries);
                let id = compile(&mut engine, owner, entries, 1).unwrap();
                assert_module(&engine, owner, id, false);
            }
        }
    }
}

#[test]
fn standalone_explicit_and_cold_profiles_refuse_only_the_memory_backend() {
    for instruction in forms() {
        let mut bytes = instruction;
        bytes.extend([0xeb, 0]);
        let engine = code(CODE, &bytes, true);
        let memory = engine.memory().unwrap();
        let specs = [BlockSpec {
            entry: GuestAddress(CODE),
            byte_length: bytes.len() as u32,
        }];
        let entries = [GuestAddress(CODE)];
        let error = instruction_error(CODE, InstructionError::BackendUnsupported);
        assert_eq!(
            prepare_region(memory, &specs, CompileLimits::default()).err(),
            Some(error)
        );
        assert_eq!(
            compile_region(memory, &specs, CompileLimits::default()).err(),
            Some(error)
        );
        assert_eq!(
            prepare_entry_region(memory, &entries, CompileLimits::default()).err(),
            Some(error)
        );
        assert_eq!(
            compile_entry_region(memory, &entries, CompileLimits::default()).err(),
            Some(error)
        );
    }
}

#[test]
fn byte_destinations_adjacent_forms_and_excluded_prefixes_keep_existing_categories() {
    let opcode = DecodeError::Unsupported(UnsupportedFeature::Opcode);
    let cases: &[(&[u8], DecodeError)] = &[
        (&[0x10, 0xd0], opcode),
        (&[0xc6, 0xc0, 0], opcode),
        (&[0x18, 0xd0], opcode),
        (&[0xa2, 0x10, 0x50, 0, 0], opcode),
        (&[0x10, 0xd0], opcode),
        (&[0xc6, 0x0b, 0], DecodeError::InvalidEncoding),
        (&[0x66, 0x88, 0x03], opcode),
        (&[0x67, 0x88, 0x03], opcode),
        (&[0xf3, 0x88, 0x03], opcode),
        (&[0xf0, 0x88, 0x03], DecodeError::InvalidEncoding),
        (
            &[0x64, 0x88, 0x03],
            DecodeError::Unsupported(UnsupportedFeature::Segment),
        ),
    ];
    for &(bytes, expected) in cases {
        let engine = code(CODE, bytes, false);
        assert_eq!(
            decode_one(engine.memory().unwrap(), GuestAddress(CODE)).err(),
            Some(expected)
        );
    }
}

#[test]
fn byte_only_and_mixed_modules_keep_typed_import_order_and_owner_bindings() {
    let bytes = [
        0x8b, 0x03, 0x89, 0x03, 0x0f, 0xb6, 0x03, 0x0f, 0xb7, 0x03, 0x88, 0x23, 0xeb, 0,
    ];
    for owner in [Owner::Replacement, Owner::Resident] {
        for entries in [false, true] {
            let mut engine = code(CODE, &bytes, false);
            describe(&mut engine, &[(CODE, bytes.len())], entries);
            let id = compile(&mut engine, owner, entries, 1).unwrap();
            assert_module(&engine, owner, id, true);
        }
    }
}

#[test]
fn fetch_end_missing_immediate_and_split_seeds_preserve_prior_publication() {
    for owner in [Owner::Replacement, Owner::Resident] {
        for entries in [false, true] {
            for instruction in [absolute(4), vec![0xc6, 0x05, 0x10, 0x50, 0, 0, 0xff]] {
                let mut bytes = instruction;
                bytes.extend([0xeb, 0]);
                let pc = 0x2000 - bytes.len() as u32;
                let mut engine = code(pc, &bytes, true);
                describe(&mut engine, &[(pc, bytes.len())], entries);
                compile(&mut engine, owner, entries, 1).unwrap();
                assert!(
                    engine
                        .memory()
                        .unwrap()
                        .resolve(GuestAddress(0x2000), Access::Execute)
                        .is_err()
                );
            }
            let (mut engine, keep) = prior_owners();
            let pc = 0x1ffa;
            upload(&mut engine, pc, &[0xc6, 0x05, 0x10, 0x50, 0, 0]);
            describe(&mut engine, &[(pc, 7)], entries);
            let before = saved(&engine, keep);
            let error = instruction_error(
                pc,
                InstructionError::Decode(DecodeError::MemoryFault {
                    pc: GuestAddress(pc),
                    fault: MemoryFault {
                        address: GuestAddress(0x2000),
                        access: Access::Execute,
                        reason: FaultReason::Unmapped,
                    },
                    length: 7,
                }),
            );
            assert_eq!(
                compile(&mut engine, owner, entries, 1),
                Err(compile_error(owner, error))
            );
            assert_retained(&engine, keep, &before);
            upload(
                &mut engine,
                CODE,
                &[0xc6, 0x05, 0x10, 0x50, 0, 0, 0xff, 0xeb, 0],
            );
            let specs = if entries {
                vec![(CODE, 0), (CODE + 6, 0)]
            } else {
                vec![(CODE, 6)]
            };
            describe(&mut engine, &specs, entries);
            let before = saved(&engine, keep);
            let error = if entries {
                CompileError::InvalidBlocks
            } else {
                instruction_error(CODE, InstructionError::InvalidBlockEnd)
            };
            assert_eq!(
                compile(&mut engine, owner, entries, specs.len() as u32),
                Err(compile_error(owner, error))
            );
            assert_retained(&engine, keep, &before);
        }
    }
}

#[test]
fn adjacent_byte_seeds_and_instruction_limit_keep_canonical_boundaries() {
    let adjacent = [0x88, 0x23, 0xc6, 0x43, 8, 0x80, 0xeb, 0, 0x0f, 0x0b];
    for owner in [Owner::Replacement, Owner::Resident] {
        for entries in [false, true] {
            let mut engine = code(CODE, &adjacent, false);
            describe(&mut engine, &[(CODE, 2), (CODE + 2, 6)], entries);
            let id = compile(&mut engine, owner, entries, 2).unwrap();
            guard(&engine, owner, id).unwrap();
            if matches!(owner, Owner::Resident) {
                for pc in [CODE, CODE + 2, CODE + 6] {
                    assert_eq!(engine.lookup_resident(pc).unwrap().get(), id);
                }
                assert!(engine.lookup_resident(CODE + 3).is_err());
            }
            let mut bytes = vec![0x90; 62];
            bytes.extend([0x88, 0x03, 0xeb, 0]);
            let mut engine = code(CODE, &bytes, false);
            describe(&mut engine, &[(CODE, bytes.len())], entries);
            compile(&mut engine, owner, entries, 1).unwrap();
            let (mut engine, keep) = prior_owners();
            let mut bytes = vec![0x90; 63];
            bytes.extend([0x88, 0x03, 0x0f, 0x0b]);
            upload(&mut engine, CODE, &bytes);
            describe(&mut engine, &[(CODE, bytes.len())], entries);
            let before = saved(&engine, keep);
            assert_eq!(
                compile(&mut engine, owner, entries, 1),
                Err(compile_error(owner, CompileError::InstructionLimit))
            );
            assert_retained(&engine, keep, &before);
        }
    }
}

#[test]
fn consumed_cross_page_snapshots_ignore_data_changes_and_reject_same_code_write() {
    let bytes = [0xc6, 0x05, 0x10, 0x50, 0, 0, 0xff, 0xeb, 0];
    for owner in [Owner::Replacement, Owner::Resident] {
        for entries in [false, true] {
            let mut engine = code(0x1ffc, &bytes, false);
            let snapshot = engine
                .memory()
                .unwrap()
                .snapshot_code(GuestAddress(0x1ffc), bytes.len())
                .unwrap();
            describe(&mut engine, &[(0x1ffc, bytes.len())], entries);
            let id = compile(&mut engine, owner, entries, 1).unwrap();
            let old_bytes = module(&engine, owner, id).to_vec();
            engine.map(DATA, 1, 7).unwrap();
            engine.write8(DATA + 0x10, 0xff).unwrap();
            engine.protect(DATA, 1, 2).unwrap();
            engine.unmap(DATA, 1).unwrap();
            assert!(engine.memory().unwrap().is_code_current(&snapshot));
            assert_eq!(module(&engine, owner, id), old_bytes);
            guard(&engine, owner, id).unwrap();
            engine.write8(0x2002, 0xff).unwrap();
            assert!(!engine.memory().unwrap().is_code_current(&snapshot));
            assert_eq!(
                guard(&engine, owner, id),
                Err(if matches!(owner, Owner::Resident) {
                    HostError::Resident(RegistryError::CodeInvalidated)
                } else {
                    HostError::CodeInvalidated
                })
            );
        }
    }
}

fn helper_packet(fields: [u32; 6]) -> [u8; 40] {
    let mut bytes = [0; 40];
    bytes[..16].copy_from_slice(&[82, 51, 77, 72, 3, 0, 1, 0, 40, 0, 0, 0, 0, 0, 0, 0]);
    for (index, field) in fields.into_iter().enumerate() {
        bytes[16 + index * 4..20 + index * 4].copy_from_slice(&field.to_le_bytes());
    }
    bytes
}

fn assert_helper_only(engine: &EngineInstance, before: &[u8], fields: [u32; 6]) {
    let mut expected = before.to_vec();
    expected[100..140].copy_from_slice(&helper_packet(fields));
    assert_eq!(engine.arena(), expected);
}

fn ram(engine: &EngineInstance, address: u32, length: usize) -> Vec<u8> {
    let mut bytes = vec![0; length];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(address), &mut bytes)
        .unwrap();
    bytes
}

fn owned(owner: Owner, pc: u32) -> (EngineInstance, u64) {
    let mut engine = code(pc, &[0x88, 0x03, 0xeb, 0], false);
    describe(&mut engine, &[(pc, 4)], false);
    let id = compile(&mut engine, owner, false, 1).unwrap();
    (engine, id)
}

fn store(
    engine: &mut EngineInstance,
    owner: Owner,
    id: u64,
    address: u32,
    value: u32,
) -> Result<StoreCompletion, HostError> {
    match owner {
        Owner::Replacement => engine.store8(address, value),
        Owner::Resident => engine.store_resident8(KEY, id, address, value),
    }
}

#[test]
fn direct_write_truncates_host_values_requires_no_artifact_and_preserves_whole_arena() {
    let mut engine = EngineInstance::new(1, KEY).unwrap();
    engine.map(DATA, 1, 7).unwrap();
    upload(&mut engine, DATA, &[0x11, 0x22, 0x33]);
    engine.protect(DATA, 1, 2).unwrap();
    engine.arena_mut().unwrap()[..100].fill(0xa5);
    let pointer = engine.arena_address();
    for value in [0, 0x100, 0x1234_5680, u32::MAX] {
        let before = engine.arena().to_vec();
        engine.write8(DATA + 1, value).unwrap();
        assert_helper_only(&engine, &before, [0, 0, 0, 0, 0, 1]);
        assert_eq!(engine.generation(), 0);
        assert_eq!(engine.arena_address(), pointer);
        assert_eq!(engine.artifact_bytes(), Err(HostError::InvalidArtifact));
        engine.protect(DATA, 1, 3).unwrap();
        assert_eq!(ram(&engine, DATA, 3), [0x11, value as u8, 0x33]);
        engine.protect(DATA, 1, 2).unwrap();
    }
}

#[test]
fn both_owned_helpers_write_only_the_final_byte_without_read_or_wrap() {
    for owner in [Owner::Replacement, Owner::Resident] {
        for (address, base) in [(0x1fff, 0x1000), (u32::MAX, 0xffff_f000)] {
            let (mut engine, id) = owned(owner, 0x8000);
            engine.map(base, 1, 7).unwrap();
            engine.map(0, 1, 3).unwrap();
            upload(&mut engine, address - 1, &[0x7f, 0x6d]);
            upload(&mut engine, 0, &[0x5a]);
            engine.protect(base, 1, 2).unwrap();
            assert!(
                engine
                    .memory()
                    .unwrap()
                    .resolve(GuestAddress(address), Access::Read)
                    .is_err()
            );
            assert!(
                engine
                    .memory()
                    .unwrap()
                    .resolve(GuestAddress(address), Access::Write)
                    .is_ok()
            );
            if address == 0x1fff {
                assert!(
                    engine
                        .memory()
                        .unwrap()
                        .resolve(GuestAddress(0x2000), Access::Write)
                        .is_err()
                );
            }
            engine.arena_mut().unwrap()[96..100].copy_from_slice(&1_u32.to_le_bytes());
            let before = engine.arena().to_vec();
            let bytes = module(&engine, owner, id).to_vec();
            let pointer = module(&engine, owner, id).as_ptr();
            assert_eq!(
                store(&mut engine, owner, id, address, 0x1ff),
                Ok(StoreCompletion::Complete)
            );
            assert_helper_only(&engine, &before, [0, 0, 0, 0, 0, 1]);
            guard(&engine, owner, id).unwrap();
            assert_eq!(module(&engine, owner, id), bytes);
            assert_eq!(module(&engine, owner, id).as_ptr(), pointer);
            assert_eq!(ram(&engine, 0, 1), [0x5a]);
            engine.protect(base, 1, 3).unwrap();
            assert_eq!(ram(&engine, address - 1, 2), [0x7f, 0xff]);
        }
    }
}

#[test]
fn exact_faults_preserve_cpu_and_current_owners_then_data_only_repair_retries() {
    for owner in [Owner::Replacement, Owner::Resident] {
        for permission in [false, true] {
            let (mut engine, id) = owned(owner, CODE);
            if permission {
                engine.map(DATA, 1, 7).unwrap();
                upload(&mut engine, DATA, &[0x11, 0x22, 0x33]);
                engine.protect(DATA, 1, 5).unwrap();
            }
            let data_snapshot = permission.then(|| {
                engine
                    .memory()
                    .unwrap()
                    .snapshot_code(GuestAddress(DATA), 3)
                    .unwrap()
            });
            let snapshot = engine
                .memory()
                .unwrap()
                .snapshot_code(GuestAddress(CODE), 4)
                .unwrap();
            let bytes = module(&engine, owner, id).to_vec();
            let pointer = module(&engine, owner, id).as_ptr();
            engine.arena_mut().unwrap()[..100].fill(0xa5);
            let before = engine.arena().to_vec();
            assert_eq!(
                store(&mut engine, owner, id, DATA + 1, 0xab),
                Ok(StoreCompletion::Complete)
            );
            assert_helper_only(
                &engine,
                &before,
                [1, 0, if permission { 2 } else { 1 }, DATA + 1, 2, 1],
            );
            guard(&engine, owner, id).unwrap();
            assert!(engine.memory().unwrap().is_code_current(&snapshot));
            if let Some(snapshot) = &data_snapshot {
                assert!(engine.memory().unwrap().is_code_current(snapshot));
            }
            assert_eq!(module(&engine, owner, id), bytes);
            assert_eq!(module(&engine, owner, id).as_ptr(), pointer);
            if permission {
                assert_eq!(ram(&engine, DATA, 3), [0x11, 0x22, 0x33]);
            } else {
                engine.map(DATA, 1, 3).unwrap();
            }
            if permission {
                engine.protect(DATA, 1, 3).unwrap();
            }
            let before_retry = engine.arena().to_vec();
            assert_eq!(
                store(&mut engine, owner, id, DATA + 1, 0xab),
                Ok(StoreCompletion::Complete)
            );
            assert_helper_only(&engine, &before_retry, [0, 0, 0, 0, 0, 1]);
            assert_eq!(
                ram(&engine, DATA, 3),
                if permission {
                    vec![0x11, 0xab, 0x33]
                } else {
                    vec![0, 0xab, 0]
                }
            );
            guard(&engine, owner, id).unwrap();
        }
    }
}

#[test]
fn full_key_and_id_guards_stale_code_and_close_precede_any_helper_or_ram_effect() {
    let mut empty = EngineInstance::new(1, KEY).unwrap();
    let before = empty.arena().to_vec();
    assert_eq!(
        empty.store8(u32::MAX, 0xff),
        Err(HostError::InvalidArtifact)
    );
    assert_eq!(empty.arena(), before);
    let (mut engine, id) = owned(Owner::Resident, CODE);
    engine.map(DATA, 1, 3).unwrap();
    for (key, invalid_id, expected) in [
        (KEY ^ (1 << 32), id, HostError::InvalidArtifact),
        (KEY ^ 1, id, HostError::InvalidArtifact),
        (KEY, 0, HostError::Resident(RegistryError::InvalidUnit)),
        (
            KEY,
            id ^ (1 << 32),
            HostError::Resident(RegistryError::InvalidUnit),
        ),
    ] {
        let before = engine.arena().to_vec();
        assert_eq!(
            engine.store_resident8(key, invalid_id, DATA, 0xff),
            Err(expected)
        );
        assert_eq!(engine.arena(), before);
        assert_eq!(ram(&engine, DATA, 1), [0]);
    }
    for owner in [Owner::Replacement, Owner::Resident] {
        let (mut engine, id) = owned(owner, CODE);
        engine.write8(CODE, 0x88).unwrap();
        let before = engine.arena().to_vec();
        let expected = if matches!(owner, Owner::Resident) {
            HostError::Resident(RegistryError::CodeInvalidated)
        } else {
            HostError::CodeInvalidated
        };
        assert_eq!(store(&mut engine, owner, id, DATA, 0xff), Err(expected));
        assert_eq!(engine.arena(), before);
        engine.close();
        let before = engine.arena().to_vec();
        assert_eq!(engine.write8(DATA, 0xff), Err(HostError::Closed));
        assert_eq!(engine.store8(DATA, 0xff), Err(HostError::Closed));
        assert_eq!(
            engine.store_resident8(0, 0, DATA, 0xff),
            Err(HostError::Closed)
        );
        assert_eq!(engine.arena(), before);
    }
}

#[test]
fn successful_changed_and_same_byte_stores_invalidate_only_affected_owner_pages() {
    for owner in [Owner::Replacement, Owner::Resident] {
        for value in [0x88, 0x90] {
            let (mut engine, id) = owned(owner, CODE);
            let before = engine.arena().to_vec();
            assert_eq!(
                store(&mut engine, owner, id, CODE, value),
                Ok(StoreCompletion::CodeInvalidated)
            );
            assert_helper_only(&engine, &before, [0, 0, 0, 0, 0, 1]);
            assert_eq!(ram(&engine, CODE, 4), [value as u8, 0x03, 0xeb, 0]);
            assert!(guard(&engine, owner, id).is_err());
        }
        let (mut engine, id) = owned(owner, CODE);
        engine.map(KEEP, 1, 7).unwrap();
        upload(&mut engine, KEEP, &[0x90, 0xeb, 0]);
        describe(&mut engine, &[(KEEP, 3)], false);
        let other = engine.compile_resident(1).unwrap().get();
        let before = engine.arena().to_vec();
        assert_eq!(
            store(&mut engine, owner, id, KEEP + 0x100, 0),
            Ok(StoreCompletion::Complete)
        );
        assert_helper_only(&engine, &before, [0, 0, 0, 0, 0, 1]);
        guard(&engine, owner, id).unwrap();
        assert_eq!(
            engine.guard_resident(KEY, other),
            Err(HostError::Resident(RegistryError::CodeInvalidated))
        );
        let before = engine.arena().to_vec();
        assert_eq!(
            store(&mut engine, owner, id, CODE + 0x100, 0),
            Ok(StoreCompletion::CodeInvalidated)
        );
        assert_helper_only(&engine, &before, [0, 0, 0, 0, 0, 1]);
    }
}
