use ring3_engine::{
    cpu::{
        UnsupportedFeature,
        dbt::{
            ArtifactError, BlockSpec, CompileError, CompileLimits, InstructionError, RegistryError,
            RegistryLimits, ResidentRegistry, compile_entry_region, compile_region,
            prepare_entry_region, prepare_region,
        },
        x86::{
            Register32,
            decode::{DecodeError, decode_one},
            ir::{EffectiveAddress, Location32, Operation},
        },
    },
    memory::{Access, AddressSpace, FaultReason, GuestAddress, PageRange, Permissions},
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
const FORMS: [(&[u8], &[u8], Option<u32>); 3] = [
    (&[0x0f, 0xaf], &[], None),
    (&[0x69], &[0x78, 0x56, 0x34, 0x92], Some(0x9234_5678)),
    (&[0x6b], &[0x80], Some(0xffff_ff80)),
];

fn encoding(prefix: &[u8], tail: &[u8], immediate: &[u8]) -> Vec<u8> {
    [prefix, tail, immediate].concat()
}

fn code(pc: u32, bytes: &[u8]) -> AddressSpace {
    let base = pc & !0xfff;
    let pages = (u64::from(pc - base) + bytes.len() as u64).div_ceil(4096) as u32;
    let mut memory = AddressSpace::new(pages + 1).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(base), pages).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory.write(GuestAddress(pc), bytes).unwrap();
    memory
}

fn spec(pc: u32, length: usize) -> BlockSpec {
    BlockSpec {
        entry: GuestAddress(pc),
        byte_length: length as u32,
    }
}

fn decoded(
    memory: &mut AddressSpace,
    bytes: &[u8],
    destination: Register32,
    source: Location32,
    immediate: Option<u32>,
) {
    memory.write(GuestAddress(CODE), bytes).unwrap();
    let instruction = decode_one(memory, GuestAddress(CODE)).unwrap();
    assert_eq!(instruction.pc(), GuestAddress(CODE));
    assert_eq!(instruction.length() as usize, bytes.len(), "{bytes:02x?}");
    assert_eq!(
        instruction.next_pc(),
        GuestAddress(CODE + bytes.len() as u32)
    );
    assert_eq!(
        instruction.operation(),
        &Operation::SignedMultiply {
            destination,
            source,
            immediate,
        },
        "{bytes:02x?}"
    );
    assert!(memory.is_code_current(instruction.code_snapshot()));
}

fn memory_forms() -> Vec<Vec<u8>> {
    let tail = [5, 0x10, 0x50, 0, 0];
    let mut forms: Vec<_> = FORMS
        .into_iter()
        .map(|(prefix, immediate, _)| encoding(prefix, &tail, immediate))
        .collect();
    forms.push(encoding(&[0x69], &tail, &[0, 0, 0, 0]));
    forms.push(encoding(&[0x6b], &tail, &[0]));
    forms
}

fn register_block(destination: u8) -> Vec<u8> {
    let mut bytes = Vec::new();
    for source in 0..8 {
        let modrm = 0xc0 | destination << 3 | source;
        for (prefix, immediate, _) in FORMS {
            bytes.extend_from_slice(&encoding(prefix, &[modrm], immediate));
        }
    }
    let alias = 0xc0 | destination << 3 | destination;
    bytes.extend_from_slice(&[0x69, alias, 0, 0, 0, 0]);
    bytes.extend_from_slice(&[0x6b, alias, 0]);
    bytes.extend_from_slice(&[0xeb, 0]);
    bytes
}

fn instruction_error(pc: u32, cause: InstructionError) -> CompileError {
    CompileError::Instruction {
        pc: GuestAddress(pc),
        cause,
    }
}

#[test]
fn canonical_register_multiply_decodes_before_backend_admission() {
    let memory = code(CODE, &[0x0f, 0xaf, 0xc2]);
    let instruction = decode_one(&memory, GuestAddress(CODE)).unwrap();
    assert_eq!(instruction.length(), 3);
    assert_eq!(instruction.next_pc(), GuestAddress(CODE + 3));
}

#[test]
fn three_codes_preserve_all_gpr_pairs_aliases_and_normalized_immediate_bits() {
    let mut memory = code(CODE, &[0x90]);
    for (dst, destination) in REGISTERS.into_iter().enumerate() {
        for (src, source) in REGISTERS.into_iter().enumerate() {
            let modrm = 0xc0 | (dst as u8) << 3 | src as u8;
            decoded(
                &mut memory,
                &[0x0f, 0xaf, modrm],
                destination,
                Location32::Register(source),
                None,
            );
            for (raw, expected) in [(0, 0), (0x7f, 0x7f), (0x80, 0xffff_ff80), (0xff, u32::MAX)] {
                decoded(
                    &mut memory,
                    &[0x6b, modrm, raw],
                    destination,
                    Location32::Register(source),
                    Some(expected),
                );
            }
            for value in [0, 0x80, 0x8000_0000, u32::MAX] {
                decoded(
                    &mut memory,
                    &encoding(&[0x69], &[modrm], &value.to_le_bytes()),
                    destination,
                    Location32::Register(source),
                    Some(value),
                );
            }
        }
    }
}

#[test]
fn memory_sources_preserve_base_index_sib_scales_and_wrapping_displacements() {
    let mut memory = code(CODE, &[0x90]);
    for (prefix, literal, immediate) in FORMS {
        for (index, register) in REGISTERS.into_iter().enumerate() {
            let mut tail = vec![0x50 | index as u8];
            if register == Register32::Esp {
                tail.push(0x24);
            }
            tail.push(0xe0);
            decoded(
                &mut memory,
                &encoding(prefix, &tail, literal),
                Register32::Edx,
                Location32::Memory(EffectiveAddress {
                    base: Some(register),
                    index: None,
                    scale: 1,
                    displacement: 0xffff_ffe0,
                }),
                immediate,
            );
        }
        for (bits, scale) in [(0, 1), (0x40, 2), (0x80, 4), (0xc0, 8)] {
            decoded(
                &mut memory,
                &encoding(prefix, &[0x14, bits | 0x0b], literal),
                Register32::Edx,
                Location32::Memory(EffectiveAddress {
                    base: Some(Register32::Ebx),
                    index: Some(Register32::Ecx),
                    scale,
                    displacement: 0,
                }),
                immediate,
            );
        }
        for (tail, address) in [
            (
                vec![0x15, 0xff, 0xff, 0xff, 0xff],
                EffectiveAddress {
                    base: None,
                    index: None,
                    scale: 1,
                    displacement: u32::MAX,
                },
            ),
            (
                vec![0x14, 0x8d, 0xf8, 0xff, 0xff, 0xff],
                EffectiveAddress {
                    base: None,
                    index: Some(Register32::Ecx),
                    scale: 4,
                    displacement: 0xffff_fff8,
                },
            ),
        ] {
            decoded(
                &mut memory,
                &encoding(prefix, &tail, literal),
                Register32::Edx,
                Location32::Memory(address),
                immediate,
            );
        }
    }
}

#[test]
fn word_byte_implicit_wide_forms_and_prefixes_keep_existing_error_categories() {
    for bytes in [
        &[0x66, 0x0f, 0xaf, 0xc2][..],
        &[0x66, 0x69, 0xc2, 0x34, 0x92],
        &[0x66, 0x6b, 0xc2, 0x80],
        &[0x66, 0xf6, 0xe8],
        &[0x66, 0xf7, 0xe8],
        &[0x66, 0xf7, 0xe2],
        &[0x66, 0xf7, 0xf2],
        &[0x66, 0xf7, 0xfa],
        &[0x67, 0x0f, 0xaf, 0x03],
        &[0xf3, 0x0f, 0xaf, 0xc2],
        &[0xf2, 0x6b, 0xc2, 0x80],
    ] {
        let memory = code(CODE, bytes);
        assert_eq!(
            decode_one(&memory, GuestAddress(CODE)).err(),
            Some(DecodeError::Unsupported(UnsupportedFeature::Opcode)),
            "{bytes:02x?}"
        );
    }
    for (bytes, expected) in [
        (
            &[0x64, 0x0f, 0xaf, 0x03][..],
            DecodeError::Unsupported(UnsupportedFeature::Segment),
        ),
        (&[0xf0, 0x0f, 0xaf, 0xc2], DecodeError::InvalidEncoding),
    ] {
        let memory = code(CODE, bytes);
        assert_eq!(
            decode_one(&memory, GuestAddress(CODE)).err(),
            Some(expected)
        );
    }
}

#[test]
fn decoder_consumes_exact_extent_and_reports_missing_modrm_or_immediate() {
    for (prefix, literal, _) in FORMS {
        let bytes = encoding(prefix, &[0xc2], literal);
        let start = 0x2000 - bytes.len() as u32;
        let memory = code(start, &bytes);
        let instruction = decode_one(&memory, GuestAddress(start)).unwrap();
        assert_eq!(instruction.length() as usize, bytes.len());
        assert_eq!(instruction.next_pc(), GuestAddress(0x2000));
        assert!(
            memory
                .resolve(GuestAddress(0x2000), Access::Execute)
                .is_err()
        );
        for top in [false, true] {
            let missing = &bytes[..bytes.len() - 1];
            let pc = if top {
                (1_u64 << 32) - missing.len() as u64
            } else {
                0x2000 - missing.len() as u64
            } as u32;
            let memory = code(pc, missing);
            let error = decode_one(&memory, GuestAddress(pc)).err().unwrap();
            let DecodeError::MemoryFault {
                pc: fault_pc,
                fault,
                length,
            } = error
            else {
                panic!("{error:?}");
            };
            assert_eq!(fault_pc, GuestAddress(pc));
            assert_eq!(length as usize, bytes.len());
            assert_eq!(fault.access, Access::Execute);
            assert_eq!(fault.address, GuestAddress(if top { pc } else { 0x2000 }));
            assert_eq!(
                fault.reason,
                if top {
                    FaultReason::AddressOverflow
                } else {
                    FaultReason::Unmapped
                }
            );
        }
    }
}

#[test]
fn consumed_immediate_snapshot_spans_pages_and_ignores_unrelated_data() {
    for changed in [0x1ffd, 0x2000] {
        let mut memory = code(0x1ffd, &[0x69, 0xc2, 0x78, 0x56, 0x34, 0x92, 0xeb, 0]);
        let artifact =
            compile_entry_region(&memory, &[GuestAddress(0x1ffd)], CompileLimits::default())
                .unwrap();
        assert_eq!(artifact.metadata().instructions, 2);
        memory
            .map_zeroed(
                PageRange::new(GuestAddress(0x5000), 1).unwrap(),
                Permissions::READ_WRITE,
            )
            .unwrap();
        memory.write(GuestAddress(0x5000), &[0xff]).unwrap();
        artifact.wasm_bytes(&memory).unwrap();
        memory
            .write(
                GuestAddress(changed),
                &[if changed == 0x1ffd { 0x69 } else { 0x56 }],
            )
            .unwrap();
        assert_eq!(
            artifact.wasm_bytes(&memory),
            Err(ArtifactError::CodeInvalidated)
        );
    }
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

fn describe(engine: &mut EngineInstance, pc: u32, length: usize, entries: bool) {
    let transfer = &mut engine.arena_mut().unwrap()[140..];
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

fn contains(bytes: &[u8], name: &[u8]) -> bool {
    bytes.windows(name.len()).any(|window| window == name)
}

#[test]
fn all_register_pairs_and_forms_admit_in_explicit_cold_and_bound_profiles() {
    for destination in 0..8 {
        let mut bytes = register_block(destination);
        let length = bytes.len();
        bytes.extend_from_slice(&[0x0f, 0x0b]);
        let mut memory = code(CODE, &bytes);
        memory
            .protect(
                PageRange::new(GuestAddress(CODE), 1).unwrap(),
                Permissions::EXECUTE,
            )
            .unwrap();
        let specs = [spec(CODE, length)];
        let seeds = [GuestAddress(CODE)];
        let snapshot = memory.snapshot_code(GuestAddress(CODE), length).unwrap();
        for prepared in [
            prepare_region(&memory, &specs, CompileLimits::default()).unwrap(),
            prepare_entry_region(&memory, &seeds, CompileLimits::default()).unwrap(),
        ] {
            assert_eq!(
                (prepared.block_count(), prepared.instruction_count()),
                (1, 27)
            );
            assert!(prepared.is_current(&memory));
        }
        let explicit = compile_region(&memory, &specs, CompileLimits::default()).unwrap();
        let cold = compile_entry_region(&memory, &seeds, CompileLimits::default()).unwrap();
        assert_eq!(explicit.metadata(), cold.metadata());
        assert_eq!(
            explicit.wasm_bytes(&memory).unwrap(),
            cold.wasm_bytes(&memory).unwrap()
        );
        assert!(memory.is_code_current(&snapshot));
        assert!(memory.resolve(GuestAddress(CODE), Access::Read).is_err());
        for owner in [Owner::Replacement, Owner::Resident] {
            for entries in [false, true] {
                let mut engine = EngineInstance::new(1, KEY).unwrap();
                engine.map(CODE, 1, 7).unwrap();
                upload(&mut engine, CODE, &bytes);
                engine.protect(CODE, 1, 4).unwrap();
                let snapshot = engine
                    .memory()
                    .unwrap()
                    .snapshot_code(GuestAddress(CODE), length)
                    .unwrap();
                engine.arena_mut().unwrap().fill(0xa5);
                describe(&mut engine, CODE, length, entries);
                let before = engine.arena().to_vec();
                let id = compile(&mut engine, owner, entries).unwrap();
                assert_eq!(
                    engine.arena(),
                    before,
                    "{owner:?} entries={entries} destination={destination}"
                );
                assert!(engine.memory().unwrap().is_code_current(&snapshot));
                assert_eq!(engine.memory().unwrap().mapped_pages(), 1);
                assert!(
                    engine
                        .memory()
                        .unwrap()
                        .resolve(GuestAddress(0x5010), Access::Read)
                        .is_err()
                );
                let bytes = module(&engine, owner, id);
                assert!(contains(bytes, b"guard"));
                for name in [
                    b"read8".as_slice(),
                    b"read16",
                    b"read32",
                    b"store32",
                    b"store_resident32",
                ] {
                    assert!(!contains(bytes, name));
                }
                match owner {
                    Owner::Replacement => {
                        assert_eq!(engine.generation(), id as u32);
                        engine.guard(KEY, id as u32).unwrap();
                    }
                    Owner::Resident => {
                        assert_eq!(engine.generation(), 0);
                        assert_eq!(engine.lookup_resident(CODE).unwrap().get(), id);
                        engine.guard_resident(KEY, id).unwrap();
                    }
                }
            }
        }
    }
}

#[test]
fn every_memory_form_and_some_zero_refuse_all_standalone_backends_before_poison() {
    for instruction in memory_forms() {
        let mut bytes = instruction;
        bytes.extend_from_slice(&[0x0f, 0x0b]);
        let memory = code(CODE, &bytes);
        assert!(memory.resolve(GuestAddress(0x5010), Access::Read).is_err());
        let expected = instruction_error(CODE, InstructionError::BackendUnsupported);
        let specs = [spec(CODE, bytes.len())];
        let entries = [GuestAddress(CODE)];
        assert_eq!(
            prepare_region(&memory, &specs, CompileLimits::default()).err(),
            Some(expected)
        );
        assert_eq!(
            compile_region(&memory, &specs, CompileLimits::default()).err(),
            Some(expected)
        );
        assert_eq!(
            prepare_entry_region(&memory, &entries, CompileLimits::default()).err(),
            Some(expected)
        );
        assert_eq!(
            compile_entry_region(&memory, &entries, CompileLimits::default()).err(),
            Some(expected)
        );
        let mut registry = ResidentRegistry::new(&memory, RegistryLimits::default()).unwrap();
        let before = registry.usage();
        assert_eq!(
            registry.compile(&memory, &specs, CompileLimits::default()),
            Err(RegistryError::Compile(expected))
        );
        assert_eq!(registry.usage(), before);
        assert_eq!(
            registry.lookup(&memory, GuestAddress(CODE)),
            Err(RegistryError::NotFound {
                pc: GuestAddress(CODE)
            })
        );
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Saved {
    arena: Vec<u8>,
    arena_address: usize,
    generation: u32,
    replacement: Vec<u8>,
    replacement_address: usize,
    resident: Vec<u8>,
    resident_address: usize,
}

fn saved(engine: &EngineInstance, keep: u64) -> Saved {
    let replacement = engine.artifact_bytes().unwrap();
    let resident = engine.resident_bytes(keep).unwrap();
    Saved {
        arena: engine.arena().to_vec(),
        arena_address: engine.arena_address(),
        generation: engine.generation(),
        replacement: replacement.to_vec(),
        replacement_address: replacement.as_ptr() as usize,
        resident: resident.to_vec(),
        resident_address: resident.as_ptr() as usize,
    }
}

#[test]
fn admitted_prefix_and_memory_then_decode_refusal_preserves_prior_owners_and_publication() {
    for owner in [Owner::Replacement, Owner::Resident] {
        for entries in [false, true] {
            let mut engine = EngineInstance::new(2, KEY).unwrap();
            engine.map(CODE, 1, 7).unwrap();
            engine.map(KEEP, 1, 7).unwrap();
            upload(&mut engine, KEEP, &[0x90, 0xeb, 0]);
            describe(&mut engine, KEEP, 3, false);
            engine.compile(1).unwrap();
            describe(&mut engine, KEEP, 3, false);
            let keep = engine.compile_resident(1).unwrap().get();
            let snapshot = engine
                .memory()
                .unwrap()
                .snapshot_code(GuestAddress(KEEP), 3)
                .unwrap();
            for instruction in memory_forms() {
                let mut bytes = vec![0x0f, 0xaf, 0xc2];
                bytes.extend_from_slice(&instruction);
                bytes.extend_from_slice(&[0x0f, 0x0b]);
                upload(&mut engine, CODE, &bytes);
                describe(&mut engine, CODE, bytes.len(), entries);
                let before = saved(&engine, keep);
                let error = instruction_error(
                    CODE + 3 + instruction.len() as u32,
                    InstructionError::Decode(DecodeError::Unsupported(UnsupportedFeature::Opcode)),
                );
                let expected = match owner {
                    Owner::Replacement => HostError::Compile(error),
                    Owner::Resident => HostError::Resident(RegistryError::Compile(error)),
                };
                assert_eq!(
                    compile(&mut engine, owner, entries),
                    Err(expected),
                    "{owner:?} entries={entries}: {instruction:02x?}"
                );
                assert_eq!(saved(&engine, keep), before);
                assert!(engine.memory().unwrap().is_code_current(&snapshot));
                assert_eq!(engine.memory().unwrap().mapped_pages(), 2);
                assert!(
                    engine
                        .memory()
                        .unwrap()
                        .resolve(GuestAddress(0x5010), Access::Read)
                        .is_err()
                );
                engine.guard(KEY, before.generation).unwrap();
                engine.guard_resident(KEY, keep).unwrap();
                assert!(engine.lookup_resident(CODE).is_err());
            }
        }
    }
}

#[test]
fn multiplies_charge_instruction_and_wasm_caps_before_later_poison() {
    for (prefix, literal, _) in FORMS {
        let instruction = encoding(prefix, &[0xc2], literal);
        let mut bytes = instruction.repeat(63);
        bytes.extend_from_slice(&[0xeb, 0]);
        let memory = code(CODE, &bytes);
        assert_eq!(
            prepare_region(
                &memory,
                &[spec(CODE, bytes.len())],
                CompileLimits::default()
            )
            .unwrap()
            .instruction_count(),
            64
        );
        assert_eq!(
            prepare_entry_region(&memory, &[GuestAddress(CODE)], CompileLimits::default())
                .unwrap()
                .instruction_count(),
            64
        );
        let mut bytes = instruction.repeat(64);
        bytes.extend_from_slice(&[0x0f, 0x0b]);
        let memory = code(CODE, &bytes);
        assert_eq!(
            prepare_region(
                &memory,
                &[spec(CODE, bytes.len())],
                CompileLimits::default()
            )
            .err(),
            Some(CompileError::InstructionLimit)
        );
        assert_eq!(
            prepare_entry_region(&memory, &[GuestAddress(CODE)], CompileLimits::default()).err(),
            Some(CompileError::InstructionLimit)
        );
        let mut bytes = instruction;
        bytes.extend_from_slice(&[0xeb, 0]);
        let memory = code(CODE, &bytes);
        let limits = CompileLimits {
            wasm_bytes: 1,
            ..CompileLimits::default()
        };
        assert_eq!(
            compile_region(&memory, &[spec(CODE, bytes.len())], limits).err(),
            Some(CompileError::WasmLimit)
        );
        assert_eq!(
            compile_entry_region(&memory, &[GuestAddress(CODE)], limits).err(),
            Some(CompileError::WasmLimit)
        );
    }
}

#[test]
fn cold_seed_boundaries_and_terminal_jump_preserve_consumed_extents() {
    let memory = code(CODE, &[0x0f, 0xaf, 0xc2, 0x6b, 0xec, 0xff, 0xeb, 0]);
    let explicit = compile_region(
        &memory,
        &[spec(CODE, 3), spec(CODE + 3, 5)],
        CompileLimits::default(),
    )
    .unwrap();
    let cold = compile_entry_region(
        &memory,
        &[GuestAddress(CODE), GuestAddress(CODE + 3)],
        CompileLimits::default(),
    )
    .unwrap();
    assert_eq!(
        (cold.metadata().blocks, cold.metadata().instructions),
        (2, 3)
    );
    assert_eq!(
        explicit.wasm_bytes(&memory).unwrap(),
        cold.wasm_bytes(&memory).unwrap()
    );
    for (prefix, literal, _) in FORMS {
        let mut bytes = encoding(prefix, &[0xc2], literal);
        let length = bytes.len();
        bytes.extend_from_slice(&[0xeb, 0]);
        let memory = code(CODE, &bytes);
        assert_eq!(
            prepare_entry_region(
                &memory,
                &[GuestAddress(CODE), GuestAddress(CODE + length as u32 - 1)],
                CompileLimits::default()
            )
            .err(),
            Some(CompileError::InvalidBlocks)
        );
        assert_eq!(
            prepare_region(&memory, &[spec(CODE, length - 1)], CompileLimits::default()).err(),
            Some(instruction_error(CODE, InstructionError::InvalidBlockEnd))
        );
        let pc = 0x2000 - bytes.len() as u32;
        for owner in [Owner::Replacement, Owner::Resident] {
            let mut engine = EngineInstance::new(1, KEY).unwrap();
            engine.map(CODE, 1, 7).unwrap();
            upload(&mut engine, pc, &bytes);
            engine.protect(CODE, 1, 4).unwrap();
            describe(&mut engine, pc, bytes.len(), true);
            let before = engine.arena().to_vec();
            let id = compile(&mut engine, owner, true).unwrap();
            assert_eq!(engine.arena(), before);
            assert!(contains(module(&engine, owner, id), b"guard"));
            assert!(
                engine
                    .memory()
                    .unwrap()
                    .resolve(GuestAddress(0x2000), Access::Execute)
                    .is_err()
            );
            assert!(
                engine
                    .memory()
                    .unwrap()
                    .resolve(GuestAddress(0x5010), Access::Read)
                    .is_err()
            );
        }
    }
}
