use super::support_byte_prefix_admission as byte_prefix_admission;

use ring3_engine::process::EngineInstance;

#[test]
fn register_byte_immediate_admits_in_bound_engine() {
    let mut engine = EngineInstance::new(1, 0x1234_5678_9abc_def0).unwrap();
    let code = [0xb4, 0xff, 0xeb, 0];
    engine.map(0x1000, 1, 7).unwrap();
    engine.arena_mut().unwrap()[140..144].copy_from_slice(&code);
    engine.upload(0x1000, 4).unwrap();
    engine.protect(0x1000, 1, 4).unwrap();
    let transfer = &mut engine.arena_mut().unwrap()[140..148];
    transfer[..4].copy_from_slice(&0x1000_u32.to_le_bytes());
    transfer[4..8].copy_from_slice(&4_u32.to_le_bytes());
    engine
        .compile(1)
        .expect("register byte immediate must admit in the bound engine");
}

use ring3_engine::{
    cpu::{
        UnsupportedFeature,
        dbt::{
            BlockSpec, CompileError, CompileLimits, InstructionError, RegistryError,
            compile_entry_region, compile_region, prepare_entry_region, prepare_region,
        },
        x86::{
            decode::{DecodeError, decode_one},
            ir::{ByteRegister, ByteValue, Operation},
        },
    },
    memory::{Access, FaultReason, GuestAddress, MemoryFault},
    process::HostError,
};

const CODE: u32 = 0x1000;
const KEEP: u32 = 0x3000;
const DATA: u32 = 0x5000;
const KEY: u64 = 0x8abc_def0_f123_4567;
const BYTES: [ByteRegister; 8] = [
    ByteRegister::Al,
    ByteRegister::Cl,
    ByteRegister::Dl,
    ByteRegister::Bl,
    ByteRegister::Ah,
    ByteRegister::Ch,
    ByteRegister::Dh,
    ByteRegister::Bh,
];
const CHAIN: [u8; 8] = [0xb4, 0xff, 0x88, 0xe0, 0x8a, 0xe0, 0xeb, 0];

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
    for (index, &(pc, length)) in specs.iter().enumerate() {
        let offset = index * stride;
        transfer[offset..offset + 4].copy_from_slice(&pc.to_le_bytes());
        if !entries {
            transfer[offset + 4..offset + 8].copy_from_slice(&(length as u32).to_le_bytes());
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

fn assert_module(wasm: &[u8], owner: Option<Owner>, id: u64, mixed: bool) {
    let Some(owner) = owner else {
        assert!(!mixed);
        crate::support_wasm_interface::assert_interface(wasm, &[], &[4], false);
        return;
    };
    let resident = matches!(owner, Owner::Resident);
    let store = if resident { 6 } else { 2 };
    let mut helpers = Vec::new();
    let mut arities = vec![4, if resident { 7 } else { 6 }];
    if mixed {
        arities.extend([1, store]);
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
            ("cpu_read8", 1),
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
    crate::support_wasm_interface::assert_bound(wasm, resident, &helpers, &arities, mixed);
    crate::support_wasm_interface::assert_binding_prefix(wasm, KEY, id, resident);
}

#[test]
fn both_register_directions_and_all_immediate_endpoints_have_exact_byte_ir() {
    let mut engine = code(CODE, &[0x90, 0x90], false);
    let mut cases = 0;
    for (destination_index, destination) in BYTES.into_iter().enumerate() {
        for (source_index, source) in BYTES.into_iter().enumerate() {
            for (opcode, modrm) in [
                (
                    0x88,
                    0xc0 | (source_index as u8) << 3 | destination_index as u8,
                ),
                (
                    0x8a,
                    0xc0 | (destination_index as u8) << 3 | source_index as u8,
                ),
            ] {
                upload(&mut engine, CODE, &[opcode, modrm]);
                let instruction = decode_one(engine.memory().unwrap(), GuestAddress(CODE)).unwrap();
                assert_eq!(
                    instruction.operation(),
                    &Operation::MoveByte {
                        destination,
                        source: ByteValue::Register(source),
                    }
                );
                assert_eq!(instruction.length(), 2);
                assert_eq!(instruction.next_pc(), GuestAddress(CODE + 2));
                assert!(
                    engine
                        .memory()
                        .unwrap()
                        .is_code_current(instruction.code_snapshot())
                );
                cases += 1;
            }
        }
        for value in [0, 0x80, 0xff] {
            upload(&mut engine, CODE, &[0xb0 + destination_index as u8, value]);
            let instruction = decode_one(engine.memory().unwrap(), GuestAddress(CODE)).unwrap();
            assert_eq!(
                instruction.operation(),
                &Operation::MoveByte {
                    destination,
                    source: ByteValue::Immediate(value),
                }
            );
            assert_eq!(instruction.length(), 2);
            assert_eq!(instruction.next_pc(), GuestAddress(CODE + 2));
            cases += 1;
        }
    }
    assert_eq!(cases, 152);
}

#[test]
fn register_chain_admits_standalone_and_all_four_bound_profiles_without_data() {
    let engine = code(CODE, &CHAIN, true);
    let memory = engine.memory().unwrap();
    assert!(memory.resolve(GuestAddress(DATA), Access::Read).is_err());
    let specs = [BlockSpec {
        entry: GuestAddress(CODE),
        byte_length: CHAIN.len() as u32,
    }];
    let entries = [GuestAddress(CODE)];
    for prepared in [
        prepare_region(memory, &specs, CompileLimits::default()).unwrap(),
        prepare_entry_region(memory, &entries, CompileLimits::default()).unwrap(),
    ] {
        assert_eq!(
            (prepared.block_count(), prepared.instruction_count()),
            (1, 4)
        );
        assert!(prepared.is_current(memory));
    }
    for compiled in [
        compile_region(memory, &specs, CompileLimits::default()).unwrap(),
        compile_entry_region(memory, &entries, CompileLimits::default()).unwrap(),
    ] {
        assert_eq!(
            (compiled.metadata().blocks, compiled.metadata().instructions),
            (1, 4)
        );
        assert_module(compiled.wasm_bytes(memory).unwrap(), None, 0, false);
    }
    for owner in [Owner::Replacement, Owner::Resident] {
        for entries in [false, true] {
            let mut engine = code(CODE, &CHAIN, true);
            describe(&mut engine, &[(CODE, CHAIN.len())], entries);
            let before = engine.arena().to_vec();
            let id = compile(&mut engine, owner, entries, 1).unwrap();
            assert_eq!(engine.arena(), before);
            assert_eq!(engine.memory().unwrap().mapped_pages(), 1);
            assert!(
                engine
                    .memory()
                    .unwrap()
                    .resolve(GuestAddress(DATA), Access::Read)
                    .is_err()
            );
            assert_module(module(&engine, owner, id), Some(owner), id, false);
            guard(&engine, owner, id).unwrap();
            if matches!(owner, Owner::Resident) {
                assert_eq!(engine.generation(), 0);
                assert_eq!(engine.artifact_bytes(), Err(HostError::InvalidArtifact));
                for pc in [CODE, CODE + 2, CODE + 4, CODE + 6] {
                    assert_eq!(engine.lookup_resident(pc).unwrap().get(), id);
                }
                assert!(engine.lookup_resident(CODE + 1).is_err());
            }
        }
    }
}

#[test]
fn excluded_widths_alternatives_and_prefixes_keep_categories_and_old_publication() {
    let opcode = DecodeError::Unsupported(UnsupportedFeature::Opcode);
    for (bytes, expected) in [
        (&[0x66, 0xc6, 0xc0, 0][..], opcode),
        (&[0x66, 0xa0, 0x10, 0x50, 0, 0], opcode),
        (&[0x66, 0xa2, 0x10, 0x50, 0, 0], opcode),
        (&[0x66, 0x10, 0xd0], opcode),
        (&[0x66, 0x18, 0xd0], opcode),
        (&[0x66, 0x66, 0x89, 0xc3], opcode),
        (&[0x66, 0x88, 0xc3], opcode),
        (&[0x67, 0x8a, 0xc3], opcode),
        (&[0x66, 0xb4, 0xff], opcode),
        (&[0xf2, 0x88, 0xc3], opcode),
        (&[0xf3, 0x8a, 0xc3], opcode),
        (&[0xf0, 0x88, 0xc3], DecodeError::InvalidEncoding),
        (&[0xc6, 0xc8, 0], DecodeError::InvalidEncoding),
        (
            &[0x64, 0x8a, 0xc3],
            DecodeError::Unsupported(UnsupportedFeature::Segment),
        ),
    ] {
        let (fixture, expected) = byte_prefix_admission::closed_fixture(bytes, expected);
        let bytes = fixture.as_slice();
        let engine = code(CODE, bytes, true);
        crate::support_fetch::assert_decode_contract(
            (decode_one(engine.memory().unwrap(), GuestAddress(CODE)))
                .expect_err("instruction must fail to decode"),
            expected,
        );
    }
    assert_eq!(
        super::support_prefix_admission::admitted(&[0x66, 0x10, 0xd0]).1,
        3
    );
    for owner in [Owner::Replacement, Owner::Resident] {
        for entries in [false, true] {
            let (mut engine, keep) = prior_owners();
            upload(&mut engine, CODE, &[0x90, 0xf0, 0x90, 0x90]);
            describe(&mut engine, &[(CODE, 4)], entries);
            let before = saved(&engine, keep);
            crate::support_fetch::assert_host_contract(
                (compile(&mut engine, owner, entries, 1))
                    .expect_err("compilation must refuse this fixture"),
                compile_error(
                    owner,
                    instruction_error(
                        CODE + 1,
                        InstructionError::Decode(DecodeError::InvalidEncoding),
                    ),
                ),
            );
            assert_retained(&engine, keep, &before);
        }
    }
}

#[test]
fn byte_registers_add_no_helpers_to_pure_or_existing_mixed_modules() {
    let bytes = [
        0xb7, 0x80, 0x88, 0xe0, 0x8a, 0xe0, 0x8b, 0x03, 0x89, 0x03, 0x8a, 0x23, 0x0f, 0xb6, 0x03,
        0x0f, 0xbf, 0x03, 0x88, 0x23, 0xeb, 0,
    ];
    for owner in [Owner::Replacement, Owner::Resident] {
        for entries in [false, true] {
            let mut engine = code(CODE, &bytes, true);
            describe(&mut engine, &[(CODE, bytes.len())], entries);
            let id = compile(&mut engine, owner, entries, 1).unwrap();
            assert_module(module(&engine, owner, id), Some(owner), id, true);
            guard(&engine, owner, id).unwrap();
            assert!(
                engine
                    .memory()
                    .unwrap()
                    .resolve(GuestAddress(DATA), Access::Read)
                    .is_err()
            );
        }
    }
}

#[test]
fn consumed_fetch_spans_truncation_and_cut_errors_preserve_prior_owners() {
    for instruction in [[0x88, 0xe0], [0x8a, 0xe0], [0xb4, 0xff]] {
        let engine = code(0x1ffe, &instruction, true);
        let decoded = decode_one(engine.memory().unwrap(), GuestAddress(0x1ffe)).unwrap();
        assert_eq!(decoded.length(), 2);
        assert_eq!(decoded.next_pc(), GuestAddress(0x2000));
        assert!(
            engine
                .memory()
                .unwrap()
                .resolve(GuestAddress(0x2000), Access::Execute)
                .is_err()
        );
        let engine = code(u32::MAX - 1, &instruction, true);
        let decoded = decode_one(engine.memory().unwrap(), GuestAddress(u32::MAX - 1)).unwrap();
        assert_eq!(decoded.next_pc(), GuestAddress(0));
        let specs = [BlockSpec {
            entry: GuestAddress(u32::MAX - 1),
            byte_length: 2,
        }];
        compile_region(engine.memory().unwrap(), &specs, CompileLimits::default()).unwrap();
        compile_entry_region(
            engine.memory().unwrap(),
            &[GuestAddress(u32::MAX - 1)],
            CompileLimits::default(),
        )
        .unwrap();
    }
    for owner in [Owner::Replacement, Owner::Resident] {
        for entries in [false, true] {
            for opcode in [0x88, 0x8a, 0xb4] {
                let (mut engine, keep) = prior_owners();
                upload(&mut engine, 0x1fff, &[opcode]);
                describe(&mut engine, &[(0x1fff, 2)], entries);
                let before = saved(&engine, keep);
                let error = instruction_error(
                    0x1fff,
                    InstructionError::Decode(DecodeError::MemoryFault {
                        pc: GuestAddress(0x1fff),
                        fault: MemoryFault {
                            address: GuestAddress(0x2000),
                            access: Access::Execute,
                            reason: FaultReason::Unmapped,
                        },
                        length: 2,
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
            upload(&mut engine, CODE, &CHAIN);
            let specs = if entries {
                vec![(CODE, 0), (CODE + 1, 0)]
            } else {
                vec![(CODE, 1)]
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
            upload(&mut engine, CODE, &[0xb4, 0xff, 0x0f, 0x06]);
            describe(&mut engine, &[(CODE, 4)], entries);
            let before = saved(&engine, keep);
            crate::support_fetch::assert_host_contract(
                (compile(&mut engine, owner, entries, 1))
                    .expect_err("compilation must refuse this fixture"),
                compile_error(
                    owner,
                    instruction_error(
                        CODE + 2,
                        InstructionError::Decode(DecodeError::Unsupported(
                            UnsupportedFeature::Privileged,
                        )),
                    ),
                ),
            );
            assert_retained(&engine, keep, &before);
        }
    }
    for opcode in [0x88, 0x8a, 0xb4] {
        let engine = code(u32::MAX, &[opcode], true);
        crate::support_fetch::assert_error(
            (decode_one(engine.memory().unwrap(), GuestAddress(u32::MAX)))
                .expect_err("code fetch must fail"),
            u32::MAX,
            u32::MAX,
            FaultReason::AddressOverflow,
            2,
        );
    }
}

#[test]
fn cold_seeds_poison_instruction_and_module_caps_keep_exact_publication() {
    let adjacent = [0xb4, 0xff, 0x88, 0xe0, 0xeb, 0, 0xf0, 0x90];
    for owner in [Owner::Replacement, Owner::Resident] {
        for entries in [false, true] {
            let mut engine = code(CODE, &adjacent, true);
            describe(&mut engine, &[(CODE, 2), (CODE + 2, 4)], entries);
            let id = compile(&mut engine, owner, entries, 2).unwrap();
            guard(&engine, owner, id).unwrap();
            if matches!(owner, Owner::Resident) {
                for pc in [CODE, CODE + 2, CODE + 4] {
                    assert_eq!(engine.lookup_resident(pc).unwrap().get(), id);
                }
                assert!(engine.lookup_resident(CODE + 1).is_err());
                assert!(engine.lookup_resident(CODE + 6).is_err());
            }
            let mut bytes = [0xb4, 0xff].repeat(63);
            bytes.extend([0xeb, 0]);
            let mut engine = code(CODE, &bytes, true);
            describe(&mut engine, &[(CODE, bytes.len())], entries);
            compile(&mut engine, owner, entries, 1).unwrap();
            let (mut engine, keep) = prior_owners();
            let mut bytes = [0xb4, 0xff].repeat(64);
            bytes.extend([0xf0, 0x90]);
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
            let bytes = [0xb4, 0xff, 0xeb, 0].repeat(8);
            let mut engine = code(CODE, &bytes, true);
            let specs: Vec<_> = (0..8).map(|index| (CODE + index * 4, 4)).collect();
            describe(&mut engine, &specs, entries);
            let id = compile(&mut engine, owner, entries, 8).unwrap();
            assert_module(module(&engine, owner, id), Some(owner), id, false);
            let (mut engine, keep) = prior_owners();
            upload(&mut engine, CODE, &[0xb4, 0xff, 0xeb, 0].repeat(9));
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
    let engine = code(CODE, &CHAIN, true);
    let specs = [BlockSpec {
        entry: GuestAddress(CODE),
        byte_length: CHAIN.len() as u32,
    }];
    let limits = CompileLimits {
        instructions: 3,
        ..CompileLimits::default()
    };
    assert_eq!(
        compile_region(engine.memory().unwrap(), &specs, limits).err(),
        Some(CompileError::InstructionLimit)
    );
    let prefix =
        compile_entry_region(engine.memory().unwrap(), &[GuestAddress(CODE)], limits).unwrap();
    assert_eq!(prefix.metadata().instructions, limits.instructions);
    assert!(prefix.wasm_bytes(engine.memory().unwrap()).unwrap().len() <= limits.wasm_bytes);
    let limits = CompileLimits {
        wasm_bytes: 8,
        ..CompileLimits::default()
    };
    assert_eq!(
        compile_region(engine.memory().unwrap(), &specs, limits).err(),
        Some(CompileError::WasmLimit)
    );
    assert_eq!(
        compile_entry_region(engine.memory().unwrap(), &[GuestAddress(CODE)], limits).err(),
        Some(CompileError::WasmLimit)
    );
}

#[test]
fn consumed_cross_page_snapshots_stale_on_code_only_and_preserve_module_owners() {
    let bytes = [0xb4, 0xff, 0x88, 0xe0, 0xeb, 0];
    for owner in [Owner::Replacement, Owner::Resident] {
        for entries in [false, true] {
            let mut engine = code(0x1fff, &bytes, false);
            let decoded = decode_one(engine.memory().unwrap(), GuestAddress(0x1fff)).unwrap();
            let snapshot = engine
                .memory()
                .unwrap()
                .snapshot_code(GuestAddress(0x1fff), bytes.len())
                .unwrap();
            describe(&mut engine, &[(0x1fff, bytes.len())], entries);
            let id = compile(&mut engine, owner, entries, 1).unwrap();
            let old = module(&engine, owner, id).to_vec();
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
            assert_eq!(module(&engine, owner, id), old);
            guard(&engine, owner, id).unwrap();
            engine.write8(0x2000, 0xff).unwrap();
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
