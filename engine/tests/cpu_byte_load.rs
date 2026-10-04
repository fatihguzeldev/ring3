use ring3_engine::{
    memory::{Access, GuestAddress},
    process::EngineInstance,
};

#[test]
fn bound_byte_load_admits_without_reading_unmapped_data() {
    let mut engine = EngineInstance::new(1, 0x1234_5678_9abc_def0).unwrap();
    let code = [0x8a, 0x03, 0xeb, 0];
    engine.map(0x1000, 1, 7).unwrap();
    engine.arena_mut().unwrap()[140..144].copy_from_slice(&code);
    engine.upload(0x1000, 4).unwrap();
    engine.protect(0x1000, 1, 4).unwrap();
    assert!(
        engine
            .memory()
            .unwrap()
            .resolve(GuestAddress(0), Access::Read)
            .is_err()
    );
    let transfer = &mut engine.arena_mut().unwrap()[140..148];
    transfer[..4].copy_from_slice(&0x1000_u32.to_le_bytes());
    transfer[4..8].copy_from_slice(&4_u32.to_le_bytes());
    engine
        .compile(1)
        .expect("bound byte load must admit without reading guest data");
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
            ir::{ByteRegister, EffectiveAddress, Operation},
        },
    },
    memory::{FaultReason, MemoryFault},
    process::HostError,
};

const CODE: u32 = 0x1000;
const KEEP: u32 = 0x3000;
const DATA: u32 = 0x5000;
const KEY: u64 = 0x1234_5678_9abc_def0;
const DESTINATIONS: [ByteRegister; 8] = [
    ByteRegister::Al,
    ByteRegister::Cl,
    ByteRegister::Dl,
    ByteRegister::Bl,
    ByteRegister::Ah,
    ByteRegister::Ch,
    ByteRegister::Dh,
    ByteRegister::Bh,
];

fn absolute(destination: u8) -> Vec<u8> {
    vec![0x8a, 0x05 | destination << 3, 0x10, 0x50, 0, 0]
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
    let mut arities = vec![4, if resident { 7 } else { 6 }, 1];
    if mixed {
        arities.push(if resident { 6 } else { 2 });
    }
    assert_eq!(unsigned(&mut types) as usize, arities.len());
    for arity in arities {
        assert_eq!(byte(&mut types), 0x60);
        assert_eq!(unsigned(&mut types), arity);
        assert_eq!(take(&mut types, arity as usize), vec![0x7f; arity as usize]);
        assert_eq!((unsigned(&mut types), byte(&mut types)), (1, 0x7f));
    }
    assert!(types.is_empty());
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
            (
                if resident {
                    "store_resident8"
                } else {
                    "store8"
                },
                3,
            ),
        ]);
    } else {
        functions.push(("read8", 2));
    }
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
    let mut declarations = section(wasm, 3);
    assert_eq!(
        (unsigned(&mut declarations), unsigned(&mut declarations)),
        (1, 0)
    );
    assert!(declarations.is_empty());
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
fn all_eight_destinations_decode_and_admit_all_four_bound_paths_without_data() {
    for (index, destination) in DESTINATIONS.into_iter().enumerate() {
        let instruction = absolute(index as u8);
        let mut bytes = instruction.clone();
        bytes.extend([0xeb, 0]);
        for owner in [Owner::Replacement, Owner::Resident] {
            for entries in [false, true] {
                let mut engine = code(CODE, &bytes, true);
                let decoded = decode_one(engine.memory().unwrap(), GuestAddress(CODE)).unwrap();
                assert_eq!(
                    decoded.operation(),
                    &Operation::LoadByte {
                        destination,
                        address: EffectiveAddress {
                            base: None,
                            index: None,
                            scale: 1,
                            displacement: DATA + 0x10
                        },
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
                        .resolve(GuestAddress(DATA + 0x10), Access::Read)
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
fn original_parent_aliases_all_scales_and_signed_displacements_have_exact_descriptors() {
    let cases: &[(&[u8], ByteRegister, EffectiveAddress)] = &[
        (
            &[0x8a, 0x00],
            ByteRegister::Al,
            EffectiveAddress {
                base: Some(Register32::Eax),
                index: None,
                scale: 1,
                displacement: 0,
            },
        ),
        (
            &[0x8a, 0x20],
            ByteRegister::Ah,
            EffectiveAddress {
                base: Some(Register32::Eax),
                index: None,
                scale: 1,
                displacement: 0,
            },
        ),
        (
            &[0x8a, 0x6c, 0x8b, 0xfe],
            ByteRegister::Ch,
            EffectiveAddress {
                base: Some(Register32::Ebx),
                index: Some(Register32::Ecx),
                scale: 4,
                displacement: 0xffff_fffe,
            },
        ),
        (
            &[0x8a, 0x7c, 0x24, 0xff],
            ByteRegister::Bh,
            EffectiveAddress {
                base: Some(Register32::Esp),
                index: None,
                scale: 1,
                displacement: u32::MAX,
            },
        ),
        (
            &[0x8a, 0x14, 0x4d, 0x80, 0xb9, 0xbb, 0xdd],
            ByteRegister::Dl,
            EffectiveAddress {
                base: None,
                index: Some(Register32::Ecx),
                scale: 2,
                displacement: 0xddbb_b980,
            },
        ),
        (
            &[0x8a, 0x44, 0xc1, 0xff],
            ByteRegister::Al,
            EffectiveAddress {
                base: Some(Register32::Ecx),
                index: Some(Register32::Eax),
                scale: 8,
                displacement: u32::MAX,
            },
        ),
        (
            &[0x8a, 0x3d, 0xff, 0xff, 0xff, 0xff],
            ByteRegister::Bh,
            EffectiveAddress {
                base: None,
                index: None,
                scale: 1,
                displacement: u32::MAX,
            },
        ),
    ];
    for &(instruction, destination, address) in cases {
        let mut bytes = instruction.to_vec();
        bytes.extend([0xeb, 0]);
        for owner in [Owner::Replacement, Owner::Resident] {
            for entries in [false, true] {
                let mut engine = code(CODE, &bytes, true);
                let decoded = decode_one(engine.memory().unwrap(), GuestAddress(CODE)).unwrap();
                assert_eq!(
                    decoded.operation(),
                    &Operation::LoadByte {
                        destination,
                        address
                    }
                );
                assert_eq!(decoded.length() as usize, instruction.len());
                assert_eq!(
                    decoded.next_pc(),
                    GuestAddress(CODE + instruction.len() as u32)
                );
                describe(&mut engine, &[(CODE, bytes.len())], entries);
                let id = compile(&mut engine, owner, entries, 1).unwrap();
                assert_module(&engine, owner, id, false);
            }
        }
    }
}

#[test]
fn standalone_explicit_and_cold_profiles_refuse_only_the_memory_backend() {
    for index in 0..8 {
        let mut bytes = absolute(index);
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
fn register_sources_adjacent_forms_and_excluded_prefixes_keep_existing_categories() {
    let opcode = DecodeError::Unsupported(UnsupportedFeature::Opcode);
    let cases: &[(&[u8], DecodeError)] = &[
        (&[0x8a, 0xc3], opcode),
        (&[0x88, 0xc3], opcode),
        (&[0xc6, 0xc0, 0], opcode),
        (&[0xb0, 0x80], opcode),
        (&[0xa0, 0x10, 0x50, 0, 0], opcode),
        (&[0x66, 0x8a, 0x03], opcode),
        (&[0x67, 0x8a, 0x03], opcode),
        (&[0xf2, 0x8a, 0x03], opcode),
        (&[0xf3, 0x8a, 0x03], opcode),
        (&[0xf0, 0x8a, 0x03], DecodeError::InvalidEncoding),
        (
            &[0x64, 0x8a, 0x03],
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
fn byte_only_and_mixed_modules_keep_typed_import_order_locals_and_owner_bindings() {
    let bytes = [
        0x8b, 0x03, 0x89, 0x03, 0x8a, 0x23, 0x0f, 0xb6, 0x03, 0x0f, 0xb7, 0x03, 0x0f, 0xbe, 0x03,
        0x0f, 0xbf, 0x03, 0x88, 0x23, 0xeb, 0,
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
fn fetch_end_missing_operands_and_poison_fail_atomically_with_exact_priority() {
    for owner in [Owner::Replacement, Owner::Resident] {
        for entries in [false, true] {
            for instruction in [vec![0x8a, 0x23], absolute(4)] {
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
            for (pc, bytes, span, fetched) in [
                (0x1fff, vec![0x8a], 2, 2),
                (0x1ffe, vec![0x8a, 0x04], 3, 3),
                (0x1ffc, vec![0x8a, 0x05, 0x10, 0x50], 6, 5),
            ] {
                let (mut engine, keep) = prior_owners();
                upload(&mut engine, pc, &bytes);
                describe(&mut engine, &[(pc, span)], entries);
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
                        length: fetched,
                    }),
                );
                assert_eq!(
                    compile(&mut engine, owner, entries, 1),
                    Err(compile_error(owner, error))
                );
                assert_retained(&engine, keep, &before);
            }
            let (mut engine, keep) = prior_owners();
            let mut bytes = absolute(4);
            bytes.extend([0xeb, 0]);
            upload(&mut engine, CODE, &bytes);
            let specs = if entries {
                vec![(CODE, 0), (CODE + 5, 0)]
            } else {
                vec![(CODE, 5)]
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
            upload(&mut engine, CODE, &[0x8a, 0x23, 0x0f, 0x06]);
            describe(&mut engine, &[(CODE, 4)], entries);
            let before = saved(&engine, keep);
            let error = instruction_error(
                CODE + 2,
                InstructionError::Decode(DecodeError::Unsupported(UnsupportedFeature::Privileged)),
            );
            assert_eq!(
                compile(&mut engine, owner, entries, 1),
                Err(compile_error(owner, error))
            );
            assert_retained(&engine, keep, &before);
        }
    }
}

#[test]
fn adjacent_seeds_instruction_and_entry_caps_keep_exact_boundaries_and_old_owners() {
    let adjacent = [0x8a, 0x23, 0x8a, 0x43, 0xff, 0xeb, 0, 0x0f, 0x0b];
    for owner in [Owner::Replacement, Owner::Resident] {
        for entries in [false, true] {
            let mut engine = code(CODE, &adjacent, true);
            describe(&mut engine, &[(CODE, 2), (CODE + 2, 5)], entries);
            let id = compile(&mut engine, owner, entries, 2).unwrap();
            guard(&engine, owner, id).unwrap();
            if matches!(owner, Owner::Resident) {
                for pc in [CODE, CODE + 2, CODE + 5] {
                    assert_eq!(engine.lookup_resident(pc).unwrap().get(), id);
                }
                assert!(engine.lookup_resident(CODE + 3).is_err());
            }
            let mut bytes = vec![0x90; 62];
            bytes.extend([0x8a, 0x03, 0xeb, 0]);
            let mut engine = code(CODE, &bytes, true);
            describe(&mut engine, &[(CODE, bytes.len())], entries);
            compile(&mut engine, owner, entries, 1).unwrap();
            let (mut engine, keep) = prior_owners();
            let mut bytes = vec![0x90; 63];
            bytes.extend([0x8a, 0x03, 0x0f, 0x0b]);
            upload(&mut engine, CODE, &bytes);
            describe(&mut engine, &[(CODE, bytes.len())], entries);
            let before = saved(&engine, keep);
            assert_eq!(
                compile(&mut engine, owner, entries, 1),
                Err(compile_error(owner, CompileError::InstructionLimit))
            );
            assert_retained(&engine, keep, &before);
            let bytes = [0x8a, 0x03, 0xeb, 0].repeat(8);
            let mut engine = code(CODE, &bytes, true);
            let specs: Vec<_> = (0..8).map(|index| (CODE + index * 4, 4)).collect();
            describe(&mut engine, &specs, entries);
            let id = compile(&mut engine, owner, entries, 8).unwrap();
            assert_module(&engine, owner, id, false);
            guard(&engine, owner, id).unwrap();
            let (mut engine, keep) = prior_owners();
            let bytes = [0x8a, 0x03, 0xeb, 0].repeat(9);
            upload(&mut engine, CODE, &bytes);
            let specs: Vec<_> = (0..9).map(|index| (CODE + index * 4, 4)).collect();
            describe(&mut engine, &specs, entries);
            let before = saved(&engine, keep);
            assert_eq!(
                compile(&mut engine, owner, entries, 9),
                Err(HostError::InvalidRequest)
            );
            assert_retained(&engine, keep, &before);
        }
    }
}

#[test]
fn consumed_cross_page_snapshots_ignore_data_changes_and_reject_same_code_write() {
    let bytes = [0x8a, 0x25, 0x10, 0x50, 0, 0, 0xeb, 0];
    for owner in [Owner::Replacement, Owner::Resident] {
        for entries in [false, true] {
            let mut engine = code(0x1ffc, &bytes, false);
            let decoded = decode_one(engine.memory().unwrap(), GuestAddress(0x1ffc)).unwrap();
            assert_eq!(decoded.length(), 6);
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
            assert!(
                engine
                    .memory()
                    .unwrap()
                    .is_code_current(decoded.code_snapshot())
            );
            assert!(engine.memory().unwrap().is_code_current(&snapshot));
            assert_eq!(module(&engine, owner, id), old_bytes);
            guard(&engine, owner, id).unwrap();
            engine.write8(0x2001, 0).unwrap();
            assert!(
                !engine
                    .memory()
                    .unwrap()
                    .is_code_current(decoded.code_snapshot())
            );
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
