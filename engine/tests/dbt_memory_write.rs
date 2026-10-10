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
const FORMS: [(u8, u8, BinaryKind, Value32, &[u8]); 15] = [
    (
        0x01,
        0,
        BinaryKind::Add,
        Value32::Register(Register32::Eax),
        &[],
    ),
    (
        0x81,
        0,
        BinaryKind::Add,
        Value32::Immediate(0x9234_5678),
        &[0x78, 0x56, 0x34, 0x92],
    ),
    (
        0x83,
        0,
        BinaryKind::Add,
        Value32::Immediate(0xffff_ff80),
        &[0x80],
    ),
    (
        0x29,
        0,
        BinaryKind::Sub,
        Value32::Register(Register32::Eax),
        &[],
    ),
    (
        0x81,
        5,
        BinaryKind::Sub,
        Value32::Immediate(0x9234_5678),
        &[0x78, 0x56, 0x34, 0x92],
    ),
    (
        0x83,
        5,
        BinaryKind::Sub,
        Value32::Immediate(0xffff_ff80),
        &[0x80],
    ),
    (
        0x21,
        0,
        BinaryKind::And,
        Value32::Register(Register32::Eax),
        &[],
    ),
    (
        0x81,
        4,
        BinaryKind::And,
        Value32::Immediate(0x9234_5678),
        &[0x78, 0x56, 0x34, 0x92],
    ),
    (
        0x83,
        4,
        BinaryKind::And,
        Value32::Immediate(0xffff_ff80),
        &[0x80],
    ),
    (
        0x09,
        0,
        BinaryKind::Or,
        Value32::Register(Register32::Eax),
        &[],
    ),
    (
        0x81,
        1,
        BinaryKind::Or,
        Value32::Immediate(0x9234_5678),
        &[0x78, 0x56, 0x34, 0x92],
    ),
    (
        0x83,
        1,
        BinaryKind::Or,
        Value32::Immediate(0xffff_ff80),
        &[0x80],
    ),
    (
        0x31,
        0,
        BinaryKind::Xor,
        Value32::Register(Register32::Eax),
        &[],
    ),
    (
        0x81,
        6,
        BinaryKind::Xor,
        Value32::Immediate(0x9234_5678),
        &[0x78, 0x56, 0x34, 0x92],
    ),
    (
        0x83,
        6,
        BinaryKind::Xor,
        Value32::Immediate(0xffff_ff80),
        &[0x80],
    ),
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
fn authored_memory_destination_add_is_admitted_by_embedded_explicit_and_entry_compile() {
    // add dword [ebx],eax; jmp next.
    let bytes = [0x01, 0x03, 0xeb, 0];
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

fn encode(opcode: u8, field: u8, tail: &[u8], immediate: &[u8]) -> Vec<u8> {
    let mut bytes = vec![opcode, tail[0] | field << 3];
    bytes.extend_from_slice(&tail[1..]);
    bytes.extend_from_slice(immediate);
    bytes
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
        for access in [Access::Read, Access::Write] {
            assert!(
                instance
                    .memory()
                    .unwrap()
                    .resolve(GuestAddress(0xa5a5_a5a5), access)
                    .is_err()
            );
        }
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
fn all_fifteen_forms_preserve_every_source_base_and_index_alias_including_esp() {
    for (opcode, extension, kind, original_source, immediate) in FORMS {
        let mut bytes = Vec::new();
        let mut expected = Vec::new();
        for (index, register) in REGISTERS.into_iter().enumerate() {
            let bits = index as u8;
            let (field, source) = if matches!(original_source, Value32::Register(_)) {
                (bits, Value32::Register(register))
            } else {
                (extension, original_source)
            };
            let base_tail = if register == Register32::Esp {
                vec![0x44, 0x24, 0xe0]
            } else {
                vec![0x40 | bits, 0xe0]
            };
            let (index_tail, index_address) = if register == Register32::Esp {
                (
                    vec![0x04, 0x25, 0xf8, 0xff, 0xff, 0xff],
                    EffectiveAddress {
                        base: None,
                        index: None,
                        scale: 1,
                        displacement: (-8_i32) as u32,
                    },
                )
            } else {
                (
                    vec![0x04, 0x85 | bits << 3, 0xf8, 0xff, 0xff, 0xff],
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
                let instruction = encode(opcode, field, &tail, immediate);
                expected.push((
                    instruction.len() as u8,
                    Operation::Binary {
                        kind,
                        destination: Location32::Memory(address),
                        source,
                    },
                ));
                bytes.extend_from_slice(&instruction);
            }
        }
        bytes.extend_from_slice(&[0xeb, 0]);
        check_decoded(&bytes, &expected);
        admit(&bytes);
    }
}

#[test]
fn all_sign_extended_immediate_forms_decode_zero_and_both_sign_boundaries() {
    for (_, extension, kind, _, _) in FORMS.into_iter().filter(|form| form.0 == 0x83) {
        let mut bytes = Vec::new();
        let mut expected = Vec::new();
        for (encoded, value) in [(0, 0), (0x7f, 127), (0x80, 0xffff_ff80), (0xff, u32::MAX)] {
            let instruction = encode(0x83, extension, &[0x44, 0x8b, 0x80], &[encoded]);
            expected.push((
                instruction.len() as u8,
                Operation::Binary {
                    kind,
                    destination: Location32::Memory(EffectiveAddress {
                        base: Some(Register32::Ebx),
                        index: Some(Register32::Ecx),
                        scale: 4,
                        displacement: (-128_i32) as u32,
                    }),
                    source: Value32::Immediate(value),
                },
            ));
            bytes.extend_from_slice(&instruction);
        }
        bytes.extend_from_slice(&[0xeb, 0]);
        check_decoded(&bytes, &expected);
        admit(&bytes);
    }
}

#[test]
fn all_fifteen_forms_keep_sib_scales_signed_displacements_and_absolute_top_words() {
    let address = |base, index, scale, displacement| EffectiveAddress {
        base,
        index,
        scale,
        displacement,
    };
    let cases: &[(&[u8], EffectiveAddress)] = &[
        (&[0x03], address(Some(Register32::Ebx), None, 1, 0)),
        (&[0x45, 0], address(Some(Register32::Ebp), None, 1, 0)),
        (&[0x04, 0x24], address(Some(Register32::Esp), None, 1, 0)),
        (
            &[0x04, 0x0b],
            address(Some(Register32::Ebx), Some(Register32::Ecx), 1, 0),
        ),
        (
            &[0x04, 0x4b],
            address(Some(Register32::Ebx), Some(Register32::Ecx), 2, 0),
        ),
        (
            &[0x04, 0x8b],
            address(Some(Register32::Ebx), Some(Register32::Ecx), 4, 0),
        ),
        (
            &[0x04, 0xcb],
            address(Some(Register32::Ebx), Some(Register32::Ecx), 8, 0),
        ),
        (
            &[0x04, 0x8d, 0xf8, 0xff, 0xff, 0xff],
            address(None, Some(Register32::Ecx), 4, (-8_i32) as u32),
        ),
        (
            &[0x05, 0xfc, 0xff, 0xff, 0xff],
            address(None, None, 1, 0xffff_fffc),
        ),
        (
            &[0x05, 0xff, 0xff, 0xff, 0xff],
            address(None, None, 1, u32::MAX),
        ),
        (
            &[0x43, 0x80],
            address(Some(Register32::Ebx), None, 1, (-128_i32) as u32),
        ),
        (
            &[0x83, 0x78, 0x56, 0x34, 0x92],
            address(Some(Register32::Ebx), None, 1, 0x9234_5678),
        ),
    ];
    for (opcode, extension, kind, source, immediate) in FORMS {
        let mut bytes = Vec::new();
        let mut expected = Vec::new();
        for (tail, address) in cases {
            let instruction = encode(opcode, extension, tail, immediate);
            expected.push((
                instruction.len() as u8,
                Operation::Binary {
                    kind,
                    destination: Location32::Memory(*address),
                    source,
                },
            ));
            bytes.extend_from_slice(&instruction);
        }
        bytes.extend_from_slice(&[0xeb, 0]);
        check_decoded(&bytes, &expected);
        admit(&bytes);
    }
}

fn instruction_error(pc: u32, cause: InstructionError) -> CompileError {
    CompileError::Instruction {
        pc: GuestAddress(pc),
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
    for actual in [
        prepare_region(&memory, &specs, CompileLimits::default()).err(),
        compile_region(&memory, &specs, CompileLimits::default()).err(),
        prepare_entry_region(&memory, &[GuestAddress(0x1000)], CompileLimits::default()).err(),
        compile_entry_region(&memory, &[GuestAddress(0x1000)], CompileLimits::default()).err(),
    ] {
        assert_eq!(actual, Some(expected), "{bytes:02x?}");
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
fn standalone_all_fifteen_memory_writes_remain_excluded_in_four_apis_before_later_poison() {
    for (opcode, extension, _, _, immediate) in FORMS {
        for tail in [
            &[0x03][..],
            &[0x04, 0x24][..],
            &[0x84, 0x8b, 0x78, 0x56, 0x34, 0x92][..],
        ] {
            let mut bytes = vec![0x90];
            bytes.extend_from_slice(&encode(opcode, extension, tail, immediate));
            bytes.push(0xf4);
            standalone_rejected(
                &bytes,
                instruction_error(0x1001, InstructionError::BackendUnsupported),
            );
        }
    }
}

#[test]
fn narrow_prefix_and_declared_span_errors_remain_precise() {
    let opcode_error =
        InstructionError::Decode(DecodeError::Unsupported(UnsupportedFeature::Opcode));
    for (opcode, extension, kind, _, immediate) in FORMS {
        if immediate.is_empty() {
            let mut bytes = vec![0x90];
            if matches!(
                kind,
                BinaryKind::Add
                    | BinaryKind::Sub
                    | BinaryKind::And
                    | BinaryKind::Or
                    | BinaryKind::Xor
            ) {
                bytes.push(0x66);
            }
            bytes.extend_from_slice(&[opcode - 1, 0x03]);
            embedded_rejected(&bytes, instruction_error(0x1001, opcode_error));
        } else if opcode == 0x83 {
            let mut bytes = vec![0x90];
            if matches!(
                kind,
                BinaryKind::Add
                    | BinaryKind::Sub
                    | BinaryKind::And
                    | BinaryKind::Or
                    | BinaryKind::Xor
            ) {
                bytes.push(0x66);
            }
            bytes.extend_from_slice(&[0x80, 0x03 | extension << 3, 0x80]);
            embedded_rejected(&bytes, instruction_error(0x1001, opcode_error));
        }
        for prefix in [0x66, 0x67, 0xf3, 0xf0, 0x64] {
            let mut bytes = vec![0x90, prefix];
            if prefix == 0x66 && matches!(opcode, 0x21 | 0x09 | 0x31) {
                bytes.push(0x66);
            }
            bytes.extend_from_slice(&encode(opcode, extension, &[0x03], immediate));
            let cause = if prefix == 0x64 {
                InstructionError::Decode(DecodeError::Unsupported(UnsupportedFeature::Segment))
            } else {
                opcode_error
            };
            embedded_rejected(&bytes, instruction_error(0x1001, cause));
        }
        let mut bytes = vec![0x90];
        bytes.extend_from_slice(&encode(
            opcode,
            extension,
            &[0x84, 0x8b, 0x78, 0x56, 0x34, 0x92],
            immediate,
        ));
        let mut instance = engine(&bytes);
        describe(&mut instance, 0x1000, bytes.len() - 1);
        let before = instance.arena().to_vec();
        assert_eq!(
            instance.compile(1),
            Err(HostError::Compile(instruction_error(
                0x1001,
                InstructionError::InvalidBlockEnd
            )))
        );
        assert_eq!(instance.arena(), before);
        assert_eq!(instance.generation(), 0);
    }
}

#[test]
fn writes_are_sequential_and_charge_the_instruction_cap_before_later_poison() {
    for (opcode, extension, _, _, immediate) in FORMS {
        let instruction = encode(opcode, extension, &[0x03], immediate);
        let mut bytes = vec![0x90; 62];
        bytes.extend_from_slice(&instruction);
        bytes.extend_from_slice(&[0xeb, 0]);
        admit(&bytes);
        for bytes in [instruction.repeat(65), {
            let mut bytes = vec![0x90; 63];
            bytes.extend_from_slice(&instruction);
            bytes.push(0xf4);
            bytes
        }] {
            embedded_rejected(&bytes, CompileError::InstructionLimit);
        }
    }
}

#[test]
fn write_ea_snapshots_cover_both_code_pages_and_ignore_unrelated_data() {
    for (opcode, extension, _, _, immediate) in FORMS {
        for entries in [false, true] {
            for changed_page in [0x1000, 0x2000] {
                let mut bytes = encode(
                    opcode,
                    extension,
                    &[0x84, 0x8b, 0x78, 0x56, 0x34, 0x92],
                    immediate,
                );
                bytes.extend_from_slice(&[0xeb, 0]);
                let mut instance = EngineInstance::new(3, KEY).unwrap();
                instance.map(0x1000, 2, 7).unwrap();
                instance.map(0x4000, 1, 3).unwrap();
                instance.arena_mut().unwrap()[140..140 + bytes.len()].copy_from_slice(&bytes);
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
}

#[test]
fn failed_decode_after_each_writing_form_retains_artifact_arena_and_generation_until_retry() {
    for (opcode, extension, _, _, immediate) in FORMS {
        for entries in [false, true] {
            let instruction = encode(opcode, extension, &[0x03], immediate);
            let mut good = instruction.clone();
            good.extend_from_slice(&[0xeb, 0]);
            let mut instance = EngineInstance::new(2, KEY).unwrap();
            instance.map(0x1000, 1, 7).unwrap();
            instance.map(0x3000, 1, 7).unwrap();
            instance.arena_mut().unwrap()[140..140 + good.len()].copy_from_slice(&good);
            instance.upload(0x1000, good.len() as u32).unwrap();
            describe(&mut instance, 0x1000, good.len());
            assert_eq!(compile(&mut instance, entries), Ok(1));
            let installed = instance.artifact_bytes().unwrap().to_vec();
            let snapshot = instance
                .memory()
                .unwrap()
                .snapshot_code(GuestAddress(0x1000), good.len())
                .unwrap();
            let mut bad = instruction.clone();
            bad.extend_from_slice(&[0x0f, 0x06]);
            instance.arena_mut().unwrap()[140..140 + bad.len()].copy_from_slice(&bad);
            instance.upload(0x3000, bad.len() as u32).unwrap();
            describe(&mut instance, 0x3000, bad.len());
            let arena = instance.arena().to_vec();
            assert_eq!(
                compile(&mut instance, entries),
                Err(HostError::Compile(instruction_error(
                    0x3000 + instruction.len() as u32,
                    InstructionError::Decode(DecodeError::Unsupported(
                        UnsupportedFeature::Privileged
                    ))
                )))
            );
            assert_eq!(instance.arena(), arena);
            assert_eq!(instance.generation(), 1);
            assert_eq!(instance.artifact_bytes().unwrap(), installed);
            assert!(instance.memory().unwrap().is_code_current(&snapshot));
            assert_eq!(instance.guard(KEY, 1), Ok(()));
            let mut output = vec![0; bad.len()];
            instance
                .memory()
                .unwrap()
                .fetch(GuestAddress(0x3000), &mut output)
                .unwrap();
            assert_eq!(output, bad);
            instance.arena_mut().unwrap()[140..140 + good.len()].copy_from_slice(&good);
            instance.upload(0x3000, good.len() as u32).unwrap();
            describe(&mut instance, 0x3000, good.len());
            assert_eq!(compile(&mut instance, entries), Ok(2));
            assert_eq!(instance.guard(KEY, 1), Err(HostError::InvalidArtifact));
            assert_eq!(instance.guard(KEY, 2), Ok(()));
        }
    }
}
