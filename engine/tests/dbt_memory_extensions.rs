use ring3_engine::cpu::dbt::{
    BlockSpec, CompileError, CompileLimits, InstructionError, compile_entry_region, compile_region,
    prepare_entry_region, prepare_region,
};
use ring3_engine::cpu::{
    UnsupportedFeature,
    x86::{
        Register32,
        decode::{DecodeError, decode_one},
        ir::{EffectiveAddress, ExtensionKind, Operation, SmallSource, SmallWidth},
    },
};
use ring3_engine::memory::{Access, AddressSpace, GuestAddress, PageRange, Permissions};
use ring3_engine::process::{EngineInstance, HostError};

const KEY: u64 = 0x1234_5678_9abc_def0;

fn engine(bytes: &[u8]) -> EngineInstance {
    let mut instance = EngineInstance::new(1, KEY).unwrap();
    instance.map(0x1000, 1, 7).unwrap();
    instance.arena_mut().unwrap()[140..140 + bytes.len()].copy_from_slice(bytes);
    instance.upload(0x1000, bytes.len() as u32).unwrap();
    instance
}

#[test]
fn authored_memory_extension_is_admitted_by_embedded_explicit_and_entry_compile() {
    // movzx eax,byte [ebx]; jmp next.
    let bytes = [0x0f, 0xb6, 0x03, 0xeb, 0];
    let mut explicit = engine(&bytes);
    explicit.arena_mut().unwrap()[140..148].copy_from_slice(&[0, 0x10, 0, 0, 5, 0, 0, 0]);
    assert_eq!(explicit.compile(1), Ok(1));
    let mut entries = engine(&bytes);
    entries.arena_mut().unwrap()[140..144].copy_from_slice(&[0, 0x10, 0, 0]);
    assert_eq!(entries.compile_entries(1, 0), Ok(1));
    assert_eq!(
        explicit.artifact_bytes().unwrap(),
        entries.artifact_bytes().unwrap()
    );
}

const FORMS: [(u8, ExtensionKind, SmallWidth); 4] = [
    (0xb6, ExtensionKind::Zero, SmallWidth::Byte),
    (0xb7, ExtensionKind::Zero, SmallWidth::Word),
    (0xbe, ExtensionKind::Sign, SmallWidth::Byte),
    (0xbf, ExtensionKind::Sign, SmallWidth::Word),
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

fn admit(bytes: &[u8]) {
    let mut explicit = engine(bytes);
    explicit.protect(0x1000, 1, 4).unwrap();
    explicit.arena_mut().unwrap().fill(0xa5);
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
    assert_eq!(explicit.compile(1), Ok(1));
    assert_eq!(explicit.arena(), arena);
    assert!(explicit.memory().unwrap().is_code_current(&snapshot));
    let mut entries = engine(bytes);
    entries.protect(0x1000, 1, 4).unwrap();
    entries.arena_mut().unwrap().fill(0xa5);
    entries.arena_mut().unwrap()[140..144].copy_from_slice(&[0, 0x10, 0, 0]);
    let arena = entries.arena().to_vec();
    assert_eq!(entries.compile_entries(1, 0), Ok(1));
    assert_eq!(entries.arena(), arena);
    assert_eq!(
        explicit.artifact_bytes().unwrap(),
        entries.artifact_bytes().unwrap()
    );
    for instance in [explicit, entries] {
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
    }
}

#[test]
fn four_forms_all_destinations_and_ea_aliases_admit_without_reading_guest_data() {
    for (destination_index, destination) in REGISTERS.into_iter().enumerate() {
        let mut bytes = Vec::new();
        let mut expected = Vec::new();
        for (opcode, kind, width) in FORMS {
            for (tail, address) in [
                (
                    vec![0x44 | ((destination_index as u8) << 3), 0x8b, 0xe0],
                    EffectiveAddress {
                        base: Some(Register32::Ebx),
                        index: Some(Register32::Ecx),
                        scale: 4,
                        displacement: (-32_i32) as u32,
                    },
                ),
                (
                    vec![0x04 | ((destination_index as u8) << 3), 0x24],
                    EffectiveAddress {
                        base: Some(Register32::Esp),
                        index: None,
                        scale: 1,
                        displacement: 0,
                    },
                ),
            ] {
                bytes.extend_from_slice(&[0x0f, opcode]);
                bytes.extend_from_slice(&tail);
                expected.push((
                    tail.len() as u8 + 2,
                    Operation::Extend {
                        kind,
                        destination,
                        source: SmallSource::Memory { address, width },
                    },
                ));
            }
        }
        bytes.extend_from_slice(&[0xeb, 0]);
        let instance = engine(&bytes);
        let mut pc = 0x1000;
        for (length, operation) in expected {
            let instruction = decode_one(instance.memory().unwrap(), GuestAddress(pc)).unwrap();
            assert_eq!(instruction.length(), length);
            assert_eq!(*instruction.operation(), operation);
            pc += u32::from(length);
        }
        admit(&bytes);
    }
}

#[test]
fn checked_memory_address_forms_keep_scales_base_index_and_signed_displacements() {
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
                displacement: 0xffff_fff8,
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
                displacement: 0xffff_ff80,
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
    let mut bytes = Vec::new();
    let mut expected = Vec::new();
    for (opcode, kind, width) in FORMS {
        for (tail, address) in cases {
            bytes.extend_from_slice(&[0x0f, opcode]);
            bytes.extend_from_slice(tail);
            expected.push((
                tail.len() as u8 + 2,
                Operation::Extend {
                    kind,
                    destination: Register32::Eax,
                    source: SmallSource::Memory {
                        address: *address,
                        width,
                    },
                },
            ));
        }
    }
    bytes.extend_from_slice(&[0xeb, 0]);
    let instance = engine(&bytes);
    let mut pc = 0x1000;
    for (length, operation) in expected {
        let instruction = decode_one(instance.memory().unwrap(), GuestAddress(pc)).unwrap();
        assert_eq!(*instruction.operation(), operation);
        assert_eq!(instruction.length(), length);
        pc += u32::from(length);
    }
    admit(&bytes);
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

#[test]
fn standalone_memory_conversion_remains_excluded_in_explicit_and_entry_apis() {
    for (opcode, _, _) in FORMS {
        let bytes = [0x90, 0x0f, opcode, 0x03, 0xeb, 0];
        let memory = standalone_code(&bytes);
        let spec = [BlockSpec {
            entry: GuestAddress(0x1000),
            byte_length: 6,
        }];
        let expected = Some(CompileError::Instruction {
            pc: GuestAddress(0x1001),
            cause: InstructionError::BackendUnsupported,
        });
        assert_eq!(
            prepare_region(&memory, &spec, CompileLimits::default()).err(),
            expected
        );
        assert_eq!(
            compile_region(&memory, &spec, CompileLimits::default()).err(),
            expected
        );
        assert_eq!(
            prepare_entry_region(&memory, &[GuestAddress(0x1000)], CompileLimits::default()).err(),
            expected
        );
        assert_eq!(
            compile_entry_region(&memory, &[GuestAddress(0x1000)], CompileLimits::default()).err(),
            expected
        );
    }
}

#[test]
fn embedded_small_width_prefix_and_adjacent_errors_remain_precise() {
    for (instruction, cause) in [
        (
            &[0x66, 0x0f, 0xb6, 0x03][..],
            InstructionError::Decode(DecodeError::Unsupported(UnsupportedFeature::Opcode)),
        ),
        (
            &[0x67, 0x0f, 0xbf, 0x03][..],
            InstructionError::Decode(DecodeError::Unsupported(UnsupportedFeature::Opcode)),
        ),
        (
            &[0xf3, 0x0f, 0xbe, 0x03][..],
            InstructionError::Decode(DecodeError::Unsupported(UnsupportedFeature::Opcode)),
        ),
        (
            &[0xf0, 0x0f, 0xb7, 0x03][..],
            InstructionError::Decode(DecodeError::InvalidEncoding),
        ),
        (
            &[0x64, 0x0f, 0xb6, 0x03][..],
            InstructionError::Decode(DecodeError::Unsupported(UnsupportedFeature::Segment)),
        ),
        (
            &[0x66, 0xc0, 0x20, 2][..],
            InstructionError::Decode(DecodeError::Unsupported(UnsupportedFeature::Opcode)),
        ),
        (
            &[0x0f, 0x06][..],
            InstructionError::Decode(DecodeError::Unsupported(UnsupportedFeature::Privileged)),
        ),
    ] {
        let mut bytes = vec![0x90];
        bytes.extend_from_slice(instruction);
        for entry_format in [false, true] {
            let mut instance = engine(&bytes);
            instance.arena_mut().unwrap()[140..148].copy_from_slice(&[
                0,
                0x10,
                0,
                0,
                bytes.len() as u8,
                0,
                0,
                0,
            ]);
            let before = instance.arena().to_vec();
            let result = if entry_format {
                instance.compile_entries(1, 0)
            } else {
                instance.compile(1)
            };
            assert_eq!(
                result,
                Err(HostError::Compile(CompileError::Instruction {
                    pc: GuestAddress(0x1001),
                    cause
                }))
            );
            assert_eq!(instance.arena(), before);
            assert_eq!(instance.generation(), 0);
        }
    }
}

#[test]
fn embedded_extensions_remain_sequential_and_charge_instruction_limit_before_poison() {
    let mut bytes = [0x0f, 0xb6, 0x03].repeat(63);
    bytes.extend_from_slice(&[0xeb, 0]);
    admit(&bytes);
    for bytes in [[0x0f, 0xb6, 0x03].repeat(65), {
        let mut bytes = [0x0f, 0xb6, 0x03].repeat(64);
        bytes.push(0xf4);
        bytes
    }] {
        for entry_format in [false, true] {
            let mut instance = engine(&bytes);
            instance.arena_mut().unwrap()[140..148].copy_from_slice(&[
                0,
                0x10,
                0,
                0,
                bytes.len() as u8,
                0,
                0,
                0,
            ]);
            let before = instance.arena().to_vec();
            let result = if entry_format {
                instance.compile_entries(1, 0)
            } else {
                instance.compile(1)
            };
            assert_eq!(
                result,
                Err(HostError::Compile(CompileError::InstructionLimit))
            );
            assert_eq!(instance.arena(), before);
        }
    }
}
