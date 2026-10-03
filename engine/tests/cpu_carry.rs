use ring3_engine::{
    cpu::{
        UnsupportedFeature,
        dbt::{
            ArtifactError, BlockSpec, CompileError, CompileLimits, InstructionError, RegistryError,
            compile_entry_region, compile_region, prepare_entry_region, prepare_region,
        },
        x86::{
            Register32,
            decode::{DecodeError, decode_one},
            ir::{BinaryKind, EffectiveAddress, Location32, Operation, Value32},
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
const KINDS: [(BinaryKind, u8, u8, u8, u8); 2] = [
    (BinaryKind::Adc, 0x11, 0x13, 0x15, 2),
    (BinaryKind::Sbb, 0x19, 0x1b, 0x1d, 3),
];
const ADDRESS: EffectiveAddress = EffectiveAddress {
    base: Some(Register32::Ebx),
    index: Some(Register32::Ecx),
    scale: 4,
    displacement: 0xffff_ffe0,
};
const FORMS: [(BinaryKind, &[u8], Location32, Value32); 10] = [
    (
        BinaryKind::Adc,
        &[0x11, 0x54, 0x8b, 0xe0],
        Location32::Memory(ADDRESS),
        Value32::Register(Register32::Edx),
    ),
    (
        BinaryKind::Adc,
        &[0x13, 0x54, 0x8b, 0xe0],
        Location32::Register(Register32::Edx),
        Value32::Memory(ADDRESS),
    ),
    (
        BinaryKind::Adc,
        &[0x15, 0x78, 0x56, 0x34, 0x92],
        Location32::Register(Register32::Eax),
        Value32::Immediate(0x9234_5678),
    ),
    (
        BinaryKind::Adc,
        &[0x81, 0x54, 0x8b, 0xe0, 0x78, 0x56, 0x34, 0x92],
        Location32::Memory(ADDRESS),
        Value32::Immediate(0x9234_5678),
    ),
    (
        BinaryKind::Adc,
        &[0x83, 0x54, 0x8b, 0xe0, 0x80],
        Location32::Memory(ADDRESS),
        Value32::Immediate(0xffff_ff80),
    ),
    (
        BinaryKind::Sbb,
        &[0x19, 0x54, 0x8b, 0xe0],
        Location32::Memory(ADDRESS),
        Value32::Register(Register32::Edx),
    ),
    (
        BinaryKind::Sbb,
        &[0x1b, 0x54, 0x8b, 0xe0],
        Location32::Register(Register32::Edx),
        Value32::Memory(ADDRESS),
    ),
    (
        BinaryKind::Sbb,
        &[0x1d, 0x78, 0x56, 0x34, 0x92],
        Location32::Register(Register32::Eax),
        Value32::Immediate(0x9234_5678),
    ),
    (
        BinaryKind::Sbb,
        &[0x81, 0x5c, 0x8b, 0xe0, 0x78, 0x56, 0x34, 0x92],
        Location32::Memory(ADDRESS),
        Value32::Immediate(0x9234_5678),
    ),
    (
        BinaryKind::Sbb,
        &[0x83, 0x5c, 0x8b, 0xe0, 0x80],
        Location32::Memory(ADDRESS),
        Value32::Immediate(0xffff_ff80),
    ),
];

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
    kind: BinaryKind,
    destination: Location32,
    source: Value32,
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
        &Operation::Binary {
            kind,
            destination,
            source
        },
        "{bytes:02x?}"
    );
    assert!(memory.is_code_current(instruction.code_snapshot()));
}

#[test]
fn ten_literal_forms_preserve_direction_addresses_and_immediate_bits() {
    let mut memory = code(CODE, &[0x90]);
    for (kind, bytes, destination, source) in FORMS {
        decoded(&mut memory, bytes, kind, destination, source);
    }
}

#[test]
fn both_register_directions_cover_all_gprs_and_self_aliases() {
    let mut memory = code(CODE, &[0x90]);
    for (kind, rm, reg, _, _) in KINDS {
        for (destination, dst) in REGISTERS.into_iter().enumerate() {
            for (source, src) in REGISTERS.into_iter().enumerate() {
                decoded(
                    &mut memory,
                    &[rm, 0xc0 | (source as u8) << 3 | destination as u8],
                    kind,
                    Location32::Register(dst),
                    Value32::Register(src),
                );
                decoded(
                    &mut memory,
                    &[reg, 0xc0 | (destination as u8) << 3 | source as u8],
                    kind,
                    Location32::Register(dst),
                    Value32::Register(src),
                );
            }
        }
    }
}

#[test]
fn signed_imm8_and_full_imm32_remain_distinct_for_register_and_memory() {
    let mut memory = code(CODE, &[0x90]);
    for (kind, _, _, accumulator, extension) in KINDS {
        for (byte, expected) in [(0x80, 0xffff_ff80), (0xff, u32::MAX), (0, 0), (0x7f, 0x7f)] {
            decoded(
                &mut memory,
                &[0x83, 0xc0 | extension << 3, byte],
                kind,
                Location32::Register(Register32::Eax),
                Value32::Immediate(expected),
            );
            decoded(
                &mut memory,
                &[0x83, 0x03 | extension << 3, byte],
                kind,
                Location32::Memory(EffectiveAddress {
                    base: Some(Register32::Ebx),
                    index: None,
                    scale: 1,
                    displacement: 0,
                }),
                Value32::Immediate(expected),
            );
        }
        for value in [0, 0x80, 0x8000_0000, u32::MAX] {
            let mut bytes = vec![accumulator];
            bytes.extend_from_slice(&value.to_le_bytes());
            decoded(
                &mut memory,
                &bytes,
                kind,
                Location32::Register(Register32::Eax),
                Value32::Immediate(value),
            );
            let mut bytes = vec![0x81, 0xc0 | extension << 3];
            bytes.extend_from_slice(&value.to_le_bytes());
            decoded(
                &mut memory,
                &bytes,
                kind,
                Location32::Register(Register32::Eax),
                Value32::Immediate(value),
            );
        }
    }
}

#[test]
fn base_and_index_aliases_include_esp_without_an_esp_index() {
    let mut memory = code(CODE, &[0x90]);
    for (kind, rm, reg, _, _) in KINDS {
        for (index, register) in REGISTERS.into_iter().enumerate() {
            let bits = index as u8;
            let mut tail = vec![0x40 | bits << 3 | bits];
            if register == Register32::Esp {
                tail.push(0x24);
            }
            tail.push(0xe0);
            let address = EffectiveAddress {
                base: Some(register),
                index: None,
                scale: 1,
                displacement: 0xffff_ffe0,
            };
            let mut bytes = vec![reg];
            bytes.extend_from_slice(&tail);
            decoded(
                &mut memory,
                &bytes,
                kind,
                Location32::Register(register),
                Value32::Memory(address),
            );
            let mut bytes = vec![rm];
            bytes.extend_from_slice(&tail);
            decoded(
                &mut memory,
                &bytes,
                kind,
                Location32::Memory(address),
                Value32::Register(register),
            );
            let index_register = if register == Register32::Esp {
                Register32::Ecx
            } else {
                register
            };
            let index_bits = if register == Register32::Esp { 1 } else { bits };
            let address = EffectiveAddress {
                base: None,
                index: Some(index_register),
                scale: 4,
                displacement: 0xffff_fff8,
            };
            let tail = [
                0x04 | bits << 3,
                0x85 | index_bits << 3,
                0xf8,
                0xff,
                0xff,
                0xff,
            ];
            let mut bytes = vec![reg];
            bytes.extend_from_slice(&tail);
            decoded(
                &mut memory,
                &bytes,
                kind,
                Location32::Register(register),
                Value32::Memory(address),
            );
            let mut bytes = vec![rm];
            bytes.extend_from_slice(&tail);
            decoded(
                &mut memory,
                &bytes,
                kind,
                Location32::Memory(address),
                Value32::Register(register),
            );
        }
    }
}

#[test]
fn effective_addresses_preserve_scales_ebp_absolute_and_wrapping_displacement() {
    let mut memory = code(CODE, &[0x90]);
    for (kind, _, reg, _, _) in KINDS {
        for (scale_bits, scale) in [(0, 1), (0x40, 2), (0x80, 4), (0xc0, 8)] {
            decoded(
                &mut memory,
                &[reg, 0x04, scale_bits | 0x0b],
                kind,
                Location32::Register(Register32::Eax),
                Value32::Memory(EffectiveAddress {
                    base: Some(Register32::Ebx),
                    index: Some(Register32::Ecx),
                    scale,
                    displacement: 0,
                }),
            );
        }
        decoded(
            &mut memory,
            &[reg, 0x45, 0],
            kind,
            Location32::Register(Register32::Eax),
            Value32::Memory(EffectiveAddress {
                base: Some(Register32::Ebp),
                index: None,
                scale: 1,
                displacement: 0,
            }),
        );
        decoded(
            &mut memory,
            &[reg, 0x05, 0xff, 0xff, 0xff, 0xff],
            kind,
            Location32::Register(Register32::Eax),
            Value32::Memory(EffectiveAddress {
                base: None,
                index: None,
                scale: 1,
                displacement: u32::MAX,
            }),
        );
    }
}

#[test]
fn unsupported_widths_prefixes_and_invalid_lock_keep_error_categories() {
    for (_, rm, reg, accumulator, _) in KINDS {
        for bytes in [
            vec![rm - 1, 0xd0],
            vec![reg - 1, 0xd0],
            vec![accumulator - 1, 1],
            vec![0x66, rm, 0xd0],
            vec![0x67, reg, 0x03],
            vec![0xf3, rm, 0xd0],
            vec![0xf0, rm, 0x03],
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
                vec![0x64, reg, 0x03],
                DecodeError::Unsupported(UnsupportedFeature::Segment),
            ),
            (vec![0xf0, rm, 0xd0], DecodeError::InvalidEncoding),
        ] {
            let memory = code(CODE, &bytes);
            assert_eq!(
                decode_one(&memory, GuestAddress(CODE)).err(),
                Some(expected),
                "{bytes:02x?}"
            );
        }
    }
}

#[test]
fn decoder_reads_only_consumed_bytes_and_reports_precise_source_bounds() {
    for (kind, rm, _, accumulator, _) in KINDS {
        let memory = code(0x1ffe, &[rm, 0xd0]);
        let instruction = decode_one(&memory, GuestAddress(0x1ffe)).unwrap();
        assert_eq!(instruction.length(), 2);
        assert_eq!(instruction.next_pc(), GuestAddress(0x2000));
        assert!(
            matches!(instruction.operation(), Operation::Binary { kind: actual, .. } if *actual == kind)
        );
        for (pc, reason, fault_address) in [
            (0x1ffc, FaultReason::Unmapped, 0x2000),
            (0xffff_fffc, FaultReason::AddressOverflow, 0xffff_fffc),
        ] {
            let memory = code(pc, &[accumulator, 1, 2, 3]);
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
            assert_eq!(length, 5);
            assert_eq!(fault.address, GuestAddress(fault_address));
            assert_eq!(fault.access, Access::Execute);
            assert_eq!(fault.reason, reason);
        }
    }
}

#[test]
fn carry_snapshots_span_consumed_code_pages_and_ignore_guest_data() {
    for changed in [0x1fff, 0x2000] {
        let mut memory = code(0x1fff, &[0x11, 0xd0, 0x19, 0xd0, 0xeb, 0]);
        let artifact =
            compile_entry_region(&memory, &[GuestAddress(0x1fff)], CompileLimits::default())
                .unwrap();
        assert_eq!(artifact.metadata().instructions, 3);
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
                &[if changed == 0x1fff { 0x11 } else { 0xd0 }],
            )
            .unwrap();
        assert_eq!(
            artifact.wasm_bytes(&memory),
            Err(ArtifactError::CodeInvalidated)
        );
    }
}

#[test]
fn standalone_register_forms_match_explicit_and_cold_artifacts() {
    for (_, rm, reg, accumulator, extension) in KINDS {
        for instruction in [
            vec![rm, 0xd0],
            vec![reg, 0xc2],
            vec![accumulator, 0, 0, 0, 0x80],
            vec![0x81, 0xc0 | extension << 3, 0, 0, 0, 0x80],
            vec![0x83, 0xc0 | extension << 3, 0xff],
        ] {
            let mut bytes = instruction;
            bytes.extend_from_slice(&[0xeb, 0, 0x0f, 0x0b]);
            let memory = code(CODE, &bytes);
            let specs = [spec(CODE, bytes.len() - 2)];
            let explicit = compile_region(&memory, &specs, CompileLimits::default()).unwrap();
            let entries =
                compile_entry_region(&memory, &[GuestAddress(CODE)], CompileLimits::default())
                    .unwrap();
            assert_eq!(explicit.metadata(), entries.metadata());
            assert_eq!(explicit.metadata().instructions, 2);
            assert_eq!(
                explicit.wasm_bytes(&memory).unwrap(),
                entries.wasm_bytes(&memory).unwrap()
            );
        }
    }
}

fn instruction_error(pc: u32, cause: InstructionError) -> CompileError {
    CompileError::Instruction {
        pc: GuestAddress(pc),
        cause,
    }
}

#[test]
fn standalone_memory_stays_excluded_in_all_four_apis_before_poison() {
    for (_, instruction, destination, source) in FORMS {
        if !matches!(destination, Location32::Memory(_)) && !matches!(source, Value32::Memory(_)) {
            continue;
        }
        let mut bytes = instruction.to_vec();
        bytes.extend_from_slice(&[0x0f, 0x0b]);
        let memory = code(CODE, &bytes);
        let expected = Some(instruction_error(
            CODE,
            InstructionError::BackendUnsupported,
        ));
        let specs = [spec(CODE, bytes.len())];
        let entries = [GuestAddress(CODE)];
        assert_eq!(
            prepare_region(&memory, &specs, CompileLimits::default()).err(),
            expected
        );
        assert_eq!(
            compile_region(&memory, &specs, CompileLimits::default()).err(),
            expected
        );
        assert_eq!(
            prepare_entry_region(&memory, &entries, CompileLimits::default()).err(),
            expected
        );
        assert_eq!(
            compile_entry_region(&memory, &entries, CompileLimits::default()).err(),
            expected
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
fn bound_explicit_and_entry_forms_admit_without_data_and_preserve_arena() {
    for (_, instruction, destination, source) in FORMS {
        let mut bytes = instruction.to_vec();
        bytes.extend_from_slice(&[0xeb, 0, 0x0f, 0x0b]);
        let length = bytes.len() - 2;
        for owner in [Owner::Replacement, Owner::Resident] {
            for entries in [false, true] {
                let mut engine = EngineInstance::new(1, KEY).unwrap();
                engine.map(CODE, 1, 7).unwrap();
                upload(&mut engine, CODE, &bytes);
                engine.protect(CODE, 1, 4).unwrap();
                assert!(
                    engine
                        .memory()
                        .unwrap()
                        .resolve(GuestAddress(0x5010), Access::Read)
                        .is_err()
                );
                let snapshot = engine
                    .memory()
                    .unwrap()
                    .snapshot_code(GuestAddress(CODE), length)
                    .unwrap();
                engine.arena_mut().unwrap().fill(0xa5);
                describe(&mut engine, CODE, length, entries);
                let before = engine.arena().to_vec();
                let id = compile(&mut engine, owner, entries).unwrap();
                assert_eq!(engine.arena(), before, "{owner:?} entries={entries}");
                assert_eq!(engine.memory().unwrap().mapped_pages(), 1);
                assert!(engine.memory().unwrap().is_code_current(&snapshot));
                let has_memory = matches!(source, Value32::Memory(_))
                    || matches!(destination, Location32::Memory(_));
                let writes = matches!(destination, Location32::Memory(_));
                let bytes = module(&engine, owner, id);
                assert!(contains(bytes, b"guard"));
                assert_eq!(contains(bytes, b"read32"), has_memory);
                assert_eq!(
                    contains(
                        bytes,
                        match owner {
                            Owner::Replacement => b"store32".as_slice(),
                            Owner::Resident => b"store_resident32".as_slice(),
                        }
                    ),
                    writes
                );
                for excluded in [b"read8".as_slice(), b"read16"] {
                    assert!(!contains(bytes, excluded));
                }
                match owner {
                    Owner::Replacement => {
                        assert_eq!(engine.generation(), id as u32);
                        engine.guard(KEY, id as u32).unwrap();
                    }
                    Owner::Resident => {
                        assert_eq!(engine.generation(), 0);
                        assert_eq!(engine.artifact_bytes(), Err(HostError::InvalidArtifact));
                        assert_eq!(engine.lookup_resident(CODE).unwrap().get(), id);
                        engine.guard_resident(KEY, id).unwrap();
                    }
                }
            }
        }
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
fn carry_prefix_then_unsupported_width_refusal_preserves_prior_owners_and_publication() {
    for (_, rm, _, _, _) in KINDS {
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
                upload(&mut engine, CODE, &[rm, 0xd0, rm - 1, 0xd0]);
                describe(&mut engine, CODE, 4, entries);
                let before = saved(&engine, keep);
                let expected = instruction_error(
                    CODE + 2,
                    InstructionError::Decode(DecodeError::Unsupported(UnsupportedFeature::Opcode)),
                );
                let error = match owner {
                    Owner::Replacement => HostError::Compile(expected),
                    Owner::Resident => HostError::Resident(RegistryError::Compile(expected)),
                };
                assert_eq!(compile(&mut engine, owner, entries), Err(error));
                assert_eq!(saved(&engine, keep), before);
                engine.guard(KEY, before.generation).unwrap();
                engine.guard_resident(KEY, keep).unwrap();
                assert!(engine.lookup_resident(CODE).is_err());
            }
        }
    }
}

#[test]
fn carry_instructions_charge_caps_before_later_poison() {
    for (_, rm, _, _, _) in KINDS {
        let mut bytes = [rm, 0xd0].repeat(63);
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
        let mut bytes = [rm, 0xd0].repeat(64);
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
        let memory = code(CODE, &[rm, 0xd0, 0xeb, 0]);
        assert_eq!(
            compile_entry_region(
                &memory,
                &[GuestAddress(CODE)],
                CompileLimits {
                    wasm_bytes: 1,
                    ..CompileLimits::default()
                }
            )
            .err(),
            Some(CompileError::WasmLimit)
        );
    }
}

#[test]
fn cold_discovery_stops_at_next_seed_and_refuses_crossing_inside_carry_instruction() {
    let memory = code(CODE, &[0x11, 0xd0, 0x19, 0xd0, 0xeb, 0]);
    let entries = [GuestAddress(CODE), GuestAddress(CODE + 2)];
    let explicit = compile_region(
        &memory,
        &[spec(CODE, 2), spec(CODE + 2, 4)],
        CompileLimits::default(),
    )
    .unwrap();
    let cold = compile_entry_region(&memory, &entries, CompileLimits::default()).unwrap();
    assert_eq!(
        (cold.metadata().blocks, cold.metadata().instructions),
        (2, 3)
    );
    assert_eq!(
        explicit.wasm_bytes(&memory).unwrap(),
        cold.wasm_bytes(&memory).unwrap()
    );
    for (_, _, _, accumulator, _) in KINDS {
        let memory = code(CODE, &[accumulator, 1, 2, 3, 4, 0xeb, 0]);
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
            prepare_region(&memory, &[spec(CODE, 4)], CompileLimits::default()).err(),
            Some(instruction_error(CODE, InstructionError::InvalidBlockEnd))
        );
    }
}

#[test]
fn bound_cold_carry_and_terminal_jump_do_not_fetch_unmapped_successor_or_data() {
    for (_, _, reg, _, _) in KINDS {
        for owner in [Owner::Replacement, Owner::Resident] {
            let mut engine = EngineInstance::new(1, KEY).unwrap();
            engine.map(CODE, 1, 7).unwrap();
            upload(&mut engine, 0x1ffc, &[reg, 0x03, 0xeb, 0]);
            engine.protect(CODE, 1, 4).unwrap();
            describe(&mut engine, 0x1ffc, 4, true);
            let before = engine.arena().to_vec();
            let id = compile(&mut engine, owner, true).unwrap();
            assert_eq!(engine.arena(), before);
            assert_eq!(engine.memory().unwrap().mapped_pages(), 1);
            assert!(contains(module(&engine, owner, id), b"read32"));
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
                    .resolve(GuestAddress(0x3000), Access::Read)
                    .is_err()
            );
        }
    }
}
