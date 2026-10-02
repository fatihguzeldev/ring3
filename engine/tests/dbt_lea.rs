use ring3_engine::cpu::dbt::{
    ArtifactError, BlockSpec, CompileError, CompileLimits, InstructionError, compile_entry_region,
    compile_region, prepare_entry_region, prepare_region,
};
use ring3_engine::cpu::{UnsupportedFeature, x86::decode::DecodeError};
use ring3_engine::memory::{
    Access, AddressSpace, FaultReason, GuestAddress, MemoryFault, PageRange, Permissions,
};
use ring3_engine::process::{EngineInstance, HostError};

fn code(pc: u32, bytes: &[u8]) -> AddressSpace {
    let first = pc & !0xfff;
    let pages = (u64::from(pc - first) + bytes.len() as u64).div_ceil(4096) as u32;
    let mut memory = AddressSpace::new(pages + 1).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(first), pages).unwrap(),
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

#[test]
fn authored_lea_is_admitted_by_explicit_and_entry_preparation() {
    // lea edi,[ebx+ecx*4-32]; jmp next.
    let bytes = [0x8d, 0x7c, 0x8b, 0xe0, 0xeb, 0];
    let memory = code(0x1000, &bytes);
    let explicit = prepare_region(
        &memory,
        &[spec(0x1000, bytes.len())],
        CompileLimits::default(),
    )
    .unwrap();
    let entries =
        prepare_entry_region(&memory, &[GuestAddress(0x1000)], CompileLimits::default()).unwrap();
    assert_eq!(
        (explicit.block_count(), explicit.instruction_count()),
        (1, 2)
    );
    assert_eq!((entries.block_count(), entries.instruction_count()), (1, 2));
    let explicit = compile_region(
        &memory,
        &[spec(0x1000, bytes.len())],
        CompileLimits::default(),
    )
    .unwrap();
    let entries =
        compile_entry_region(&memory, &[GuestAddress(0x1000)], CompileLimits::default()).unwrap();
    assert_eq!(explicit.metadata(), entries.metadata());
    assert_eq!(
        explicit.wasm_bytes(&memory).unwrap(),
        entries.wasm_bytes(&memory).unwrap()
    );
}

fn admitted(instruction: &[u8]) {
    let mut bytes = instruction.to_vec();
    bytes.extend_from_slice(&[0xeb, 0]);
    let mut memory = code(0x1000, &bytes);
    memory
        .protect(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::EXECUTE,
        )
        .unwrap();
    let snapshot = memory
        .snapshot_code(GuestAddress(0x1000), bytes.len())
        .unwrap();
    let explicit = prepare_region(
        &memory,
        &[spec(0x1000, bytes.len())],
        CompileLimits::default(),
    )
    .unwrap();
    let entries =
        prepare_entry_region(&memory, &[GuestAddress(0x1000)], CompileLimits::default()).unwrap();
    assert_eq!(
        (explicit.block_count(), explicit.instruction_count()),
        (1, 2),
        "{instruction:02x?}"
    );
    assert_eq!(
        (entries.block_count(), entries.instruction_count()),
        (1, 2),
        "{instruction:02x?}"
    );
    let explicit = compile_region(
        &memory,
        &[spec(0x1000, bytes.len())],
        CompileLimits::default(),
    )
    .unwrap();
    let entries =
        compile_entry_region(&memory, &[GuestAddress(0x1000)], CompileLimits::default()).unwrap();
    assert_eq!(explicit.metadata(), entries.metadata());
    assert_eq!(
        explicit.wasm_bytes(&memory).unwrap(),
        entries.wasm_bytes(&memory).unwrap(),
        "{instruction:02x?}"
    );
    assert!(memory.is_code_current(&snapshot));
    assert_eq!(memory.mapped_pages(), 1);
    assert!(memory.resolve(GuestAddress(0x1000), Access::Read).is_err());
    let mut output = vec![0; bytes.len()];
    memory.fetch(GuestAddress(0x1000), &mut output).unwrap();
    assert_eq!(output, bytes);
}

#[test]
fn authored_address_forms_admit_all_scales_and_displacement_classes_without_data_mapping() {
    for (_name, bytes) in [
        ("base", &[0x8d, 0x03][..]),
        ("ebp zero", &[0x8d, 0x45, 0][..]),
        ("esp base", &[0x8d, 0x04, 0x24][..]),
        ("base index scale1", &[0x8d, 0x04, 0x0b][..]),
        ("base index scale2", &[0x8d, 0x04, 0x4b][..]),
        ("base index scale4", &[0x8d, 0x04, 0x8b][..]),
        ("base index scale8", &[0x8d, 0x04, 0xcb][..]),
        (
            "no base scale1",
            &[0x8d, 0x04, 0x0d, 0x78, 0x56, 0x34, 0x12][..],
        ),
        (
            "no base scale2",
            &[0x8d, 0x04, 0x4d, 0x78, 0x56, 0x34, 0x12][..],
        ),
        (
            "no base scale4",
            &[0x8d, 0x04, 0x8d, 0x78, 0x56, 0x34, 0x12][..],
        ),
        (
            "no base scale8",
            &[0x8d, 0x04, 0xcd, 0x78, 0x56, 0x34, 0x12][..],
        ),
        ("absolute max", &[0x8d, 0x05, 0xff, 0xff, 0xff, 0xff][..]),
        ("disp8 negative", &[0x8d, 0x43, 0x80][..]),
        ("disp8 positive", &[0x8d, 0x43, 0x7f][..]),
        ("disp32 negative", &[0x8d, 0x83, 0, 0, 0, 0x80][..]),
        ("disp32 positive", &[0x8d, 0x83, 0xff, 0xff, 0xff, 0x7f][..]),
        ("disp32 minus1", &[0x8d, 0x83, 0xff, 0xff, 0xff, 0xff][..]),
        ("esp scaled index", &[0x8d, 0x44, 0x8c, 0xe0][..]),
        ("esp destination base", &[0x8d, 0x64, 0x24, 4][..]),
        ("destination base and index", &[0x8d, 0x44, 0xc0, 0x80][..]),
        ("sib no index ignores scale bits", &[0x8d, 0x04, 0xe3][..]),
    ] {
        admitted(bytes);
    }
}

#[test]
fn every_destination_can_alias_its_base_and_every_legal_index() {
    for register in 0..8u8 {
        match register {
            4 => admitted(&[0x8d, 0x24, 0x24]),
            5 => admitted(&[0x8d, 0x6d, 0]),
            _ => admitted(&[0x8d, register << 3 | register]),
        }
        if register != 4 {
            admitted(&[0x8d, 0x44 | register << 3, 0x83 | register << 3, 0xff]);
        }
    }
}

fn embedded(bytes: &[u8]) -> EngineInstance {
    let mut engine = EngineInstance::new(1, 0xabcd_ef01_1234_5678).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    engine.arena_mut().unwrap()[140..140 + bytes.len()].copy_from_slice(bytes);
    engine.upload(0x1000, bytes.len() as u32).unwrap();
    engine.arena_mut().unwrap()[..140].fill(0xa5);
    engine.protect(0x1000, 1, 4).unwrap();
    engine
}

#[test]
fn embedded_explicit_and_entry_modules_agree_and_leave_all_arena_and_code_bytes_untouched() {
    let bytes = [0x8d, 0x7c, 0x8b, 0xe0, 0xeb, 0];
    let mut explicit = embedded(&bytes);
    explicit.arena_mut().unwrap()[140..148].copy_from_slice(&[0, 0x10, 0, 0, 6, 0, 0, 0]);
    let before = explicit.arena().to_vec();
    let snapshot = explicit
        .memory()
        .unwrap()
        .snapshot_code(GuestAddress(0x1000), bytes.len())
        .unwrap();
    assert_eq!(explicit.compile(1), Ok(1));
    assert_eq!(explicit.arena(), before);
    assert!(explicit.memory().unwrap().is_code_current(&snapshot));
    let mut entries = embedded(&bytes);
    entries.arena_mut().unwrap()[140..144].copy_from_slice(&[0, 0x10, 0, 0]);
    let before = entries.arena().to_vec();
    assert_eq!(entries.compile_entries(1, 0), Ok(1));
    assert_eq!(entries.arena(), before);
    assert_eq!(
        explicit.artifact_bytes().unwrap(),
        entries.artifact_bytes().unwrap()
    );
    for engine in [explicit, entries] {
        let mut output = [0; 6];
        engine
            .memory()
            .unwrap()
            .fetch(GuestAddress(0x1000), &mut output)
            .unwrap();
        assert_eq!(output, bytes);
        assert_eq!(engine.memory().unwrap().mapped_pages(), 1);
    }
}

fn rejected(instruction: &[u8], cause: InstructionError) {
    let mut bytes = vec![0x90];
    bytes.extend_from_slice(instruction);
    let expected = CompileError::Instruction {
        pc: GuestAddress(0x1001),
        cause,
    };
    let memory = code(0x1000, &bytes);
    assert_eq!(
        prepare_region(
            &memory,
            &[spec(0x1000, bytes.len())],
            CompileLimits::default()
        )
        .err(),
        Some(expected),
        "{instruction:02x?}"
    );
    assert_eq!(
        prepare_entry_region(&memory, &[GuestAddress(0x1000)], CompileLimits::default()).err(),
        Some(expected),
        "{instruction:02x?}"
    );
    for entry_format in [false, true] {
        let mut engine = embedded(&bytes);
        engine.arena_mut().unwrap()[140..148].copy_from_slice(&[
            0,
            0x10,
            0,
            0,
            bytes.len() as u8,
            0,
            0,
            0,
        ]);
        let arena = engine.arena().to_vec();
        let result = if entry_format {
            engine.compile_entries(1, 0)
        } else {
            engine.compile(1)
        };
        assert_eq!(
            result,
            Err(HostError::Compile(expected)),
            "{instruction:02x?}"
        );
        assert_eq!(engine.arena(), arena);
        assert_eq!(engine.generation(), 0);
    }
}

#[test]
fn malformed_lea_prefixes_and_adjacent_exclusions_keep_exact_error_categories() {
    rejected(
        &[0x8d, 0xc0],
        InstructionError::Decode(DecodeError::InvalidEncoding),
    );
    rejected(
        &[0xf0, 0x8d, 0x03],
        InstructionError::Decode(DecodeError::InvalidEncoding),
    );
    for bytes in [
        &[0x66, 0x8d, 0x03][..],
        &[0x67, 0x8d, 0x03][..],
        &[0xf3, 0x8d, 0x03][..],
    ] {
        rejected(
            bytes,
            InstructionError::Decode(DecodeError::Unsupported(UnsupportedFeature::Opcode)),
        );
    }
    rejected(
        &[0x64, 0x8d, 0x03],
        InstructionError::Decode(DecodeError::Unsupported(UnsupportedFeature::Segment)),
    );
    for bytes in [&[0x03, 0x03][..], &[0x0f, 0xb6, 0xc0][..], &[0x40][..]] {
        rejected(bytes, InstructionError::BackendUnsupported);
    }
}

#[test]
fn lea_is_sequential_and_charges_existing_instruction_and_wasm_limits() {
    let mut bytes = [0x8d, 0x03].repeat(63);
    bytes.extend_from_slice(&[0xeb, 0]);
    let memory = code(0x1000, &bytes);
    assert_eq!(
        prepare_region(
            &memory,
            &[spec(0x1000, bytes.len())],
            CompileLimits::default()
        )
        .unwrap()
        .instruction_count(),
        64
    );
    assert_eq!(
        prepare_entry_region(&memory, &[GuestAddress(0x1000)], CompileLimits::default())
            .unwrap()
            .instruction_count(),
        64
    );
    let bytes = [0x8d, 0x03].repeat(65);
    let memory = code(0x1000, &bytes);
    assert_eq!(
        prepare_region(
            &memory,
            &[spec(0x1000, bytes.len())],
            CompileLimits::default()
        )
        .err(),
        Some(CompileError::InstructionLimit)
    );
    assert_eq!(
        prepare_entry_region(&memory, &[GuestAddress(0x1000)], CompileLimits::default()).err(),
        Some(CompileError::InstructionLimit)
    );
    let memory = code(0x1000, &[0x8d, 0x03, 0xeb, 0]);
    assert_eq!(
        prepare_entry_region(
            &memory,
            &[GuestAddress(0x1000)],
            CompileLimits {
                instructions: 1,
                ..CompileLimits::default()
            }
        )
        .err(),
        Some(CompileError::InstructionLimit)
    );
    assert_eq!(
        compile_entry_region(
            &memory,
            &[GuestAddress(0x1000)],
            CompileLimits {
                wasm_bytes: 1,
                ..CompileLimits::default()
            }
        )
        .err(),
        Some(CompileError::WasmLimit)
    );
}

#[test]
fn final_complete_lea_stops_at_address_space_end_and_incomplete_lea_keeps_execute_overflow() {
    let memory = code(u32::MAX - 1, &[0x8d, 0x03]);
    let limits = CompileLimits {
        instructions: 1,
        ..CompileLimits::default()
    };
    let artifact = compile_entry_region(&memory, &[GuestAddress(u32::MAX - 1)], limits).unwrap();
    assert_eq!(
        (artifact.metadata().blocks, artifact.metadata().instructions),
        (1, 1)
    );
    let explicit = compile_region(&memory, &[spec(u32::MAX - 1, 2)], limits).unwrap();
    assert_eq!(
        artifact.wasm_bytes(&memory).unwrap(),
        explicit.wasm_bytes(&memory).unwrap()
    );
    let memory = code(u32::MAX, &[0x8d]);
    assert_eq!(
        prepare_entry_region(&memory, &[GuestAddress(u32::MAX)], CompileLimits::default()).err(),
        Some(CompileError::Instruction {
            pc: GuestAddress(u32::MAX),
            cause: InstructionError::Decode(DecodeError::MemoryFault {
                pc: GuestAddress(u32::MAX),
                fault: MemoryFault {
                    address: GuestAddress(u32::MAX),
                    access: Access::Execute,
                    reason: FaultReason::AddressOverflow
                },
                length: 2,
            }),
        })
    );
}

#[test]
fn lea_snapshots_cover_both_consumed_pages_and_ignore_unrelated_data() {
    for changed_page in [0x1000, 0x2000] {
        let mut memory = code(0x1fff, &[0x8d, 0x7c, 0x8b, 0xe0, 0xeb, 0]);
        let prepared =
            prepare_entry_region(&memory, &[GuestAddress(0x1fff)], CompileLimits::default())
                .unwrap();
        let artifact =
            compile_entry_region(&memory, &[GuestAddress(0x1fff)], CompileLimits::default())
                .unwrap();
        memory
            .map_zeroed(
                PageRange::new(GuestAddress(0x4000), 1).unwrap(),
                Permissions::ALL,
            )
            .unwrap();
        memory.write(GuestAddress(0x4000), &[0x90]).unwrap();
        assert!(prepared.is_current(&memory));
        artifact.wasm_bytes(&memory).unwrap();
        memory
            .protect(
                PageRange::new(GuestAddress(changed_page), 1).unwrap(),
                Permissions::ALL,
            )
            .unwrap();
        assert!(!prepared.is_current(&memory));
        assert_eq!(
            artifact.wasm_bytes(&memory),
            Err(ArtifactError::CodeInvalidated)
        );
    }
}
