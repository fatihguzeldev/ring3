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
const FORMS: [(u8, u8, BinaryKind, Value32, &[u8]); 5] = [
    (
        0x39,
        0,
        BinaryKind::Cmp,
        Value32::Register(Register32::Eax),
        &[],
    ),
    (
        0x81,
        7,
        BinaryKind::Cmp,
        Value32::Immediate(0x9234_5678),
        &[0x78, 0x56, 0x34, 0x92],
    ),
    (
        0x83,
        7,
        BinaryKind::Cmp,
        Value32::Immediate(0xffff_ff80),
        &[0x80],
    ),
    (
        0x85,
        0,
        BinaryKind::Test,
        Value32::Register(Register32::Eax),
        &[],
    ),
    (
        0xf7,
        0,
        BinaryKind::Test,
        Value32::Immediate(0x9234_5678),
        &[0x78, 0x56, 0x34, 0x92],
    ),
];

fn engine(bytes: &[u8]) -> EngineInstance {
    let mut instance = EngineInstance::new(1, KEY).unwrap();
    instance.map(0x1000, 1, 7).unwrap();
    instance.arena_mut().unwrap()[140..140 + bytes.len()].copy_from_slice(bytes);
    instance.upload(0x1000, bytes.len() as u32).unwrap();
    instance
}

#[test]
fn authored_memory_cmp_is_admitted_by_embedded_explicit_and_entry_compile() {
    // cmp [ebx],eax; jmp next.
    let bytes = [0x39, 0x03, 0xeb, 0];
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
fn both_register_forms_keep_all_eight_sources_and_base_index_esp_aliases() {
    for (index, source) in REGISTERS.into_iter().enumerate() {
        let register = index as u8;
        let mut bytes = Vec::new();
        let mut expected = Vec::new();
        for (opcode, kind) in [(0x39, BinaryKind::Cmp), (0x85, BinaryKind::Test)] {
            let base_tail = if source == Register32::Esp {
                vec![0x44 | register << 3, 0x24, 0xe0]
            } else {
                vec![0x40 | register << 3 | register, 0xe0]
            };
            let index_register = if source == Register32::Esp {
                Register32::Ecx
            } else {
                source
            };
            let index_bits = if source == Register32::Esp {
                1
            } else {
                register
            };
            for (tail, address) in [
                (
                    base_tail,
                    EffectiveAddress {
                        base: Some(source),
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
                        destination: Location32::Memory(address),
                        source: Value32::Register(source),
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
fn five_forms_preserve_ea_scales_base_index_and_signed_displacements() {
    type AddressCase<'a> = (&'a [u8], Option<Register32>, Option<Register32>, u8, u32);
    let cases: &[AddressCase<'_>] = &[
        (&[0x03], Some(Register32::Ebx), None, 1, 0),
        (&[0x45, 0], Some(Register32::Ebp), None, 1, 0),
        (&[0x04, 0x24], Some(Register32::Esp), None, 1, 0),
        (
            &[0x04, 0x0b],
            Some(Register32::Ebx),
            Some(Register32::Ecx),
            1,
            0,
        ),
        (
            &[0x04, 0x4b],
            Some(Register32::Ebx),
            Some(Register32::Ecx),
            2,
            0,
        ),
        (
            &[0x04, 0x8b],
            Some(Register32::Ebx),
            Some(Register32::Ecx),
            4,
            0,
        ),
        (
            &[0x04, 0xcb],
            Some(Register32::Ebx),
            Some(Register32::Ecx),
            8,
            0,
        ),
        (
            &[0x04, 0x8d, 0xf8, 0xff, 0xff, 0xff],
            None,
            Some(Register32::Ecx),
            4,
            (-8_i32) as u32,
        ),
        (&[0x05, 0xff, 0xff, 0xff, 0xff], None, None, 1, u32::MAX),
        (
            &[0x43, 0x80],
            Some(Register32::Ebx),
            None,
            1,
            (-128_i32) as u32,
        ),
        (
            &[0x83, 0x78, 0x56, 0x34, 0x92],
            Some(Register32::Ebx),
            None,
            1,
            0x9234_5678,
        ),
    ];
    for (opcode, extension, kind, source, immediate) in FORMS {
        let mut bytes = Vec::new();
        let mut expected = Vec::new();
        for (tail, base, index, scale, displacement) in cases {
            bytes.extend_from_slice(&[opcode, tail[0] | extension << 3]);
            bytes.extend_from_slice(&tail[1..]);
            bytes.extend_from_slice(immediate);
            expected.push((
                (tail.len() + immediate.len() + 1) as u8,
                Operation::Binary {
                    kind,
                    destination: Location32::Memory(EffectiveAddress {
                        base: *base,
                        index: *index,
                        scale: *scale,
                        displacement: *displacement,
                    }),
                    source,
                },
            ));
        }
        bytes.extend_from_slice(&[0xeb, 0]);
        check_decoded(&bytes, &expected);
        admit(&bytes);
    }
}

#[test]
fn cmp_imm8_sign_extends_and_full_immediates_keep_all_thirty_two_bits() {
    let address = EffectiveAddress {
        base: Some(Register32::Ebx),
        index: None,
        scale: 1,
        displacement: 0,
    };
    let mut bytes = Vec::new();
    let mut expected = Vec::new();
    for (encoded, value) in [(0, 0), (0x7f, 0x7f), (0x80, 0xffff_ff80), (0xff, u32::MAX)] {
        bytes.extend_from_slice(&[0x83, 0x3b, encoded]);
        expected.push((
            3,
            Operation::Binary {
                kind: BinaryKind::Cmp,
                destination: Location32::Memory(address),
                source: Value32::Immediate(value),
            },
        ));
    }
    for (opcode, modrm, kind) in [
        (0x81, 0x3b, BinaryKind::Cmp),
        (0xf7, 0x03, BinaryKind::Test),
    ] {
        for value in [0_u32, 0x7fff_ffff, 0x8000_0000, u32::MAX] {
            bytes.extend_from_slice(&[opcode, modrm]);
            bytes.extend_from_slice(&value.to_le_bytes());
            expected.push((
                6,
                Operation::Binary {
                    kind,
                    destination: Location32::Memory(address),
                    source: Value32::Immediate(value),
                },
            ));
        }
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
fn standalone_read_only_memory_forms_remain_excluded_before_later_poison() {
    for (opcode, extension, _, _, immediate) in FORMS {
        let mut bytes = vec![0x90, opcode, 0x03 | extension << 3];
        bytes.extend_from_slice(immediate);
        bytes.push(0xf4);
        standalone_rejected(
            &bytes,
            instruction_error(InstructionError::BackendUnsupported),
        );
    }
}

#[test]
fn standalone_writing_binary_memory_forms_remain_excluded() {
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
    }
}

#[test]
fn prefixes_small_width_and_declared_spans_retain_precise_errors() {
    for (instruction, cause) in [
        (
            &[0x66, 0x66, 0x39, 0x03][..],
            InstructionError::Decode(DecodeError::Unsupported(UnsupportedFeature::Opcode)),
        ),
        (
            &[0x67, 0x85, 0x03][..],
            InstructionError::Decode(DecodeError::Unsupported(UnsupportedFeature::Opcode)),
        ),
        (
            &[0xf3, 0x81, 0x3b, 1, 0, 0, 0][..],
            InstructionError::Decode(DecodeError::Unsupported(UnsupportedFeature::Opcode)),
        ),
        (
            &[0xf0, 0x39, 0x03][..],
            InstructionError::Decode(DecodeError::InvalidEncoding),
        ),
        (
            &[0xf0, 0xf7, 0x03, 1, 0, 0, 0][..],
            InstructionError::Decode(DecodeError::InvalidEncoding),
        ),
        (
            &[0x64, 0x83, 0x3b, 0x80][..],
            InstructionError::Decode(DecodeError::Unsupported(UnsupportedFeature::Segment)),
        ),
        (
            &[0x66, 0x38, 0x03][..],
            InstructionError::Decode(DecodeError::Unsupported(UnsupportedFeature::Opcode)),
        ),
        (
            &[0x66, 0x84, 0x03][..],
            InstructionError::Decode(DecodeError::Unsupported(UnsupportedFeature::Opcode)),
        ),
    ] {
        let mut bytes = vec![0x90];
        bytes.extend_from_slice(instruction);
        embedded_rejected(&bytes, instruction_error(cause));
    }
    for (opcode, extension, _, _, immediate) in FORMS {
        let mut bytes = vec![0x90, opcode, 0x83 | extension << 3, 0x78, 0x56, 0x34, 0x92];
        bytes.extend_from_slice(immediate);
        let mut instance = engine(&bytes);
        describe(&mut instance, 0x1000, bytes.len() - 1);
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
fn comparisons_stay_sequential_and_charge_the_cap_before_later_poison() {
    let mut bytes = [0x39, 0x03].repeat(63);
    bytes.extend_from_slice(&[0xeb, 0]);
    admit(&bytes);
    for bytes in [[0x39, 0x03].repeat(65), {
        let mut bytes = [0x39, 0x03].repeat(64);
        bytes.push(0xf4);
        bytes
    }] {
        embedded_rejected(&bytes, CompileError::InstructionLimit);
    }
}

#[test]
fn comparison_ea_snapshots_cover_both_code_pages_and_ignore_unrelated_data() {
    for entries in [false, true] {
        for changed_page in [0x1000, 0x2000] {
            let bytes = [0x39, 0x84, 0x8b, 0x78, 0x56, 0x34, 0x92, 0xeb, 0];
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
