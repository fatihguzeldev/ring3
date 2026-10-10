use ring3_engine::cpu::dbt::{
    BlockSpec, CompileError, CompileLimits, InstructionError, compile_entry_region, compile_region,
    prepare_entry_region, prepare_region,
};
use ring3_engine::cpu::{
    UnsupportedFeature,
    x86::{
        Register32,
        decode::{DecodeError, decode_one},
        ir::{BinaryKind, EffectiveAddress, Location32, Operation, Value32},
    },
};
use ring3_engine::memory::{Access, AddressSpace, GuestAddress, PageRange, Permissions};
use ring3_engine::process::{EngineInstance, HostError};

const KEY: u64 = 0x1234_5678_9abc_def0;
const FORMS: [(u8, BinaryKind); 6] = [
    (0x03, BinaryKind::Add),
    (0x2b, BinaryKind::Sub),
    (0x3b, BinaryKind::Cmp),
    (0x23, BinaryKind::And),
    (0x0b, BinaryKind::Or),
    (0x33, BinaryKind::Xor),
];
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

fn engine(bytes: &[u8]) -> EngineInstance {
    let mut instance = EngineInstance::new(1, KEY).unwrap();
    instance.map(0x1000, 1, 7).unwrap();
    instance.arena_mut().unwrap()[140..140 + bytes.len()].copy_from_slice(bytes);
    instance.upload(0x1000, bytes.len() as u32).unwrap();
    instance
}

#[test]
fn authored_memory_add_is_admitted_by_embedded_explicit_and_entry_compile() {
    // add eax,[ebx]; jmp next.
    let bytes = [0x03, 0x03, 0xeb, 0];
    let mut explicit = engine(&bytes);
    explicit.arena_mut().unwrap()[140..148].copy_from_slice(&[0, 0x10, 0, 0, 4, 0, 0, 0]);
    assert_eq!(explicit.compile(1), Ok(1));
    let mut entries = engine(&bytes);
    entries.arena_mut().unwrap()[140..144].copy_from_slice(&[0, 0x10, 0, 0]);
    assert_eq!(entries.compile_entries(1, 0), Ok(1));
    assert_eq!(
        explicit.artifact_bytes().unwrap(),
        entries.artifact_bytes().unwrap()
    );
}

fn describe(instance: &mut EngineInstance, pc: u32, length: usize) {
    instance.arena_mut().unwrap()[140..144].copy_from_slice(&pc.to_le_bytes());
    instance.arena_mut().unwrap()[144..148].copy_from_slice(&(length as u32).to_le_bytes());
}

fn compile(instance: &mut EngineInstance, entries: bool) -> Result<u32, HostError> {
    if entries {
        instance.compile_entries(1, 0)
    } else {
        instance.compile(1)
    }
}

fn admit(bytes: &[u8]) {
    let mut artifacts = Vec::new();
    for entries in [false, true] {
        let mut instance = engine(bytes);
        instance.protect(0x1000, 1, 4).unwrap();
        instance.arena_mut().unwrap().fill(0xa5);
        describe(&mut instance, 0x1000, bytes.len());
        let arena = instance.arena().to_vec();
        let snapshot = instance
            .memory()
            .unwrap()
            .snapshot_code(GuestAddress(0x1000), bytes.len())
            .unwrap();
        assert_eq!(compile(&mut instance, entries), Ok(1), "{bytes:02x?}");
        assert_eq!(instance.arena(), arena);
        assert!(instance.memory().unwrap().is_code_current(&snapshot));
        assert_eq!(instance.guard(KEY, 1), Ok(()));
        assert_eq!(instance.memory().unwrap().mapped_pages(), 1);
        assert!(
            instance
                .memory()
                .unwrap()
                .resolve(GuestAddress(0xa5a5_a5a5), Access::Read)
                .is_err()
        );
        let mut output = vec![0; bytes.len()];
        instance
            .memory()
            .unwrap()
            .fetch(GuestAddress(0x1000), &mut output)
            .unwrap();
        assert_eq!(output, bytes);
        let artifact = instance.artifact_bytes().unwrap();
        assert!(artifact.windows(6).any(|part| part == b"read32"));
        for excluded in [b"store32".as_slice(), b"read8", b"read16"] {
            assert!(
                !artifact
                    .windows(excluded.len())
                    .any(|part| part == excluded)
            );
        }
        artifacts.push(artifact.to_vec());
    }
    assert_eq!(artifacts[0], artifacts[1]);
}

fn check_decoded(bytes: &[u8], expected: &[(u8, Operation)]) {
    let instance = engine(bytes);
    let mut pc = 0x1000;
    for (length, operation) in expected {
        let decoded = decode_one(instance.memory().unwrap(), GuestAddress(pc)).unwrap();
        assert_eq!(decoded.length(), *length);
        assert_eq!(decoded.operation(), operation);
        pc += u32::from(*length);
    }
}

#[test]
fn six_forms_and_all_destinations_preserve_base_and_index_aliases_without_data_reads() {
    for (index, destination) in REGISTERS.into_iter().enumerate() {
        let register = index as u8;
        let mut bytes = Vec::new();
        let mut expected = Vec::new();
        for (opcode, kind) in FORMS {
            let base_tail = if destination == Register32::Esp {
                vec![0x44 | register << 3, 0x24, 0xe0]
            } else {
                vec![0x40 | register << 3 | register, 0xe0]
            };
            let index_register = if destination == Register32::Esp {
                Register32::Ecx
            } else {
                destination
            };
            let index_bits = if destination == Register32::Esp {
                1
            } else {
                register
            };
            for (tail, address) in [
                (
                    base_tail,
                    EffectiveAddress {
                        base: Some(destination),
                        index: None,
                        scale: 1,
                        displacement: (-32_i32) as u32,
                    },
                ),
                (
                    vec![
                        0x04 | register << 3,
                        0x85 | index_bits << 3,
                        0xf8,
                        0xff,
                        0xff,
                        0xff,
                    ],
                    EffectiveAddress {
                        base: None,
                        index: Some(index_register),
                        scale: 4,
                        displacement: (-8_i32) as u32,
                    },
                ),
            ] {
                bytes.push(opcode);
                bytes.extend_from_slice(&tail);
                expected.push((
                    tail.len() as u8 + 1,
                    Operation::Binary {
                        kind,
                        destination: Location32::Register(destination),
                        source: Value32::Memory(address),
                    },
                ));
            }
        }
        bytes.extend_from_slice(&[0xeb, 0]);
        check_decoded(&bytes, &expected);
        admit(&bytes);
    }
}

#[test]
fn effective_addresses_keep_scales_base_index_and_signed_displacements() {
    let cases: &[(&[u8], EffectiveAddress)] = &[
        (
            &[0x03],
            EffectiveAddress {
                base: Some(Register32::Ebx),
                index: None,
                scale: 1,
                displacement: 0,
            },
        ),
        (
            &[0x45, 0],
            EffectiveAddress {
                base: Some(Register32::Ebp),
                index: None,
                scale: 1,
                displacement: 0,
            },
        ),
        (
            &[0x04, 0x24],
            EffectiveAddress {
                base: Some(Register32::Esp),
                index: None,
                scale: 1,
                displacement: 0,
            },
        ),
        (
            &[0x04, 0x0b],
            EffectiveAddress {
                base: Some(Register32::Ebx),
                index: Some(Register32::Ecx),
                scale: 1,
                displacement: 0,
            },
        ),
        (
            &[0x04, 0x4b],
            EffectiveAddress {
                base: Some(Register32::Ebx),
                index: Some(Register32::Ecx),
                scale: 2,
                displacement: 0,
            },
        ),
        (
            &[0x04, 0x8b],
            EffectiveAddress {
                base: Some(Register32::Ebx),
                index: Some(Register32::Ecx),
                scale: 4,
                displacement: 0,
            },
        ),
        (
            &[0x04, 0xcb],
            EffectiveAddress {
                base: Some(Register32::Ebx),
                index: Some(Register32::Ecx),
                scale: 8,
                displacement: 0,
            },
        ),
        (
            &[0x04, 0x8d, 0xf8, 0xff, 0xff, 0xff],
            EffectiveAddress {
                base: None,
                index: Some(Register32::Ecx),
                scale: 4,
                displacement: (-8_i32) as u32,
            },
        ),
        (
            &[0x05, 0xff, 0xff, 0xff, 0xff],
            EffectiveAddress {
                base: None,
                index: None,
                scale: 1,
                displacement: u32::MAX,
            },
        ),
        (
            &[0x43, 0x80],
            EffectiveAddress {
                base: Some(Register32::Ebx),
                index: None,
                scale: 1,
                displacement: (-128_i32) as u32,
            },
        ),
        (
            &[0x83, 0x78, 0x56, 0x34, 0x92],
            EffectiveAddress {
                base: Some(Register32::Ebx),
                index: None,
                scale: 1,
                displacement: 0x9234_5678,
            },
        ),
    ];
    for (opcode, kind) in FORMS {
        let mut bytes = Vec::new();
        let mut expected = Vec::new();
        for (tail, address) in cases {
            bytes.push(opcode);
            bytes.extend_from_slice(tail);
            expected.push((
                tail.len() as u8 + 1,
                Operation::Binary {
                    kind,
                    destination: Location32::Register(Register32::Eax),
                    source: Value32::Memory(*address),
                },
            ));
        }
        bytes.extend_from_slice(&[0xeb, 0]);
        check_decoded(&bytes, &expected);
        admit(&bytes);
    }
}

fn standalone_code(bytes: &[u8]) -> AddressSpace {
    let mut memory = AddressSpace::new(1).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory.write(GuestAddress(0x1000), bytes).unwrap();
    memory
}

fn standalone_rejected(bytes: &[u8], expected: CompileError) {
    let memory = standalone_code(bytes);
    let specs = [BlockSpec {
        entry: GuestAddress(0x1000),
        byte_length: bytes.len() as u32,
    }];
    assert_eq!(
        prepare_region(&memory, &specs, CompileLimits::default()).err(),
        Some(expected),
        "{bytes:02x?}"
    );
    assert_eq!(
        compile_region(&memory, &specs, CompileLimits::default()).err(),
        Some(expected),
        "{bytes:02x?}"
    );
    assert_eq!(
        prepare_entry_region(&memory, &[GuestAddress(0x1000)], CompileLimits::default()).err(),
        Some(expected),
        "{bytes:02x?}"
    );
    assert_eq!(
        compile_entry_region(&memory, &[GuestAddress(0x1000)], CompileLimits::default()).err(),
        Some(expected),
        "{bytes:02x?}"
    );
}

fn instruction_error(cause: InstructionError) -> CompileError {
    CompileError::Instruction {
        pc: GuestAddress(0x1001),
        cause,
    }
}

fn embedded_rejected(bytes: &[u8], expected: CompileError) {
    for entries in [false, true] {
        let mut instance = engine(bytes);
        describe(&mut instance, 0x1000, bytes.len());
        let before = instance.arena().to_vec();
        assert_eq!(
            compile(&mut instance, entries),
            Err(HostError::Compile(expected)),
            "{bytes:02x?}"
        );
        assert_eq!(instance.arena(), before);
        assert_eq!(instance.generation(), 0);
        assert_eq!(instance.artifact_bytes(), Err(HostError::InvalidArtifact));
    }
}

#[test]
fn standalone_memory_sources_stay_excluded_in_all_four_apis_before_later_poison() {
    for (opcode, _) in FORMS {
        standalone_rejected(
            &[0x90, opcode, 0x03, 0xf4],
            instruction_error(InstructionError::BackendUnsupported),
        );
    }
}

#[test]
fn standalone_writing_binary_memory_destinations_stay_excluded() {
    let mut instructions = Vec::new();
    for opcode in [0x01, 0x29, 0x21, 0x09, 0x31] {
        for source in 0..8 {
            instructions.push(vec![opcode, 0x03 | source << 3]);
        }
    }
    for extension in [0, 5, 4, 1, 6] {
        instructions.push(vec![0x81, 0x03 | extension << 3, 1, 0, 0, 0]);
        instructions.push(vec![0x83, 0x03 | extension << 3, 0x80]);
    }
    for instruction in instructions {
        let mut bytes = vec![0x90];
        bytes.extend_from_slice(&instruction);
        bytes.push(0xf4);
        let expected = instruction_error(InstructionError::BackendUnsupported);
        standalone_rejected(&bytes, expected);
    }
}

#[test]
fn prefixes_small_width_and_adjacent_exclusions_keep_precise_errors() {
    for (instruction, cause) in [
        (
            &[0x66, 0x66, 0x03, 0x03][..],
            InstructionError::Decode(DecodeError::Unsupported(UnsupportedFeature::Opcode)),
        ),
        (
            &[0x67, 0x2b, 0x03][..],
            InstructionError::Decode(DecodeError::Unsupported(UnsupportedFeature::Opcode)),
        ),
        (
            &[0xf3, 0x23, 0x03][..],
            InstructionError::Decode(DecodeError::Unsupported(UnsupportedFeature::Opcode)),
        ),
        (
            &[0xf0, 0x33, 0x03][..],
            InstructionError::Decode(DecodeError::InvalidEncoding),
        ),
        (
            &[0x64, 0x3b, 0x03][..],
            InstructionError::Decode(DecodeError::Unsupported(UnsupportedFeature::Segment)),
        ),
        (
            &[0x66, 0x02, 0x03][..],
            InstructionError::Decode(DecodeError::Unsupported(UnsupportedFeature::Opcode)),
        ),
        (
            &[0x0f, 0x06][..],
            InstructionError::Decode(DecodeError::Unsupported(UnsupportedFeature::Privileged)),
        ),
    ] {
        let mut bytes = vec![0x90];
        bytes.extend_from_slice(instruction);
        embedded_rejected(&bytes, instruction_error(cause));
    }
    for (opcode, _) in FORMS {
        let mut instance = engine(&[0x90, opcode, 0x83, 0x78, 0x56, 0x34, 0x92]);
        describe(&mut instance, 0x1000, 6);
        let before = instance.arena().to_vec();
        assert_eq!(
            instance.compile(1),
            Err(HostError::Compile(instruction_error(
                InstructionError::InvalidBlockEnd
            )))
        );
        assert_eq!(instance.arena(), before);
        assert_eq!(instance.generation(), 0);
    }
}

#[test]
fn binary_reads_stay_sequential_and_charge_the_cap_before_later_poison() {
    let mut bytes = [0x03, 0x03].repeat(63);
    bytes.extend_from_slice(&[0xeb, 0]);
    admit(&bytes);
    for bytes in [[0x03, 0x03].repeat(65), {
        let mut bytes = [0x03, 0x03].repeat(64);
        bytes.push(0xf4);
        bytes
    }] {
        embedded_rejected(&bytes, CompileError::InstructionLimit);
    }
}

#[test]
fn binary_ea_snapshots_cover_both_code_pages_and_ignore_unrelated_data() {
    for entries in [false, true] {
        for changed_page in [0x1000, 0x2000] {
            let bytes = [0x03, 0x84, 0x8b, 0x78, 0x56, 0x34, 0x92, 0xeb, 0];
            let mut instance = EngineInstance::new(3, KEY).unwrap();
            instance.map(0x1000, 2, 7).unwrap();
            instance.map(0x4000, 1, 3).unwrap();
            instance.arena_mut().unwrap()[140..149].copy_from_slice(&bytes);
            instance.upload(0x1ffd, bytes.len() as u32).unwrap();
            describe(&mut instance, 0x1ffd, bytes.len());
            assert_eq!(compile(&mut instance, entries), Ok(1));
            instance.write32(0x4000, 0x1122_3344).unwrap();
            instance.protect(0x4000, 1, 1).unwrap();
            assert_eq!(instance.guard(KEY, 1), Ok(()));
            instance.protect(changed_page, 1, 7).unwrap();
            assert_eq!(instance.guard(KEY, 1), Err(HostError::CodeInvalidated));
            assert_eq!(instance.artifact_bytes(), Err(HostError::CodeInvalidated));
            assert_eq!(instance.generation(), 1);
        }
    }
}
