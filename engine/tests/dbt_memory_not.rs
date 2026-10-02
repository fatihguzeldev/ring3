use ring3_engine::cpu::dbt::{
    BlockSpec, CompileError, CompileLimits, InstructionError, compile_entry_region, compile_region,
    prepare_entry_region, prepare_region,
};
use ring3_engine::cpu::{
    UnsupportedFeature,
    x86::{
        Register32,
        decode::{DecodeError, decode_one},
        ir::{EffectiveAddress, Location32, Operation, UnaryKind},
    },
};
use ring3_engine::memory::{Access, AddressSpace, GuestAddress, PageRange, Permissions};
use ring3_engine::process::{EngineInstance, HostError};

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

fn engine(bytes: &[u8]) -> EngineInstance {
    let mut instance = EngineInstance::new(1, KEY).unwrap();
    instance.map(0x1000, 1, 7).unwrap();
    instance.arena_mut().unwrap()[140..140 + bytes.len()].copy_from_slice(bytes);
    instance.upload(0x1000, bytes.len() as u32).unwrap();
    instance
}

#[test]
fn authored_memory_not_is_admitted_by_embedded_explicit_and_entry_compile() {
    // not dword [ebx]; jmp next.
    let bytes = [0xf7, 0x13, 0xeb, 0];
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
        assert!(
            instance
                .memory()
                .unwrap()
                .resolve(GuestAddress(0xa5a5_a5a5), Access::Write)
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
        for required in [b"read32".as_slice(), b"store32"] {
            assert!(
                artifact
                    .windows(required.len())
                    .any(|part| part == required)
            );
        }
        for excluded in [b"read8".as_slice(), b"read16"] {
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

fn check_decoded(bytes: &[u8], expected: &[(u8, EffectiveAddress)]) {
    let instance = engine(bytes);
    let mut pc = 0x1000;
    for (length, address) in expected {
        let decoded = decode_one(instance.memory().unwrap(), GuestAddress(pc)).unwrap();
        assert_eq!(decoded.length(), *length);
        assert_eq!(
            *decoded.operation(),
            Operation::Unary {
                kind: UnaryKind::Not,
                destination: Location32::Memory(*address)
            }
        );
        pc += u32::from(*length);
    }
}

#[test]
fn all_register_bases_and_encodable_indices_keep_esp_no_index_identity() {
    let mut bytes = Vec::new();
    let mut expected = Vec::new();
    for (index, register) in REGISTERS.into_iter().enumerate() {
        let bits = index as u8;
        let base_tail = if register == Register32::Esp {
            vec![0x54, 0x24, 0xe0]
        } else {
            vec![0x50 | bits, 0xe0]
        };
        let (index_tail, index_address) = if register == Register32::Esp {
            (
                vec![0x14, 0x25, 0xf8, 0xff, 0xff, 0xff],
                EffectiveAddress {
                    base: None,
                    index: None,
                    scale: 1,
                    displacement: (-8_i32) as u32,
                },
            )
        } else {
            (
                vec![0x14, 0x85 | bits << 3, 0xf8, 0xff, 0xff, 0xff],
                EffectiveAddress {
                    base: None,
                    index: Some(register),
                    scale: 4,
                    displacement: (-8_i32) as u32,
                },
            )
        };
        for (tail, address) in [
            (
                base_tail,
                EffectiveAddress {
                    base: Some(register),
                    index: None,
                    scale: 1,
                    displacement: (-32_i32) as u32,
                },
            ),
            (index_tail, index_address),
        ] {
            bytes.push(0xf7);
            bytes.extend_from_slice(&tail);
            expected.push((tail.len() as u8 + 1, address));
        }
    }
    bytes.extend_from_slice(&[0xeb, 0]);
    check_decoded(&bytes, &expected);
    admit(&bytes);
}

#[test]
fn sib_scales_signed_displacements_and_absolute_top_words_compile_without_operand_access() {
    let address = |base, index, scale, displacement| EffectiveAddress {
        base,
        index,
        scale,
        displacement,
    };
    let cases: &[(&[u8], EffectiveAddress)] = &[
        (&[0x13], address(Some(Register32::Ebx), None, 1, 0)),
        (&[0x55, 0], address(Some(Register32::Ebp), None, 1, 0)),
        (&[0x14, 0x24], address(Some(Register32::Esp), None, 1, 0)),
        (
            &[0x14, 0x0b],
            address(Some(Register32::Ebx), Some(Register32::Ecx), 1, 0),
        ),
        (
            &[0x14, 0x4b],
            address(Some(Register32::Ebx), Some(Register32::Ecx), 2, 0),
        ),
        (
            &[0x14, 0x8b],
            address(Some(Register32::Ebx), Some(Register32::Ecx), 4, 0),
        ),
        (
            &[0x14, 0xcb],
            address(Some(Register32::Ebx), Some(Register32::Ecx), 8, 0),
        ),
        (
            &[0x14, 0x8d, 0xf8, 0xff, 0xff, 0xff],
            address(None, Some(Register32::Ecx), 4, (-8_i32) as u32),
        ),
        (
            &[0x15, 0xfc, 0xff, 0xff, 0xff],
            address(None, None, 1, 0xffff_fffc),
        ),
        (
            &[0x15, 0xff, 0xff, 0xff, 0xff],
            address(None, None, 1, u32::MAX),
        ),
        (
            &[0x53, 0x80],
            address(Some(Register32::Ebx), None, 1, (-128_i32) as u32),
        ),
        (
            &[0x93, 0x78, 0x56, 0x34, 0x92],
            address(Some(Register32::Ebx), None, 1, 0x9234_5678),
        ),
    ];
    let mut bytes = Vec::new();
    let mut expected = Vec::new();
    for (tail, address) in cases {
        bytes.push(0xf7);
        bytes.extend_from_slice(tail);
        expected.push((tail.len() as u8 + 1, *address));
    }
    bytes.extend_from_slice(&[0xeb, 0]);
    check_decoded(&bytes, &expected);
    admit(&bytes);
}

fn instruction_error(cause: InstructionError) -> CompileError {
    CompileError::Instruction {
        pc: GuestAddress(0x1001),
        cause,
    }
}

fn standalone_rejected(bytes: &[u8], expected: CompileError) {
    let mut memory = AddressSpace::new(1).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory.write(GuestAddress(0x1000), bytes).unwrap();
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
fn standalone_memory_not_remains_excluded_in_all_four_apis_before_later_poison() {
    for instruction in [
        &[0xf7, 0x13][..],
        &[0xf7, 0x14, 0x24][..],
        &[0xf7, 0x94, 0x8b, 0x78, 0x56, 0x34, 0x92][..],
    ] {
        let mut bytes = vec![0x90];
        bytes.extend_from_slice(instruction);
        bytes.push(0xf4);
        standalone_rejected(
            &bytes,
            instruction_error(InstructionError::BackendUnsupported),
        );
    }
}

#[test]
fn writing_binary_memory_forms_remain_excluded() {
    for instruction in [
        &[0x01, 0x03][..],
        &[0x29, 0x03][..],
        &[0x21, 0x03][..],
        &[0x09, 0x03][..],
        &[0x31, 0x03][..],
        &[0x81, 0x03, 1, 0, 0, 0][..],
        &[0x83, 0x2b, 0x80][..],
    ] {
        let mut bytes = vec![0x90];
        bytes.extend_from_slice(instruction);
        bytes.push(0xf4);
        let expected = instruction_error(InstructionError::BackendUnsupported);
        standalone_rejected(&bytes, expected);
        embedded_rejected(&bytes, expected);
    }
}

#[test]
fn narrow_prefix_and_declared_span_errors_remain_precise() {
    for (instruction, cause) in [
        (
            &[0xf6, 0x13][..],
            InstructionError::Decode(DecodeError::Unsupported(UnsupportedFeature::Opcode)),
        ),
        (
            &[0x66, 0xf7, 0x13][..],
            InstructionError::Decode(DecodeError::Unsupported(UnsupportedFeature::Opcode)),
        ),
        (
            &[0x67, 0xf7, 0x13][..],
            InstructionError::Decode(DecodeError::Unsupported(UnsupportedFeature::Opcode)),
        ),
        (
            &[0xf3, 0xf7, 0x13][..],
            InstructionError::Decode(DecodeError::Unsupported(UnsupportedFeature::Opcode)),
        ),
        (
            &[0xf0, 0xf7, 0x13][..],
            InstructionError::Decode(DecodeError::Unsupported(UnsupportedFeature::Opcode)),
        ),
        (
            &[0x64, 0xf7, 0x13][..],
            InstructionError::Decode(DecodeError::Unsupported(UnsupportedFeature::Segment)),
        ),
    ] {
        let mut bytes = vec![0x90];
        bytes.extend_from_slice(instruction);
        embedded_rejected(&bytes, instruction_error(cause));
    }
    let mut instance = engine(&[0x90, 0xf7, 0x94, 0x8b, 0x78, 0x56, 0x34, 0x92]);
    describe(&mut instance, 0x1000, 7);
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

#[test]
fn not_is_sequential_and_charges_the_instruction_cap_before_later_poison() {
    let mut bytes = vec![0x90; 62];
    bytes.extend_from_slice(&[0xf7, 0x13, 0xeb, 0]);
    admit(&bytes);
    for bytes in [[0xf7, 0x13].repeat(65), {
        let mut bytes = vec![0x90; 63];
        bytes.extend_from_slice(&[0xf7, 0x13, 0xf4]);
        bytes
    }] {
        embedded_rejected(&bytes, CompileError::InstructionLimit);
    }
}

#[test]
fn not_ea_snapshots_cover_both_code_pages_and_ignore_unrelated_data() {
    for entries in [false, true] {
        for changed_page in [0x1000, 0x2000] {
            let bytes = [0xf7, 0x94, 0x8b, 0x78, 0x56, 0x34, 0x92, 0xeb, 0];
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
