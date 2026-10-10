use ring3_engine::{
    abi::arena::TRANSFER_OFFSET,
    cpu::{
        UnsupportedFeature,
        dbt::{
            BlockSpec, CompileError, CompileLimits, InstructionError, compile_entry_region,
            compile_region, prepare_entry_region, prepare_region,
        },
        x86::{
            Register32,
            decode::{DecodeError, decode_one},
            ir::{EffectiveAddress, Operation, WordMemoryArithmeticKind, WordValue},
        },
    },
    memory::{
        Access, AddressSpace, FaultReason, GuestAddress, MemoryFault, PageRange, Permissions,
    },
    process::EngineInstance,
};

const CODE: u32 = 0x1000;
const KEY: u64 = 0x574d_4341_5253_5431;

fn kind(opcode: u8) -> WordMemoryArithmeticKind {
    match opcode {
        0x11 => WordMemoryArithmeticKind::Adc,
        0x19 => WordMemoryArithmeticKind::Sbb,
        _ => unreachable!(),
    }
}

fn code(bytes: &[u8]) -> EngineInstance {
    let mut engine = EngineInstance::new(1, KEY).unwrap();
    engine.map(CODE, 1, 7).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(bytes);
    engine.upload(CODE, bytes.len() as u32).unwrap();
    engine.protect(CODE, 1, 4).unwrap();
    assert!(
        engine
            .memory()
            .unwrap()
            .resolve(GuestAddress(0), Access::Read)
            .is_err()
    );
    engine
}

fn decode_admission(opcode: u8) {
    let bytes = [0x66, opcode, 0x03];
    let engine = code(&bytes);
    let before = engine.arena().to_vec();
    let decoded = decode_one(engine.memory().unwrap(), GuestAddress(CODE))
        .expect("WORD memory-destination ADC/SBB must decode without accessing data");
    let Operation::MemoryArithmeticWord {
        kind: actual_kind,
        address,
        source,
    } = decoded.operation()
    else {
        panic!("WORD carry store decoded as {:?}", decoded.operation());
    };
    assert_eq!(*actual_kind, kind(opcode));
    assert_eq!(*source, WordValue::Register(Register32::Eax));
    assert_eq!(
        *address,
        EffectiveAddress {
            base: Some(Register32::Ebx),
            index: None,
            scale: 1,
            displacement: 0,
        }
    );
    assert_eq!(
        (decoded.length(), decoded.next_pc()),
        (3, GuestAddress(CODE + 3))
    );
    assert!(
        engine
            .memory()
            .unwrap()
            .is_code_current(decoded.code_snapshot())
    );
    assert_eq!(engine.arena(), before);
    assert_eq!(engine.memory().unwrap().mapped_pages(), 1);
}

#[test]
fn word_memory_adc_register_decodes_without_accessing_data() {
    decode_admission(0x11);
}

#[test]
fn word_memory_sbb_register_decodes_without_accessing_data() {
    decode_admission(0x19);
}

fn bound_admission(resident: bool, entries: bool) {
    for opcode in [0x11, 0x19] {
        bound_instruction_bank(resident, entries, opcode, 1, 0);
    }
}

fn bound_instruction_bank(resident: bool, entries: bool, opcode: u8, count: usize, nops: usize) {
    let mut bytes = [0x66, opcode, 0x03].repeat(count);
    bytes.extend(std::iter::repeat_n(0x90, nops));
    bytes.extend([0xeb, 0]);
    let mut engine = code(&bytes);
    let descriptor = &mut engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8];
    descriptor[..4].copy_from_slice(&CODE.to_le_bytes());
    descriptor[4..]
        .copy_from_slice(&if entries { 0_u32 } else { bytes.len() as u32 }.to_le_bytes());
    let before = engine.arena().to_vec();
    let id = match (resident, entries) {
        (false, false) => engine.compile(1).map(u64::from),
        (false, true) => engine.compile_entries(1, 0).map(u64::from),
        (true, false) => engine.compile_resident(1).map(|id| id.get()),
        (true, true) => engine.compile_resident_entries(1, 0).map(|id| id.get()),
    }
    .expect("WORD memory-destination ADC/SBB must compile bound without accessing data");
    let module = if resident {
        engine.resident_bytes(id).unwrap()
    } else {
        engine.artifact_bytes().unwrap()
    };
    assert_eq!(&module[..8], b"\0asm\x01\0\0\0");
    assert!(module.len() <= CompileLimits::default().wasm_bytes);
    assert_eq!(engine.arena(), before);
    assert_eq!(engine.memory().unwrap().mapped_pages(), 1);
}

#[test]
fn word_memory_carry_store_compiles_replacement_explicit() {
    bound_admission(false, false);
}

#[test]
fn word_memory_carry_store_compiles_replacement_entry() {
    bound_admission(false, true);
}

#[test]
fn word_memory_carry_store_compiles_resident_explicit() {
    bound_admission(true, false);
}

#[test]
fn word_memory_carry_store_compiles_resident_entry() {
    bound_admission(true, true);
}

#[test]
fn word_memory_carry_store_mixed_64_instruction_blocks_fit_every_bound_profile() {
    for opcode in [0x11, 0x19] {
        for resident in [false, true] {
            for entries in [false, true] {
                bound_instruction_bank(resident, entries, opcode, 32, 31);
            }
        }
    }
}

const PARENTS: [Register32; 8] = [
    Register32::Eax,
    Register32::Ecx,
    Register32::Edx,
    Register32::Ebx,
    Register32::Esp,
    Register32::Ebp,
    Register32::Esi,
    Register32::Edi,
];

fn encoding(opcode: u8, source: u8, tail: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0x66, opcode, tail[0] | source << 3];
    bytes.extend_from_slice(&tail[1..]);
    bytes
}

fn code_space(pc: u32, bytes: &[u8]) -> AddressSpace {
    let pages = (u64::from(pc & 0xfff) + bytes.len() as u64).div_ceil(4096) as u32;
    let mut memory = AddressSpace::new(pages).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(pc & !0xfff), pages).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory.write(GuestAddress(pc), bytes).unwrap();
    memory
        .protect(
            PageRange::new(GuestAddress(pc & !0xfff), pages).unwrap(),
            Permissions::EXECUTE,
        )
        .unwrap();
    memory
}

fn address_cases() -> [(&'static [u8], EffectiveAddress); 13] {
    use Register32::{Ebp, Ebx, Ecx, Esp};
    let address = |base, index, scale, displacement| EffectiveAddress {
        base,
        index,
        scale,
        displacement,
    };
    [
        (&[0x03], address(Some(Ebx), None, 1, 0)),
        (&[0x04, 0x24], address(Some(Esp), None, 1, 0)),
        (&[0x45, 0], address(Some(Ebp), None, 1, 0)),
        (&[0x43, 0x80], address(Some(Ebx), None, 1, 0xffff_ff80)),
        (&[0x43, 0x7f], address(Some(Ebx), None, 1, 127)),
        (
            &[0x83, 0x78, 0x56, 0x34, 0x92],
            address(Some(Ebx), None, 1, 0x9234_5678),
        ),
        (&[0x04, 0x0b], address(Some(Ebx), Some(Ecx), 1, 0)),
        (&[0x04, 0x4b], address(Some(Ebx), Some(Ecx), 2, 0)),
        (&[0x04, 0x8b], address(Some(Ebx), Some(Ecx), 4, 0)),
        (&[0x04, 0xcb], address(Some(Ebx), Some(Ecx), 8, 0)),
        (
            &[0x04, 0x8d, 0x78, 0x56, 0x34, 0x92],
            address(None, Some(Ecx), 4, 0x9234_5678),
        ),
        (
            &[0x05, 0x78, 0x56, 0x34, 0x92],
            address(None, None, 1, 0x9234_5678),
        ),
        (
            &[0x84, 0xf4, 0x66, 0x67, 0xf0, 0xf3],
            address(Some(Esp), Some(Register32::Esi), 8, 0xf3f0_6766),
        ),
    ]
}

fn assert_memory_operation(bytes: &[u8], source: Register32, address: EffectiveAddress) {
    let memory = code_space(CODE, bytes);
    let decoded = decode_one(&memory, GuestAddress(CODE)).unwrap();
    let Operation::MemoryArithmeticWord {
        kind: actual_kind,
        address: actual_address,
        source: actual_source,
    } = decoded.operation()
    else {
        panic!("{bytes:02x?}: {:?}", decoded.operation());
    };
    assert_eq!(*actual_kind, kind(bytes[1]), "{bytes:02x?}");
    assert_eq!(
        (*actual_source, *actual_address),
        (WordValue::Register(source), address),
        "{bytes:02x?}"
    );
    assert_eq!(
        (decoded.length() as usize, decoded.next_pc()),
        (bytes.len(), GuestAddress(CODE + bytes.len() as u32))
    );
    assert!(memory.is_code_current(decoded.code_snapshot()));
    assert_eq!(memory.mapped_pages(), 1);
}

#[test]
fn word_memory_carry_store_sources_address_classes_and_payloads_have_exact_ir() {
    let mut rows = 0;
    for opcode in [0x11, 0x19] {
        for (source, parent) in PARENTS.into_iter().enumerate() {
            for (tail, address) in address_cases() {
                assert_memory_operation(&encoding(opcode, source as u8, tail), parent, address);
                rows += 1;
            }
        }
    }
    assert_eq!(rows, 208);
}

#[test]
fn word_memory_carry_store_source_can_alias_every_base_and_encodable_sib_index() {
    let mut rows = 0;
    for opcode in [0x11, 0x19] {
        for (source, parent) in PARENTS.into_iter().enumerate() {
            let tail = match parent {
                Register32::Esp => vec![0x04, 0x24],
                Register32::Ebp => vec![0x45, 0],
                _ => vec![source as u8],
            };
            assert_memory_operation(
                &encoding(opcode, source as u8, &tail),
                parent,
                EffectiveAddress {
                    base: Some(parent),
                    index: None,
                    scale: 1,
                    displacement: 0,
                },
            );
            rows += 1;
            if parent != Register32::Esp {
                let tail = [0x04, 0xc5 | (source as u8) << 3, 0xff, 0xff, 0xff, 0xff];
                assert_memory_operation(
                    &encoding(opcode, source as u8, &tail),
                    parent,
                    EffectiveAddress {
                        base: None,
                        index: Some(parent),
                        scale: 8,
                        displacement: u32::MAX,
                    },
                );
                rows += 1;
            }
        }
    }
    assert_eq!(rows, 30);
}

fn instruction_error(error: DecodeError) -> CompileError {
    CompileError::Instruction {
        pc: GuestAddress(CODE),
        cause: InstructionError::Decode(error),
    }
}

#[test]
fn word_memory_carry_store_only_single_operand_override_and_flat32_are_open() {
    let opcode_error = DecodeError::Unsupported(UnsupportedFeature::Opcode);
    let segment_error = DecodeError::Unsupported(UnsupportedFeature::Segment);
    let mut rows = 0;
    for opcode in [0x11, 0x19] {
        let original = [0x66, opcode, 0x03];
        let mut cases = Vec::new();
        for prefix in [0x66, 0x67, 0xf0, 0xf2, 0xf3] {
            cases.push(([vec![prefix], original.to_vec()].concat(), opcode_error));
        }
        cases.push((vec![0x66, 0x67, opcode, 0x03], opcode_error));
        for prefix in [0x26, 0x2e, 0x36, 0x3e, 0x64, 0x65] {
            cases.push(([vec![prefix], original.to_vec()].concat(), segment_error));
        }
        for (mut bytes, error) in cases {
            bytes.extend([0xeb, 0]);
            let memory = code_space(CODE, &bytes);
            assert_eq!(
                decode_one(&memory, GuestAddress(CODE)).err(),
                Some(error),
                "{bytes:02x?}"
            );
            let specs = [BlockSpec {
                entry: GuestAddress(CODE),
                byte_length: bytes.len() as u32,
            }];
            assert_eq!(
                compile_region(&memory, &specs, CompileLimits::default()).err(),
                Some(instruction_error(error)),
                "{bytes:02x?}"
            );
            assert_eq!(
                compile_entry_region(&memory, &[GuestAddress(CODE)], CompileLimits::default())
                    .err(),
                Some(instruction_error(error)),
                "{bytes:02x?}"
            );
            rows += 1;
        }
    }
    assert_eq!(rows, 24);
}

#[test]
fn word_memory_carry_store_duplicate_prefix_immediates_stay_closed() {
    for extension in [2, 3] {
        for (opcode, immediate) in [(0x81, &[0xff, 0xff][..]), (0x83, &[0x80][..])] {
            let mut bytes = vec![0x66, 0x66, opcode, 0x03 | extension << 3];
            bytes.extend_from_slice(immediate);
            bytes.extend([0xeb, 0]);
            let memory = code_space(CODE, &bytes);
            let error = DecodeError::Unsupported(UnsupportedFeature::Opcode);
            assert_eq!(
                decode_one(&memory, GuestAddress(CODE)).err(),
                Some(error),
                "{bytes:02x?}"
            );
            assert_eq!(
                compile_region(
                    &memory,
                    &[BlockSpec {
                        entry: GuestAddress(CODE),
                        byte_length: bytes.len() as u32
                    }],
                    CompileLimits::default(),
                )
                .err(),
                Some(instruction_error(error))
            );
            assert_eq!(
                compile_entry_region(&memory, &[GuestAddress(CODE)], CompileLimits::default())
                    .err(),
                Some(instruction_error(error))
            );
        }
    }
}

#[test]
fn decoded_word_memory_carry_stores_stay_closed_in_standalone_backend() {
    for opcode in [0x11, 0x19] {
        let bytes = [0x66, opcode, 0x03, 0xeb, 0];
        let memory = code_space(CODE, &bytes);
        decode_one(&memory, GuestAddress(CODE)).unwrap();
        let specs = [BlockSpec {
            entry: GuestAddress(CODE),
            byte_length: bytes.len() as u32,
        }];
        let expected = Some(CompileError::Instruction {
            pc: GuestAddress(CODE),
            cause: InstructionError::BackendUnsupported,
        });
        assert_eq!(
            prepare_region(&memory, &specs, CompileLimits::default()).err(),
            expected
        );
        assert_eq!(
            compile_region(&memory, &specs, CompileLimits::default()).err(),
            expected
        );
        assert_eq!(
            prepare_entry_region(&memory, &[GuestAddress(CODE)], CompileLimits::default()).err(),
            expected
        );
        assert_eq!(
            compile_entry_region(&memory, &[GuestAddress(CODE)], CompileLimits::default()).err(),
            expected
        );
    }
}

#[test]
fn word_memory_carry_store_fetch_is_progressive_and_top_end_wraps_only_next_pc() {
    for opcode in [0x11, 0x19] {
        let bytes = encoding(opcode, 4, &[0x84, 0xf4, 0x66, 0x67, 0xf0, 0xf3]);
        for pc in [0x1fff, u32::MAX - bytes.len() as u32 + 1] {
            let memory = code_space(pc, &bytes);
            let decoded = decode_one(&memory, GuestAddress(pc)).unwrap();
            assert_eq!(
                (decoded.length() as usize, decoded.next_pc()),
                (
                    bytes.len(),
                    GuestAddress(pc.wrapping_add(bytes.len() as u32))
                )
            );
        }
        for cut in 1..bytes.len() {
            let pc = 0x2000 - cut as u32;
            let memory = code_space(pc, &bytes[..cut]);
            assert_eq!(
                decode_one(&memory, GuestAddress(pc)).err(),
                Some(DecodeError::MemoryFault {
                    pc: GuestAddress(pc),
                    fault: MemoryFault {
                        address: GuestAddress(0x2000),
                        access: Access::Execute,
                        reason: FaultReason::Unmapped,
                    },
                    length: cut as u32 + 1,
                })
            );
        }
    }
}
