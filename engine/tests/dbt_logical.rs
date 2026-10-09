use ring3_engine::cpu::dbt::{
    ArtifactError, BlockSpec, CompileError, CompileLimits, InstructionError, compile_entry_region,
    compile_region, prepare_entry_region, prepare_region,
};
use ring3_engine::cpu::{UnsupportedFeature, x86::decode::DecodeError};
use ring3_engine::memory::{AddressSpace, GuestAddress, PageRange, Permissions};
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
fn authored_logical_block_is_admitted_by_explicit_and_entry_preparation() {
    // and eax,ebx; or eax,ecx; xor esp,esp; test eax,0x12345678; jmp next.
    let bytes = [
        0x21, 0xd8, 0x0b, 0xc1, 0x31, 0xe4, 0xa9, 0x78, 0x56, 0x34, 0x12, 0xeb, 0,
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
        (1, 5)
    );
    assert_eq!((entries.block_count(), entries.instruction_count()), (1, 5));
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
}

#[test]
fn both_register_directions_and_aliases_include_every_general_register() {
    for opcode in [0x21u8, 0x23, 0x09, 0x0b, 0x31, 0x33, 0x85] {
        for register in 0..8 {
            for source in [register, (register + 3) % 8] {
                admitted(&[opcode, 0xc0 | source << 3 | register]);
            }
        }
    }
}

#[test]
fn accumulator_and_modrm_imm32_encodings_keep_the_same_admitted_profile() {
    for opcode in [0x25, 0x0d, 0x35, 0xa9] {
        admitted(&[opcode, 0x98, 0xba, 0xdc, 0xfe]);
    }
    for (opcode, extension) in [(0x81u8, 4u8), (0x81, 1), (0x81, 6), (0xf7, 0)] {
        for register in 0..8 {
            admitted(&[
                opcode,
                0xc0 | extension << 3 | register,
                0x98,
                0xba,
                0xdc,
                0xfe,
            ]);
        }
    }
}

#[test]
fn signed_imm8_forms_cover_both_sign_boundaries_for_all_destinations() {
    for extension in [4u8, 1, 6] {
        for register in 0..8 {
            for immediate in [0, 0x7f, 0x80, 0xff] {
                admitted(&[0x83, 0xc0 | extension << 3 | register, immediate]);
            }
        }
    }
}

fn embedded(bytes: &[u8]) -> EngineInstance {
    let mut engine = EngineInstance::new(1, 0xabcd_ef01_1234_5678).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    engine.arena_mut().unwrap()[140..140 + bytes.len()].copy_from_slice(bytes);
    engine.upload(0x1000, bytes.len() as u32).unwrap();
    engine.arena_mut().unwrap()[..140].fill(0xa5);
    engine
}

#[test]
fn embedded_explicit_and_entry_emission_agree_without_state_or_guest_data_access() {
    let bytes = [
        0x21, 0xd8, 0x0b, 0xc1, 0x31, 0xe4, 0xa9, 0x78, 0x56, 0x34, 0x12, 0xeb, 0,
    ];
    let mut explicit = embedded(&bytes);
    explicit.arena_mut().unwrap()[140..148].copy_from_slice(&[0, 0x10, 0, 0, 13, 0, 0, 0]);
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
        let mut output = [0; 13];
        engine
            .memory()
            .unwrap()
            .read(GuestAddress(0x1000), &mut output)
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
fn standalone_memory_logical_destinations_and_immediates_remain_backend_excluded() {
    for instruction in [
        &[0x21, 0x03][..],
        &[0x09, 0x03][..],
        &[0x31, 0x03][..],
        &[0x81, 0x23, 1, 0, 0, 0][..],
        &[0x81, 0x0b, 1, 0, 0, 0][..],
        &[0x81, 0x33, 1, 0, 0, 0][..],
        &[0x83, 0x23, 0x80][..],
        &[0x83, 0x0b, 0x80][..],
        &[0x83, 0x33, 0x80][..],
    ] {
        let mut bytes = vec![0x90];
        bytes.extend_from_slice(instruction);
        let memory = code(0x1000, &bytes);
        let specs = [spec(0x1000, bytes.len())];
        let expected = Some(CompileError::Instruction {
            pc: GuestAddress(0x1001),
            cause: InstructionError::BackendUnsupported,
        });
        for actual in [
            prepare_region(&memory, &specs, CompileLimits::default()).err(),
            compile_region(&memory, &specs, CompileLimits::default()).err(),
            prepare_entry_region(&memory, &[GuestAddress(0x1000)], CompileLimits::default()).err(),
            compile_entry_region(&memory, &[GuestAddress(0x1000)], CompileLimits::default()).err(),
        ] {
            assert_eq!(actual, expected, "{instruction:02x?}");
        }
    }
}

#[test]
fn small_width_prefix_and_adjacent_operation_errors_retain_their_exact_categories() {
    for instruction in [
        &[0x66, 0x20, 0x00][..],
        &[0x66, 0x08, 0x00][..],
        &[0x66, 0x30, 0x00][..],
        &[0x66, 0x84, 0x03][..],
        &[0x66, 0x66, 0x21, 0xc0][..],
        &[0x66, 0x66, 0x09, 0xc0][..],
        &[0x66, 0x66, 0x31, 0xc0][..],
        &[0x66, 0x66, 0x85, 0xc0][..],
        &[0x67, 0x21, 0xc0][..],
        &[0xf3, 0x21, 0xc0][..],
        &[0xf0, 0x21, 0x03][..],
    ] {
        rejected(
            instruction,
            InstructionError::Decode(DecodeError::Unsupported(UnsupportedFeature::Opcode)),
        );
    }
    rejected(
        &[0x64, 0x21, 0xc0],
        InstructionError::Decode(DecodeError::Unsupported(UnsupportedFeature::Segment)),
    );
    rejected(
        &[0xf0, 0x21, 0xc0],
        InstructionError::Decode(DecodeError::InvalidEncoding),
    );
    rejected(
        &[0x0f, 0x06],
        InstructionError::Decode(DecodeError::Unsupported(UnsupportedFeature::Privileged)),
    );
    let memory = code(0x1000, &[0x90, 0x0f, 0xb6, 0x03]);
    let expected = Some(CompileError::Instruction {
        pc: GuestAddress(0x1001),
        cause: InstructionError::BackendUnsupported,
    });
    assert_eq!(
        prepare_region(&memory, &[spec(0x1000, 4)], CompileLimits::default()).err(),
        expected
    );
    assert_eq!(
        prepare_entry_region(&memory, &[GuestAddress(0x1000)], CompileLimits::default()).err(),
        expected
    );
}

#[test]
fn logical_instructions_charge_existing_caps_and_do_not_terminate_discovery() {
    let mut bytes = [0x31, 0xc0].repeat(63);
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
    let bytes = [0x31, 0xc0].repeat(65);
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
    let memory = code(0x1000, &[0x85, 0xe4, 0xeb, 0]);
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
fn logical_code_snapshots_cover_both_consumed_pages_and_ignore_unrelated_data() {
    for changed_page in [0x1000, 0x2000] {
        let mut memory = code(0x1fff, &[0x31, 0xc0, 0xeb, 0]);
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
        let address = if changed_page == 0x1000 {
            0x1fff
        } else {
            0x2000
        };
        let value = if changed_page == 0x1000 { 0x31 } else { 0xc0 };
        memory.write(GuestAddress(address), &[value]).unwrap();
        assert!(!prepared.is_current(&memory));
        assert_eq!(
            artifact.wasm_bytes(&memory),
            Err(ArtifactError::CodeInvalidated)
        );
    }
}
