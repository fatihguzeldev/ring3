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
            ir::{EffectiveAddress, Location32, Operation, ShiftCount, ShiftKind},
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
const KINDS: [(ShiftKind, u8); 3] = [
    (ShiftKind::Shl, 4),
    (ShiftKind::Shr, 5),
    (ShiftKind::Sar, 7),
];
const FORMS: [(ShiftKind, &[u8], ShiftCount); 9] = [
    (ShiftKind::Shl, &[0xd1, 0xe0], ShiftCount::Immediate(1)),
    (
        ShiftKind::Shl,
        &[0xc1, 0xe0, 0xff],
        ShiftCount::Immediate(255),
    ),
    (ShiftKind::Shl, &[0xd3, 0xe0], ShiftCount::Cl),
    (ShiftKind::Shr, &[0xd1, 0xe8], ShiftCount::Immediate(1)),
    (
        ShiftKind::Shr,
        &[0xc1, 0xe8, 0xff],
        ShiftCount::Immediate(255),
    ),
    (ShiftKind::Shr, &[0xd3, 0xe8], ShiftCount::Cl),
    (ShiftKind::Sar, &[0xd1, 0xf8], ShiftCount::Immediate(1)),
    (
        ShiftKind::Sar,
        &[0xc1, 0xf8, 0xff],
        ShiftCount::Immediate(255),
    ),
    (ShiftKind::Sar, &[0xd3, 0xf8], ShiftCount::Cl),
];
const ADDRESS: EffectiveAddress = EffectiveAddress {
    base: Some(Register32::Ebx),
    index: Some(Register32::Ecx),
    scale: 4,
    displacement: 0xffff_ffe0,
};

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
    kind: ShiftKind,
    destination: Location32,
    count: ShiftCount,
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
        &Operation::Shift {
            kind,
            destination,
            count,
        },
        "{bytes:02x?}"
    );
    assert!(memory.is_code_current(instruction.code_snapshot()));
}

fn memory_forms() -> Vec<Vec<u8>> {
    let mut forms = Vec::new();
    for (_, register, _) in FORMS {
        let mut bytes = vec![register[0], (register[1] & 0x38) | 5];
        bytes.extend_from_slice(&0x5010_u32.to_le_bytes());
        if register[0] == 0xc1 {
            bytes.push(0xff);
        }
        forms.push(bytes);
    }
    for (_, extension) in KINDS {
        let mut bytes = vec![0xc1, extension << 3 | 5];
        bytes.extend_from_slice(&0x5010_u32.to_le_bytes());
        bytes.push(0);
        forms.push(bytes);
    }
    forms
}

fn register_block(register: u8) -> Vec<u8> {
    let mut bytes = Vec::new();
    for (_, instruction, _) in FORMS {
        bytes.push(instruction[0]);
        bytes.push(instruction[1] | register);
        if instruction.len() == 3 {
            bytes.push(instruction[2]);
        }
    }
    for (_, extension) in KINDS {
        for raw in [0, 32] {
            bytes.extend_from_slice(&[0xc1, 0xc0 | extension << 3 | register, raw]);
        }
    }
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
fn canonical_register_shift_decodes_before_backend_admission() {
    let memory = code(CODE, &[0xd1, 0xe0]);
    let instruction = decode_one(&memory, GuestAddress(CODE)).unwrap();
    assert_eq!(instruction.length(), 2);
    assert_eq!(instruction.next_pc(), GuestAddress(CODE + 2));
}

#[test]
fn nine_literal_forms_preserve_kind_destination_and_count_identity() {
    let mut memory = code(CODE, &[0x90]);
    for (kind, bytes, count) in FORMS {
        decoded(
            &mut memory,
            bytes,
            kind,
            Location32::Register(Register32::Eax),
            count,
        );
        let mut indirect = vec![bytes[0], (bytes[1] & 0x38) | 0x44, 0x8b, 0xe0];
        if bytes.len() == 3 {
            indirect.push(bytes[2]);
        }
        decoded(
            &mut memory,
            &indirect,
            kind,
            Location32::Memory(ADDRESS),
            count,
        );
    }
}

#[test]
fn every_gpr_and_raw_unsigned_count_remain_distinct_from_cl() {
    let mut memory = code(CODE, &[0x90]);
    for (index, register) in REGISTERS.into_iter().enumerate() {
        for (kind, extension) in KINDS {
            for (opcode, count) in [(0xd1, ShiftCount::Immediate(1)), (0xd3, ShiftCount::Cl)] {
                decoded(
                    &mut memory,
                    &[opcode, 0xc0 | extension << 3 | index as u8],
                    kind,
                    Location32::Register(register),
                    count,
                );
            }
            for raw in [0, 1, 2, 31, 32, 33, 128, 255] {
                decoded(
                    &mut memory,
                    &[0xc1, 0xc0 | extension << 3 | index as u8, raw],
                    kind,
                    Location32::Register(register),
                    ShiftCount::Immediate(raw),
                );
            }
        }
    }
}

#[test]
fn memory_descriptions_preserve_base_index_scales_and_wrapping_displacement() {
    let mut memory = code(CODE, &[0x90]);
    for (kind, extension) in KINDS {
        for (index, register) in REGISTERS.into_iter().enumerate() {
            let mut bytes = vec![0xd3, 0x40 | extension << 3 | index as u8];
            if register == Register32::Esp {
                bytes.push(0x24);
            }
            bytes.push(0xe0);
            decoded(
                &mut memory,
                &bytes,
                kind,
                Location32::Memory(EffectiveAddress {
                    base: Some(register),
                    index: None,
                    scale: 1,
                    displacement: 0xffff_ffe0,
                }),
                ShiftCount::Cl,
            );
        }
        for (bits, scale) in [(0, 1), (0x40, 2), (0x80, 4), (0xc0, 8)] {
            decoded(
                &mut memory,
                &[0xd1, extension << 3 | 4, bits | 0x0b],
                kind,
                Location32::Memory(EffectiveAddress {
                    base: Some(Register32::Ebx),
                    index: Some(Register32::Ecx),
                    scale,
                    displacement: 0,
                }),
                ShiftCount::Immediate(1),
            );
        }
        for (tail, address) in [
            (
                vec![extension << 3 | 5, 0xff, 0xff, 0xff, 0xff],
                EffectiveAddress {
                    base: None,
                    index: None,
                    scale: 1,
                    displacement: u32::MAX,
                },
            ),
            (
                vec![extension << 3 | 4, 0x8d, 0xf8, 0xff, 0xff, 0xff],
                EffectiveAddress {
                    base: None,
                    index: Some(Register32::Ecx),
                    scale: 4,
                    displacement: 0xffff_fff8,
                },
            ),
        ] {
            let mut bytes = vec![0xc1];
            bytes.extend_from_slice(&tail);
            bytes.push(0);
            decoded(
                &mut memory,
                &bytes,
                kind,
                Location32::Memory(address),
                ShiftCount::Immediate(0),
            );
        }
    }
}

#[test]
fn width_rotates_sal_alias_and_prefixes_keep_precise_rejection_categories() {
    for (_, extension) in KINDS {
        for bytes in [
            vec![0x66, 0xc0, extension << 3, 2],
            vec![0x66, 0xd2, extension << 3],
            vec![0x66, 0x66, 0xd1, 0xc0 | extension << 3],
            vec![0x67, 0xd3, extension << 3 | 3],
            vec![0xf3, 0xd1, 0xc0 | extension << 3],
        ] {
            let memory = code(CODE, &bytes);
            assert_eq!(
                decode_one(&memory, GuestAddress(CODE)).err(),
                Some(DecodeError::Unsupported(UnsupportedFeature::Opcode)),
                "{bytes:02x?}"
            );
        }
        for (bytes, expected) in [
            (
                vec![0x64, 0xd3, extension << 3 | 3],
                DecodeError::Unsupported(UnsupportedFeature::Segment),
            ),
            (
                vec![0xf0, 0xd1, 0xc0 | extension << 3],
                DecodeError::InvalidEncoding,
            ),
        ] {
            let memory = code(CODE, &bytes);
            assert_eq!(
                decode_one(&memory, GuestAddress(CODE)).err(),
                Some(expected)
            );
        }
    }
    for bytes in [
        &[0xd1, 0xf0][..],
        &[0xc1, 0xf0, 1],
        &[0xd3, 0xf0],
        &[0xd1, 0x33],
        &[0xc1, 0x33, 1],
        &[0xd3, 0x33],
        &[0x66, 0x66, 0xd1, 0xc0],
        &[0x66, 0x66, 0xd1, 0xc8],
        &[0x66, 0x66, 0xd1, 0xd0],
        &[0x66, 0x66, 0xd1, 0xd8],
        &[0x66, 0x0f, 0xa4, 0xd0, 1],
        &[0x66, 0x0f, 0xac, 0xd0, 1],
    ] {
        let memory = code(CODE, bytes);
        assert_eq!(
            decode_one(&memory, GuestAddress(CODE)).err(),
            Some(DecodeError::Unsupported(UnsupportedFeature::Opcode)),
            "{bytes:02x?}"
        );
    }
}

#[test]
fn consumed_decode_bounds_stop_before_successor_and_report_missing_count() {
    for (_, extension) in KINDS {
        for opcode in [0xd1, 0xd3] {
            let memory = code(0x1ffe, &[opcode, 0xc0 | extension << 3]);
            let instruction = decode_one(&memory, GuestAddress(0x1ffe)).unwrap();
            assert_eq!(instruction.length(), 2);
            assert_eq!(instruction.next_pc(), GuestAddress(0x2000));
            assert!(memory.is_code_current(instruction.code_snapshot()));
            assert!(
                memory
                    .resolve(GuestAddress(0x2000), Access::Execute)
                    .is_err()
            );
        }
        for (pc, reason, fault_address) in [
            (0x1ffe, FaultReason::Unmapped, 0x2000),
            (0xffff_fffe, FaultReason::AddressOverflow, 0xffff_fffe),
        ] {
            let memory = code(pc, &[0xc1, 0xc0 | extension << 3]);
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
            assert_eq!(length, 3);
            assert_eq!(fault.address, GuestAddress(fault_address));
            assert_eq!(fault.access, Access::Execute);
            assert_eq!(fault.reason, reason);
        }
    }
}

#[test]
fn consumed_shift_snapshots_span_code_pages_and_ignore_guest_data() {
    for changed in [0x1ffe, 0x2000] {
        let mut memory = code(0x1ffe, &[0xc1, 0xe0, 0xff, 0xeb, 0]);
        let artifact =
            compile_entry_region(&memory, &[GuestAddress(0x1ffe)], CompileLimits::default())
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
                &[if changed == 0x1ffe { 0xc1 } else { 0xff }],
            )
            .unwrap();
        assert_eq!(
            artifact.wasm_bytes(&memory),
            Err(ArtifactError::CodeInvalidated)
        );
    }
}

#[test]
fn all_register_profiles_match_standalone_explicit_and_entry_artifacts() {
    for register in 0..8 {
        let mut bytes = register_block(register);
        let length = bytes.len();
        bytes.extend_from_slice(&[0x0f, 0x0b]);
        let mut memory = code(CODE, &bytes);
        memory
            .protect(
                PageRange::new(GuestAddress(CODE), 1).unwrap(),
                Permissions::EXECUTE,
            )
            .unwrap();
        let snapshot = memory.snapshot_code(GuestAddress(CODE), length).unwrap();
        let specs = [spec(CODE, length)];
        let entries = [GuestAddress(CODE)];
        for prepared in [
            prepare_region(&memory, &specs, CompileLimits::default()).unwrap(),
            prepare_entry_region(&memory, &entries, CompileLimits::default()).unwrap(),
        ] {
            assert_eq!(
                (prepared.block_count(), prepared.instruction_count()),
                (1, 16)
            );
            assert!(prepared.is_current(&memory));
        }
        let explicit = compile_region(&memory, &specs, CompileLimits::default()).unwrap();
        let cold = compile_entry_region(&memory, &entries, CompileLimits::default()).unwrap();
        assert_eq!(explicit.metadata(), cold.metadata());
        assert_eq!(
            explicit.wasm_bytes(&memory).unwrap(),
            cold.wasm_bytes(&memory).unwrap()
        );
        assert!(memory.is_code_current(&snapshot));
        assert_eq!(memory.mapped_pages(), 1);
        assert!(memory.resolve(GuestAddress(CODE), Access::Read).is_err());
        assert!(memory.resolve(GuestAddress(0x5010), Access::Read).is_err());
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
fn all_gprs_and_count_forms_admit_in_bound_profiles_without_stack_or_helpers() {
    for register in 0..8 {
        let bytes = register_block(register);
        for owner in [Owner::Replacement, Owner::Resident] {
            for entries in [false, true] {
                let mut engine = EngineInstance::new(1, KEY).unwrap();
                engine.map(CODE, 1, 7).unwrap();
                upload(&mut engine, CODE, &bytes);
                engine.protect(CODE, 1, 4).unwrap();
                let snapshot = engine
                    .memory()
                    .unwrap()
                    .snapshot_code(GuestAddress(CODE), bytes.len())
                    .unwrap();
                engine.arena_mut().unwrap().fill(0xa5);
                describe(&mut engine, CODE, bytes.len(), entries);
                let before = engine.arena().to_vec();
                let id = compile(&mut engine, owner, entries).unwrap();
                assert_eq!(
                    engine.arena(),
                    before,
                    "{owner:?} entries={entries} register={register}"
                );
                assert_eq!(engine.memory().unwrap().mapped_pages(), 1);
                assert!(engine.memory().unwrap().is_code_current(&snapshot));
                assert!(
                    engine
                        .memory()
                        .unwrap()
                        .resolve(GuestAddress(0x5010), Access::Read)
                        .is_err()
                );
                let bytes = module(&engine, owner, id);
                assert!(contains(bytes, b"guard"));
                for excluded in [
                    b"read8".as_slice(),
                    b"read16",
                    b"read32",
                    b"store32",
                    b"store_resident32",
                ] {
                    assert!(!contains(bytes, excluded), "{owner:?}: {excluded:?}");
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
fn every_memory_form_including_zero_refuses_all_standalone_backends_before_poison() {
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
fn admitted_prefix_and_each_memory_then_decode_refusal_preserves_both_old_owners_and_publication() {
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
                let mut bytes = vec![0xd1, 0xe0];
                bytes.extend_from_slice(&instruction);
                bytes.extend_from_slice(&[0x0f, 0x0b]);
                upload(&mut engine, CODE, &bytes);
                describe(&mut engine, CODE, bytes.len(), entries);
                let before = saved(&engine, keep);
                let error = instruction_error(
                    CODE + 2 + instruction.len() as u32,
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
fn register_shifts_charge_instruction_and_wasm_caps_before_later_poison() {
    for (_, extension) in KINDS {
        let instruction = [0xd1, 0xc0 | extension << 3];
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
        let memory = code(CODE, &[instruction[0], instruction[1], 0xeb, 0]);
        let limits = CompileLimits {
            wasm_bytes: 1,
            ..CompileLimits::default()
        };
        assert_eq!(
            compile_region(&memory, &[spec(CODE, 4)], limits).err(),
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
    let memory = code(CODE, &[0xd1, 0xe0, 0xc1, 0xf9, 0xff, 0xeb, 0]);
    let explicit = compile_region(
        &memory,
        &[spec(CODE, 2), spec(CODE + 2, 5)],
        CompileLimits::default(),
    )
    .unwrap();
    let cold = compile_entry_region(
        &memory,
        &[GuestAddress(CODE), GuestAddress(CODE + 2)],
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
    for (_, extension) in KINDS {
        let memory = code(CODE, &[0xc1, 0xc0 | extension << 3, 0xff, 0xeb, 0]);
        assert_eq!(
            prepare_entry_region(
                &memory,
                &[GuestAddress(CODE), GuestAddress(CODE + 2)],
                CompileLimits::default()
            )
            .err(),
            Some(CompileError::InvalidBlocks)
        );
        assert_eq!(
            prepare_region(&memory, &[spec(CODE, 2)], CompileLimits::default()).err(),
            Some(instruction_error(CODE, InstructionError::InvalidBlockEnd))
        );
    }
    for owner in [Owner::Replacement, Owner::Resident] {
        let mut engine = EngineInstance::new(1, KEY).unwrap();
        engine.map(CODE, 1, 7).unwrap();
        upload(&mut engine, 0x1ffc, &[0xd3, 0xf9, 0xeb, 0]);
        engine.protect(CODE, 1, 4).unwrap();
        describe(&mut engine, 0x1ffc, 4, true);
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
