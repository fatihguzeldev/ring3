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
fn authored_six_encoding_unary_block_is_admitted_by_explicit_and_entry_preparation() {
    // inc eax; inc eax(modrm); dec eax; dec eax(modrm); not eax; neg eax; jmp next.
    let bytes = [
        0x40, 0xff, 0xc0, 0x48, 0xff, 0xc8, 0xf7, 0xd0, 0xf7, 0xd8, 0xeb, 0,
    ];
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
        (1, 7)
    );
    assert_eq!((entries.block_count(), entries.instruction_count()), (1, 7));
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
        entries.wasm_bytes(&memory).unwrap()
    );
    assert!(memory.is_code_current(&snapshot));
    assert_eq!(memory.mapped_pages(), 1);
    assert!(memory.resolve(GuestAddress(0x1000), Access::Read).is_err());
    let mut output = vec![0; bytes.len()];
    memory.fetch(GuestAddress(0x1000), &mut output).unwrap();
    assert_eq!(output, bytes);
}

#[test]
fn all_six_encodings_admit_every_general_register_including_esp() {
    for register in 0..8u8 {
        admitted(&[0x40 + register]);
        admitted(&[0x48 + register]);
        admitted(&[0xff, 0xc0 + register]);
        admitted(&[0xff, 0xc8 + register]);
        admitted(&[0xf7, 0xd0 + register]);
        admitted(&[0xf7, 0xd8 + register]);
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
fn embedded_esp_operations_preserve_arena_and_code_without_stack_mapping() {
    let bytes = [
        0x44, 0xff, 0xc4, 0x4c, 0xff, 0xcc, 0xf7, 0xd4, 0xf7, 0xdc, 0xeb, 0,
    ];
    let mut explicit = embedded(&bytes);
    explicit.arena_mut().unwrap()[140..148].copy_from_slice(&[0, 0x10, 0, 0, 12, 0, 0, 0]);
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
        let mut output = [0; 12];
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
fn memory_unary_destinations_remain_backend_excluded_without_data_access() {
    for bytes in [
        &[0xff, 0x03][..],
        &[0xff, 0x0b][..],
        &[0xf7, 0x13][..],
        &[0xf7, 0x1b][..],
    ] {
        rejected(bytes, InstructionError::BackendUnsupported);
    }
}

#[test]
fn small_width_prefix_and_adjacent_rejections_keep_exact_categories() {
    for bytes in [
        &[0xfe, 0xc0][..],
        &[0xfe, 0xc8][..],
        &[0xf6, 0xd0][..],
        &[0xf6, 0xd8][..],
        &[0x66, 0x40][..],
        &[0x66, 0x48][..],
        &[0x66, 0xff, 0xc0][..],
        &[0x66, 0xff, 0xc8][..],
        &[0x66, 0xf7, 0xd0][..],
        &[0x66, 0xf7, 0xd8][..],
        &[0x67, 0xff, 0xc0][..],
        &[0xf3, 0xff, 0xc0][..],
        &[0xf0, 0xff, 0x03][..],
        &[0xd1, 0xe0][..],
    ] {
        rejected(
            bytes,
            InstructionError::Decode(DecodeError::Unsupported(UnsupportedFeature::Opcode)),
        );
    }
    rejected(
        &[0xf0, 0xff, 0xc0],
        InstructionError::Decode(DecodeError::InvalidEncoding),
    );
    rejected(
        &[0x64, 0xff, 0xc0],
        InstructionError::Decode(DecodeError::Unsupported(UnsupportedFeature::Segment)),
    );
    for bytes in [&[0x03, 0x03][..], &[0x0f, 0xb6, 0x03][..]] {
        rejected(bytes, InstructionError::BackendUnsupported);
    }
}

#[test]
fn unary_operations_are_sequential_and_charge_existing_caps_before_next_decode() {
    let mut bytes = vec![0x40; 63];
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
    let memory = code(0x1000, &[0x40; 65]);
    assert_eq!(
        prepare_region(&memory, &[spec(0x1000, 65)], CompileLimits::default()).err(),
        Some(CompileError::InstructionLimit)
    );
    assert_eq!(
        prepare_entry_region(&memory, &[GuestAddress(0x1000)], CompileLimits::default()).err(),
        Some(CompileError::InstructionLimit)
    );
    let memory = code(0x1000, &[0x40, 0xf4]);
    let limit = CompileLimits {
        instructions: 1,
        ..CompileLimits::default()
    };
    assert_eq!(
        prepare_region(&memory, &[spec(0x1000, 2)], limit).err(),
        Some(CompileError::InstructionLimit)
    );
    assert_eq!(
        prepare_entry_region(&memory, &[GuestAddress(0x1000)], limit).err(),
        Some(CompileError::InstructionLimit)
    );
    let memory = code(0x1000, &[0xf7, 0xd4, 0xeb, 0]);
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
fn final_complete_unary_stops_at_address_space_end_without_wrapping_fetch() {
    for (pc, bytes) in [(u32::MAX, &[0x40][..]), (u32::MAX - 1, &[0xf7, 0xd8][..])] {
        let memory = code(pc, bytes);
        let limits = CompileLimits {
            instructions: 1,
            ..CompileLimits::default()
        };
        let artifact = compile_entry_region(&memory, &[GuestAddress(pc)], limits).unwrap();
        assert_eq!(
            (artifact.metadata().blocks, artifact.metadata().instructions),
            (1, 1)
        );
        let explicit = compile_region(&memory, &[spec(pc, bytes.len())], limits).unwrap();
        assert_eq!(
            artifact.wasm_bytes(&memory).unwrap(),
            explicit.wasm_bytes(&memory).unwrap()
        );
    }
    let memory = code(u32::MAX, &[0xf7]);
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
fn unary_snapshots_cover_both_code_pages_and_ignore_unrelated_data() {
    for changed_page in [0x1000, 0x2000] {
        let mut memory = code(0x1fff, &[0xf7, 0xd4, 0xeb, 0]);
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
