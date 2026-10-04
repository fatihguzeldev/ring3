use ring3_engine::cpu::dbt::{
    ArtifactError, BlockSpec, CompileError, CompileLimits, InstructionError, compile_entry_region,
    compile_region, prepare_entry_region, prepare_region,
};
use ring3_engine::cpu::{
    UnsupportedFeature,
    x86::{
        Register32,
        decode::{DecodeError, decode_one},
        ir::{ExtensionKind, Operation, SmallSource, SmallWidth},
    },
};
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
fn authored_register_extension_block_is_admitted_by_explicit_and_entry_preparation() {
    // movzx eax,ah; movzx esp,sp; movsx edx,bl; movsx edi,si; jmp next.
    let bytes = [
        0x0f, 0xb6, 0xc4, 0x0f, 0xb7, 0xe4, 0x0f, 0xbe, 0xd3, 0x0f, 0xbf, 0xfe, 0xeb, 0,
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
}

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

const BYTE_SOURCES: [(Register32, bool); 8] = [
    (Register32::Eax, false),
    (Register32::Ecx, false),
    (Register32::Edx, false),
    (Register32::Ebx, false),
    (Register32::Eax, true),
    (Register32::Ecx, true),
    (Register32::Edx, true),
    (Register32::Ebx, true),
];

const FORMS: [(u8, ExtensionKind, SmallWidth); 4] = [
    (0xb6, ExtensionKind::Zero, SmallWidth::Byte),
    (0xb7, ExtensionKind::Zero, SmallWidth::Word),
    (0xbe, ExtensionKind::Sign, SmallWidth::Byte),
    (0xbf, ExtensionKind::Sign, SmallWidth::Word),
];

fn destination_block(destination: u8) -> Vec<u8> {
    let mut bytes = Vec::new();
    for (opcode, _, _) in FORMS {
        for source in 0..8 {
            bytes.extend_from_slice(&[0x0f, opcode, 0xc0 | (destination << 3) | source]);
        }
    }
    bytes.extend_from_slice(&[0xeb, 0]);
    bytes
}

#[test]
fn four_opcodes_all_sources_and_destinations_keep_checked_identities_and_byte_parity() {
    for (destination_index, destination) in REGISTERS.into_iter().enumerate() {
        let bytes = destination_block(destination_index as u8);
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
        let mut pc = 0x1000;
        for (_, kind, width) in FORMS {
            for source_index in 0..8 {
                let (register, high_byte) = match width {
                    SmallWidth::Byte => BYTE_SOURCES[source_index],
                    SmallWidth::Word => (REGISTERS[source_index], false),
                };
                let instruction = decode_one(&memory, GuestAddress(pc)).unwrap();
                assert_eq!(instruction.length(), 3);
                assert_eq!(
                    *instruction.operation(),
                    Operation::Extend {
                        kind,
                        destination,
                        source: SmallSource::Register {
                            register,
                            width,
                            high_byte
                        },
                    },
                    "destination {destination:?}, source {source_index}, {kind:?}/{width:?}"
                );
                pc += 3;
            }
        }
        let explicit = prepare_region(
            &memory,
            &[spec(0x1000, bytes.len())],
            CompileLimits::default(),
        )
        .unwrap();
        let entries =
            prepare_entry_region(&memory, &[GuestAddress(0x1000)], CompileLimits::default())
                .unwrap();
        assert_eq!(
            (explicit.block_count(), explicit.instruction_count()),
            (1, 33)
        );
        assert_eq!(
            (entries.block_count(), entries.instruction_count()),
            (1, 33)
        );
        let explicit = compile_region(
            &memory,
            &[spec(0x1000, bytes.len())],
            CompileLimits::default(),
        )
        .unwrap();
        let entries =
            compile_entry_region(&memory, &[GuestAddress(0x1000)], CompileLimits::default())
                .unwrap();
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
}

fn embedded(bytes: &[u8]) -> EngineInstance {
    let mut engine = EngineInstance::new(1, 0xabcd_ef01_1234_5678).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    engine.arena_mut().unwrap()[140..140 + bytes.len()].copy_from_slice(bytes);
    engine.upload(0x1000, bytes.len() as u32).unwrap();
    engine.arena_mut().unwrap().fill(0xa5);
    engine.protect(0x1000, 1, 4).unwrap();
    engine
}

#[test]
fn embedded_explicit_and_entry_forms_preserve_execute_only_code_and_full_arena() {
    for destination in 0..8 {
        let bytes = destination_block(destination);
        let mut explicit = embedded(&bytes);
        explicit.arena_mut().unwrap()[140..148].copy_from_slice(&[
            0,
            0x10,
            0,
            0,
            bytes.len() as u8,
            0,
            0,
            0,
        ]);
        let arena = explicit.arena().to_vec();
        let snapshot = explicit
            .memory()
            .unwrap()
            .snapshot_code(GuestAddress(0x1000), bytes.len())
            .unwrap();
        assert_eq!(explicit.compile(1), Ok(1), "destination {destination}");
        assert_eq!(explicit.arena(), arena);
        assert!(explicit.memory().unwrap().is_code_current(&snapshot));
        let mut entries = embedded(&bytes);
        entries.arena_mut().unwrap()[140..144].copy_from_slice(&[0, 0x10, 0, 0]);
        let arena = entries.arena().to_vec();
        assert_eq!(
            entries.compile_entries(1, 0),
            Ok(1),
            "destination {destination}"
        );
        assert_eq!(entries.arena(), arena);
        assert_eq!(
            explicit.artifact_bytes().unwrap(),
            entries.artifact_bytes().unwrap()
        );
        for engine in [explicit, entries] {
            assert_eq!(engine.guard(0xabcd_ef01_1234_5678, 1), Ok(()));
            assert_eq!(engine.memory().unwrap().mapped_pages(), 1);
            assert!(
                engine
                    .memory()
                    .unwrap()
                    .resolve(GuestAddress(0xa5a5_a5a5), Access::Read)
                    .is_err()
            );
            let mut output = vec![0; bytes.len()];
            engine
                .memory()
                .unwrap()
                .fetch(GuestAddress(0x1000), &mut output)
                .unwrap();
            assert_eq!(output, bytes);
        }
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
        let snapshot = engine
            .memory()
            .unwrap()
            .snapshot_code(GuestAddress(0x1000), bytes.len())
            .unwrap();
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
        assert!(engine.memory().unwrap().is_code_current(&snapshot));
    }
}

#[test]
fn standalone_memory_source_extensions_remain_backend_excluded_before_any_guest_data_read() {
    for instruction in [
        &[0x0f, 0xb6, 0x03][..],
        &[0x0f, 0xb7, 0x03][..],
        &[0x0f, 0xbe, 0x03][..],
        &[0x0f, 0xbf, 0x03][..],
        &[0x0f, 0xb6, 0x04, 0x24][..],
        &[0x0f, 0xbf, 0x05, 0xff, 0xff, 0xff, 0xff][..],
    ] {
        let mut bytes = vec![0x90];
        bytes.extend_from_slice(instruction);
        let memory = code(0x1000, &bytes);
        let expected = Some(CompileError::Instruction {
            pc: GuestAddress(0x1001),
            cause: InstructionError::BackendUnsupported,
        });
        assert_eq!(
            prepare_region(
                &memory,
                &[spec(0x1000, bytes.len())],
                CompileLimits::default()
            )
            .err(),
            expected
        );
        assert_eq!(
            prepare_entry_region(&memory, &[GuestAddress(0x1000)], CompileLimits::default()).err(),
            expected
        );
    }
}

#[test]
fn narrow_destinations_and_prefixes_keep_exact_decoder_categories() {
    for instruction in [
        &[0x66, 0x0f, 0xb6, 0xc0][..],
        &[0x66, 0x0f, 0xb7, 0xc0][..],
        &[0x66, 0x0f, 0xbe, 0xc0][..],
        &[0x66, 0x0f, 0xbf, 0xc0][..],
        &[0x67, 0x0f, 0xb6, 0xc0][..],
        &[0xf3, 0x0f, 0xb6, 0xc0][..],
        &[0xf2, 0x0f, 0xbf, 0xc0][..],
        &[0xd0, 0x20][..],
    ] {
        rejected(
            instruction,
            InstructionError::Decode(DecodeError::Unsupported(UnsupportedFeature::Opcode)),
        );
    }
    rejected(
        &[0x64, 0x0f, 0xbe, 0xc4],
        InstructionError::Decode(DecodeError::Unsupported(UnsupportedFeature::Segment)),
    );
    rejected(
        &[0xf0, 0x0f, 0xb6, 0xc0],
        InstructionError::Decode(DecodeError::InvalidEncoding),
    );
    rejected(
        &[0xf0, 0x0f, 0xbf, 0x03],
        InstructionError::Decode(DecodeError::InvalidEncoding),
    );
}

#[test]
fn sequential_extensions_charge_global_and_lower_caps_before_next_decode() {
    let mut bytes = [0x0f, 0xb6, 0xc4].repeat(63);
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
    let memory = code(0x1000, &[0x0f, 0xbf, 0xe4].repeat(65));
    assert_eq!(
        prepare_region(&memory, &[spec(0x1000, 195)], CompileLimits::default()).err(),
        Some(CompileError::InstructionLimit)
    );
    assert_eq!(
        prepare_entry_region(&memory, &[GuestAddress(0x1000)], CompileLimits::default()).err(),
        Some(CompileError::InstructionLimit)
    );
    let memory = code(0x1000, &[0x0f, 0xb6, 0xc4, 0xf4]);
    let limits = CompileLimits {
        instructions: 1,
        ..CompileLimits::default()
    };
    assert_eq!(
        prepare_region(&memory, &[spec(0x1000, 4)], limits).err(),
        Some(CompileError::InstructionLimit)
    );
    assert_eq!(
        prepare_entry_region(&memory, &[GuestAddress(0x1000)], limits).err(),
        Some(CompileError::InstructionLimit)
    );
    assert_eq!(
        compile_region(
            &memory,
            &[spec(0x1000, 3)],
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
fn exact_instruction_ends_and_truncated_fetches_preserve_execute_fault_spans() {
    for (opcode, _, _) in FORMS {
        let pc = u32::MAX - 2;
        let memory = code(pc, &[0x0f, opcode, 0xe4]);
        let limits = CompileLimits {
            instructions: 1,
            ..CompileLimits::default()
        };
        let explicit = compile_region(&memory, &[spec(pc, 3)], limits).unwrap();
        let entries = compile_entry_region(&memory, &[GuestAddress(pc)], limits).unwrap();
        assert_eq!(entries.metadata().instructions, 1);
        assert_eq!(
            explicit.wasm_bytes(&memory).unwrap(),
            entries.wasm_bytes(&memory).unwrap()
        );
    }
    let memory = code(0x1000, &[0x0f, 0xb6, 0xc4]);
    assert_eq!(
        prepare_region(&memory, &[spec(0x1000, 2)], CompileLimits::default()).err(),
        Some(CompileError::Instruction {
            pc: GuestAddress(0x1000),
            cause: InstructionError::InvalidBlockEnd,
        })
    );
    for (pc, address, reason) in [
        (0x1ffe, 0x2000, FaultReason::Unmapped),
        (u32::MAX - 1, u32::MAX - 1, FaultReason::AddressOverflow),
    ] {
        let memory = code(pc, &[0x0f, 0xb6]);
        let expected = CompileError::Instruction {
            pc: GuestAddress(pc),
            cause: InstructionError::Decode(DecodeError::MemoryFault {
                pc: GuestAddress(pc),
                fault: MemoryFault {
                    address: GuestAddress(address),
                    access: Access::Execute,
                    reason,
                },
                length: 3,
            }),
        };
        assert_eq!(
            prepare_entry_region(&memory, &[GuestAddress(pc)], CompileLimits::default()).err(),
            Some(expected)
        );
        if pc == 0x1ffe {
            assert_eq!(
                prepare_region(&memory, &[spec(pc, 3)], CompileLimits::default()).err(),
                Some(expected)
            );
        }
    }
    let mut memory = code(0x1ffe, &[0x0f, 0xbe, 0xc4, 0xeb, 0]);
    memory
        .protect(
            PageRange::new(GuestAddress(0x2000), 1).unwrap(),
            Permissions::READ,
        )
        .unwrap();
    assert_eq!(
        prepare_entry_region(&memory, &[GuestAddress(0x1ffe)], CompileLimits::default()).err(),
        Some(CompileError::Instruction {
            pc: GuestAddress(0x1ffe),
            cause: InstructionError::Decode(DecodeError::MemoryFault {
                pc: GuestAddress(0x1ffe),
                fault: MemoryFault {
                    address: GuestAddress(0x2000),
                    access: Access::Execute,
                    reason: FaultReason::Permission
                },
                length: 3,
            }),
        })
    );
}

#[test]
fn extension_snapshots_cover_only_consumed_pages_and_compilation_preserves_stamps() {
    let mut memory = code(0x1ffd, &[0x0f, 0xb7, 0xe4]);
    let artifact = compile_region(&memory, &[spec(0x1ffd, 3)], CompileLimits::default()).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x2000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory.write(GuestAddress(0x2000), &[0x90]).unwrap();
    artifact.wasm_bytes(&memory).unwrap();
    for changed_page in [0x1000, 0x2000] {
        let mut memory = code(0x1ffe, &[0x0f, 0xbf, 0xe4, 0xeb, 0]);
        let prepared =
            prepare_entry_region(&memory, &[GuestAddress(0x1ffe)], CompileLimits::default())
                .unwrap();
        let artifact =
            compile_entry_region(&memory, &[GuestAddress(0x1ffe)], CompileLimits::default())
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
