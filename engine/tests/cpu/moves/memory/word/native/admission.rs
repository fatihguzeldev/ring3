use std::{
    fmt::Write as _,
    fs::{self, OpenOptions},
    io::Write as _,
    path::Path,
};

use ring3_engine::{
    abi::arena::{TRANSFER_OFFSET, TRANSFER_SIZE},
    cpu::{
        UnsupportedFeature,
        dbt::{
            BlockSpec, CompileError, CompileLimits, InstructionError, RegistryError,
            compile_entry_region, compile_region, prepare_entry_region, prepare_region,
        },
        x86::decode::DecodeError,
    },
    memory::{Access, FaultReason, GuestAddress, MemoryFault},
    process::{EngineInstance, HostError},
};

const CODE: u32 = 0x1000;
const KEY: u64 = 0x1234_5678_9abc_def0;
const KEEP: u32 = 0x3000;

#[derive(Clone, Copy, Debug)]
enum Owner {
    Replacement,
    Resident,
}

fn write_new(path: &Path, bytes: &[u8]) {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .unwrap()
        .write_all(bytes)
        .unwrap();
}

#[test]
#[ignore = "requires a fresh RING3_WORD_MEMORY_CAPTURE_DIR for before/after module evidence"]
fn capture_compatible_modules_through_existing_public_apis() {
    let output = std::env::var_os("RING3_WORD_MEMORY_CAPTURE_DIR")
        .expect("set RING3_WORD_MEMORY_CAPTURE_DIR to a fresh evidence directory");
    let output = Path::new(&output);
    fs::create_dir_all(output).unwrap();
    let banks: [(&str, &[u8]); 8] = [
        ("byte-load", &[0x8a, 0x03, 0xeb, 0]),
        ("byte-store", &[0x88, 0x03, 0xeb, 0]),
        ("dword-load", &[0x8b, 0x03, 0xeb, 0]),
        ("dword-store", &[0x89, 0x03, 0xeb, 0]),
        ("word-load-string", &[0x66, 0xad, 0xeb, 0]),
        ("word-store-string", &[0x66, 0xab, 0xeb, 0]),
        ("word-move-string", &[0x66, 0xa5, 0xeb, 0]),
        (
            "word-register-move",
            &[
                0x66, 0xb8, 0x34, 0x12, 0x66, 0x89, 0xc1, 0x66, 0x8b, 0xd1, 0xeb, 0,
            ],
        ),
    ];
    let mut manifest = String::from("bank\towner\tprofile\tpc\tkey\tx86\tmodule\tbytes\n");
    let mut captured = 0;
    for (bank, bytes) in banks {
        for owner in [Owner::Replacement, Owner::Resident] {
            for entries in [false, true] {
                let mut engine = EngineInstance::new(1, KEY).unwrap();
                engine.map(CODE, 1, 7).unwrap();
                engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
                    .copy_from_slice(bytes);
                engine.upload(CODE, bytes.len() as u32).unwrap();
                engine.protect(CODE, 1, 4).unwrap();
                for address in [0, 0x5000] {
                    assert!(
                        engine
                            .memory()
                            .unwrap()
                            .resolve(GuestAddress(address), Access::Read)
                            .is_err()
                    );
                }
                let transfer = &mut engine.arena_mut().unwrap()
                    [TRANSFER_OFFSET..TRANSFER_OFFSET + TRANSFER_SIZE];
                transfer.fill(0xa5);
                transfer[..4].copy_from_slice(&CODE.to_le_bytes());
                if !entries {
                    transfer[4..8].copy_from_slice(&(bytes.len() as u32).to_le_bytes());
                }
                let module = match (owner, entries) {
                    (Owner::Replacement, false) => {
                        engine.compile(1).unwrap();
                        engine.artifact_bytes().unwrap()
                    }
                    (Owner::Replacement, true) => {
                        engine.compile_entries(1, 0).unwrap();
                        engine.artifact_bytes().unwrap()
                    }
                    (Owner::Resident, false) => {
                        let id = engine.compile_resident(1).unwrap();
                        engine.resident_bytes(id.get()).unwrap()
                    }
                    (Owner::Resident, true) => {
                        let id = engine.compile_resident_entries(1, 0).unwrap();
                        engine.resident_bytes(id.get()).unwrap()
                    }
                };
                let owner = match owner {
                    Owner::Replacement => "replacement",
                    Owner::Resident => "resident",
                };
                let profile = if entries { "entries" } else { "explicit" };
                let filename = format!("{bank}-{owner}-{profile}.wasm");
                write_new(&output.join(&filename), module);
                let mut hex = String::new();
                for byte in bytes {
                    write!(hex, "{byte:02x}").unwrap();
                }
                writeln!(
                    manifest,
                    "{bank}\t{owner}\t{profile}\t{CODE:08x}\t{KEY:016x}\t{hex}\t{filename}\t{}",
                    module.len()
                )
                .unwrap();
                captured += 1;
            }
        }
    }
    assert_eq!(captured, 32);
    write_new(&output.join("manifest.tsv"), manifest.as_bytes());
}

fn upload(engine: &mut EngineInstance, pc: u32, bytes: &[u8]) {
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(bytes);
    engine.upload(pc, bytes.len() as u32).unwrap();
}

fn describe(engine: &mut EngineInstance, pc: u32, length: usize, entries: bool) {
    let transfer =
        &mut engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + TRANSFER_SIZE];
    transfer.fill(0xa5);
    transfer[..4].copy_from_slice(&pc.to_le_bytes());
    if !entries {
        transfer[4..8].copy_from_slice(&(length as u32).to_le_bytes());
    }
}

fn compile(engine: &mut EngineInstance, owner: Owner, entries: bool) -> Result<u64, HostError> {
    match (owner, entries) {
        (Owner::Replacement, false) => engine.compile(1).map(u64::from),
        (Owner::Replacement, true) => engine.compile_entries(1, 0).map(u64::from),
        (Owner::Resident, false) => engine.compile_resident(1).map(|id| id.get()),
        (Owner::Resident, true) => engine.compile_resident_entries(1, 0).map(|id| id.get()),
    }
}

fn module(engine: &EngineInstance, owner: Owner, id: u64) -> &[u8] {
    match owner {
        Owner::Replacement => engine.artifact_bytes().unwrap(),
        Owner::Resident => engine.resident_bytes(id).unwrap(),
    }
}

fn guard(engine: &EngineInstance, owner: Owner, id: u64) {
    match owner {
        Owner::Replacement => engine.guard(KEY, id as u32),
        Owner::Resident => engine.guard_resident(KEY, id),
    }
    .unwrap();
}

fn code(bytes: &[u8]) -> EngineInstance {
    let mut engine = EngineInstance::new(1, KEY).unwrap();
    engine.map(CODE, 1, 7).unwrap();
    upload(&mut engine, CODE, bytes);
    engine.protect(CODE, 1, 4).unwrap();
    engine
}

fn assert_imports(wasm: &[u8], owner: Owner, helpers: &[&str]) {
    let resident = matches!(owner, Owner::Resident);
    let store = if resident { 6 } else { 2 };
    let mut arities = vec![4, if resident { 7 } else { 6 }];
    if helpers.iter().any(|name| name.starts_with("cpu_read")) {
        arities.push(1);
    }
    if helpers.iter().any(|name| name.starts_with("cpu_store")) {
        arities.push(store);
    }
    let owned: Vec<_> = helpers
        .iter()
        .map(|name| {
            if resident {
                name.replacen("cpu_store", "cpu_store_resident", 1)
            } else {
                (*name).to_owned()
            }
        })
        .collect();
    let imports: Vec<_> = owned
        .iter()
        .map(|name| {
            (
                name.as_str(),
                if name.starts_with("cpu_read") {
                    1
                } else {
                    store
                },
            )
        })
        .collect();
    crate::support_wasm_interface::assert_bound(wasm, resident, &imports, &arities, true);
}

#[test]
fn four_bound_profiles_select_exact_narrow_and_mixed_imports_without_data_access() {
    let mut cases: Vec<(Vec<u8>, &[&str])> = Vec::new();
    for register in 0..8 {
        cases.push((vec![0x66, 0x8b, 0x03 | register << 3], &["cpu_read16"]));
        cases.push((vec![0x66, 0x89, 0x03 | register << 3], &["cpu_store16"]));
    }
    cases.push((vec![0x66, 0xc7, 0x03, 0x34, 0x12], &["cpu_store16"]));
    cases.push((
        vec![0x66, 0x89, 0x03, 0x66, 0xc7, 0x03, 0x34, 0x12],
        &["cpu_store16"],
    ));
    cases.push((
        vec![
            0x66, 0x8b, 0x03, 0x66, 0x89, 0x03, 0x66, 0xc7, 0x03, 0x34, 0x12,
        ],
        &["cpu_read16", "cpu_store16"],
    ));
    cases.push((
        vec![
            0x8b, 0x03, 0x89, 0x03, 0x8a, 0x03, 0x88, 0x03, 0x66, 0x8b, 0x03, 0x66, 0x89, 0x03,
        ],
        &[
            "cpu_read32",
            "cpu_store32",
            "cpu_read8",
            "cpu_read16",
            "cpu_store8",
            "cpu_store16",
        ],
    ));
    for (mut bytes, helpers) in cases {
        bytes.extend([0xeb, 0]);
        for owner in [Owner::Replacement, Owner::Resident] {
            for entries in [false, true] {
                let mut engine = code(&bytes);
                for address in [0, 0x5000] {
                    assert!(
                        engine
                            .memory()
                            .unwrap()
                            .resolve(GuestAddress(address), Access::Read)
                            .is_err()
                    );
                }
                describe(&mut engine, CODE, bytes.len(), entries);
                let before = engine.arena().to_vec();
                let id = compile(&mut engine, owner, entries).unwrap();
                assert_eq!(
                    engine.arena(),
                    before,
                    "native compilation does not publish ABI metadata"
                );
                guard(&engine, owner, id);
                assert_imports(module(&engine, owner, id), owner, helpers);
                assert_eq!(engine.memory().unwrap().mapped_pages(), 1);
            }
        }
    }
}

fn forms() -> [&'static [u8]; 3] {
    [
        &[0x66, 0x8b, 0x03],
        &[0x66, 0x89, 0x03],
        &[0x66, 0xc7, 0x03, 0x34, 0x12],
    ]
}

fn instruction_error(pc: u32, cause: InstructionError) -> CompileError {
    CompileError::Instruction {
        pc: GuestAddress(pc),
        cause,
    }
}

#[test]
fn standalone_explicit_and_cold_profiles_refuse_word_memory_backend() {
    for instruction in forms() {
        let mut bytes = instruction.to_vec();
        bytes.extend([0xeb, 0]);
        let engine = code(&bytes);
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
    describe(&mut engine, KEEP, 3, false);
    engine.compile(1).unwrap();
    describe(&mut engine, KEEP, 3, false);
    let id = engine.compile_resident(1).unwrap().get();
    (engine, id)
}

fn compile_error(owner: Owner, error: CompileError) -> HostError {
    match owner {
        Owner::Replacement => HostError::Compile(error),
        Owner::Resident => HostError::Resident(RegistryError::Compile(error)),
    }
}

#[test]
fn late_decode_fetch_and_instruction_cap_failures_preserve_arena_and_prior_owners() {
    for instruction in forms() {
        for owner in [Owner::Replacement, Owner::Resident] {
            for entries in [false, true] {
                let mut valid = instruction.to_vec();
                valid.extend([0x90; 62]);
                valid.extend([0xeb, 0]);
                let mut engine = code(&valid);
                describe(&mut engine, CODE, valid.len(), entries);
                let id = compile(&mut engine, owner, entries).unwrap();
                guard(&engine, owner, id);
                for failure in 0..3 {
                    let (mut engine, keep) = prior_owners();
                    let mut bytes = instruction.to_vec();
                    let (pc, span, error) = match failure {
                        0 => {
                            bytes.extend([0x0f, 0x06]);
                            (
                                CODE,
                                bytes.len(),
                                instruction_error(
                                    CODE + instruction.len() as u32,
                                    InstructionError::Decode(DecodeError::Unsupported(
                                        UnsupportedFeature::Privileged,
                                    )),
                                ),
                            )
                        }
                        1 => {
                            bytes.push(0x66);
                            let pc = 0x2000 - bytes.len() as u32;
                            (
                                pc,
                                bytes.len() + 1,
                                instruction_error(
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
                                ),
                            )
                        }
                        _ => {
                            bytes.extend([0x90; 63]);
                            bytes.extend([0xf0, 0x90]);
                            (CODE, bytes.len(), CompileError::InstructionLimit)
                        }
                    };
                    upload(&mut engine, pc, &bytes);
                    describe(&mut engine, pc, span, entries);
                    if entries && error == CompileError::InstructionLimit {
                        assert!(crate::support_admission::admit_if_deferred(
                            &mut engine,
                            crate::support_admission::ColdEntryCase {
                                key: KEY,
                                keeper: keep,
                                resident: matches!(owner, Owner::Resident),
                                pages: &[CODE, KEEP],
                                expected: error
                            }
                        ));
                        continue;
                    }
                    let before = saved(&engine, keep);
                    crate::support_fetch::assert_host_contract(
                        (compile(&mut engine, owner, entries))
                            .expect_err("compilation must refuse this fixture"),
                        compile_error(owner, error),
                    );
                    assert_eq!(saved(&engine, keep), before);
                    engine.guard(KEY, before.generation).unwrap();
                    engine.guard_resident(KEY, keep).unwrap();
                    assert_eq!(engine.lookup_resident(KEEP).unwrap().get(), keep);
                    assert!(engine.lookup_resident(pc).is_err());
                }
            }
        }
    }
}
