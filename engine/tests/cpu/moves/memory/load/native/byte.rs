use super::support_byte_prefix_admission as byte_prefix_admission;

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
    let transfer =
        &mut engine.arena_mut().unwrap()[140..140 + ring3_engine::abi::arena::TRANSFER_SIZE];
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
    let store = if resident { 6 } else { 2 };
    let mut helpers = Vec::new();
    let mut arities = vec![4, if resident { 7 } else { 6 }, 1];
    if mixed {
        arities.push(store);
        helpers.extend([
            ("cpu_read32", 1),
            (
                if resident {
                    "cpu_store_resident32"
                } else {
                    "cpu_store32"
                },
                store,
            ),
        ]);
    }
    helpers.push(("cpu_read8", 1));
    if mixed {
        helpers.extend([
            ("cpu_read16", 1),
            (
                if resident {
                    "cpu_store_resident8"
                } else {
                    "cpu_store8"
                },
                store,
            ),
        ]);
    }
    crate::support_wasm_interface::assert_bound(wasm, resident, &helpers, &arities, true);
    crate::support_wasm_interface::assert_binding_prefix(wasm, KEY, id, resident);
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
                            displacement: DATA + 0x10,
                            ..Default::default()
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
                ..Default::default()
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
                ..Default::default()
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
                ..Default::default()
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
                ..Default::default()
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
                ..Default::default()
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
                ..Default::default()
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
                ..Default::default()
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
        (&[0x66, 0x10, 0xd0], opcode),
        (&[0x66, 0x18, 0xd0], opcode),
        (&[0x66, 0xc6, 0xc0, 0], opcode),
        (&[0x66, 0x10, 0xd0], opcode),
        (&[0x66, 0xa0, 0x10, 0x50, 0, 0], opcode),
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
        let (fixture, expected) = byte_prefix_admission::closed_fixture(bytes, expected);
        let bytes = fixture.as_slice();

        let engine = code(CODE, bytes, false);
        crate::support_fetch::assert_decode_contract(
            (decode_one(engine.memory().unwrap(), GuestAddress(CODE)))
                .expect_err("instruction must fail to decode"),
            expected,
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
                crate::support_fetch::assert_host_contract(
                    (compile(&mut engine, owner, entries, 1))
                        .expect_err("compilation must refuse this fixture"),
                    compile_error(owner, error),
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
            crate::support_fetch::assert_host_contract(
                (compile(&mut engine, owner, entries, specs.len() as u32))
                    .expect_err("compilation must refuse this fixture"),
                compile_error(owner, error),
            );
            assert_retained(&engine, keep, &before);
            upload(&mut engine, CODE, &[0x8a, 0x23, 0x0f, 0x06]);
            describe(&mut engine, &[(CODE, 4)], entries);
            let before = saved(&engine, keep);
            let error = instruction_error(
                CODE + 2,
                InstructionError::Decode(DecodeError::Unsupported(UnsupportedFeature::Privileged)),
            );
            crate::support_fetch::assert_host_contract(
                (compile(&mut engine, owner, entries, 1))
                    .expect_err("compilation must refuse this fixture"),
                compile_error(owner, error),
            );
            assert_retained(&engine, keep, &before);
        }
    }
}

#[test]
fn adjacent_seeds_instruction_and_entry_caps_keep_exact_boundaries_and_old_owners() {
    let adjacent = [0x8a, 0x23, 0x8a, 0x43, 0xff, 0xeb, 0, 0xf0, 0x90];
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
            bytes.extend([0x8a, 0x03, 0xf0, 0x90]);
            upload(&mut engine, CODE, &bytes);
            describe(&mut engine, &[(CODE, bytes.len())], entries);
            let before = saved(&engine, keep);
            if entries {
                assert!(crate::support_admission::admit_if_deferred(
                    &mut engine,
                    crate::support_admission::ColdEntryCase {
                        key: KEY,
                        keeper: keep,
                        resident: matches!(owner, Owner::Resident),
                        pages: &[CODE, KEEP],
                        expected: CompileError::InstructionLimit,
                    },
                ));
            } else {
                crate::support_fetch::assert_host_contract(
                    compile(&mut engine, owner, entries, 1)
                        .expect_err("explicit oversized compilation must refuse"),
                    compile_error(owner, CompileError::InstructionLimit),
                );
                assert_retained(&engine, keep, &before);
            }
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
            crate::support_fetch::assert_host_contract(
                (compile(&mut engine, owner, entries, 9))
                    .expect_err("compilation must refuse this fixture"),
                HostError::InvalidRequest,
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
            crate::support_fetch::assert_host_contract(
                (guard(&engine, owner, id)).expect_err("compilation must refuse this fixture"),
                if matches!(owner, Owner::Resident) {
                    HostError::Resident(RegistryError::CodeInvalidated)
                } else {
                    HostError::CodeInvalidated
                },
            );
        }
    }
}
