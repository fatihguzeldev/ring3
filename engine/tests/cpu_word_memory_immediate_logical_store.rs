use ring3_engine::{
    abi::arena::TRANSFER_OFFSET,
    cpu::{
        UnsupportedFeature,
        dbt::{
            BlockSpec, CompileError, CompileLimits, InstructionError, RegistryError,
            compile_entry_region, compile_region, prepare_entry_region, prepare_region,
        },
        x86::{
            Register32,
            decode::{DecodeError, decode_one},
            ir::{EffectiveAddress, Operation, WordLogicalKind, WordValue},
        },
    },
    memory::{
        Access, AddressSpace, FaultReason, GuestAddress, MemoryFault, PageRange, Permissions,
    },
    process::{EngineInstance, HostError},
};

const CODE: u32 = 0x1000;
const KEY: u64 = 0x574d_494c_5354_4f31;
const KINDS: [(u8, WordLogicalKind); 3] = [
    (4, WordLogicalKind::And),
    (1, WordLogicalKind::Or),
    (6, WordLogicalKind::Xor),
];

fn encoding(opcode: u8, extension: u8) -> Vec<u8> {
    let mut bytes = vec![0x66, opcode, 0x03 | extension << 3, 0x80];
    if opcode == 0x81 {
        bytes.push(0x92);
    }
    bytes
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

fn decode_admission(opcode: u8, extension: u8, kind: WordLogicalKind) {
    let bytes = encoding(opcode, extension);
    let engine = code(&bytes);
    let before = engine.arena().to_vec();
    let decoded = decode_one(engine.memory().unwrap(), GuestAddress(CODE))
        .expect("WORD memory-destination immediate logic must decode without accessing data");
    let Operation::MemoryLogicalWord {
        kind: actual_kind,
        address,
        source,
    } = decoded.operation()
    else {
        panic!("WORD immediate store decoded as {:?}", decoded.operation());
    };
    assert_eq!(*actual_kind, kind);
    assert_eq!(
        *source,
        WordValue::Immediate(if opcode == 0x81 { 0x9280 } else { 0xff80 })
    );
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
        (decoded.length() as usize, decoded.next_pc()),
        (bytes.len(), GuestAddress(CODE + bytes.len() as u32))
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
fn word_memory_and_imm16_decodes_without_accessing_data() {
    decode_admission(0x81, 4, WordLogicalKind::And);
}

#[test]
fn word_memory_and_sign_extended_imm8_decodes_without_accessing_data() {
    decode_admission(0x83, 4, WordLogicalKind::And);
}

#[test]
fn word_memory_or_imm16_decodes_without_accessing_data() {
    decode_admission(0x81, 1, WordLogicalKind::Or);
}

#[test]
fn word_memory_or_sign_extended_imm8_decodes_without_accessing_data() {
    decode_admission(0x83, 1, WordLogicalKind::Or);
}

#[test]
fn word_memory_xor_imm16_decodes_without_accessing_data() {
    decode_admission(0x81, 6, WordLogicalKind::Xor);
}

#[test]
fn word_memory_xor_sign_extended_imm8_decodes_without_accessing_data() {
    decode_admission(0x83, 6, WordLogicalKind::Xor);
}

fn bound_admission(resident: bool, entries: bool) {
    for opcode in [0x81, 0x83] {
        for (extension, _) in KINDS {
            let mut bytes = encoding(opcode, extension);
            bytes.extend([0xeb, 0]);
            assert_bound_admission(&bytes, resident, entries);
        }
    }
}

fn assert_bound_admission(bytes: &[u8], resident: bool, entries: bool) {
    let mut engine = code(bytes);
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
    .expect("WORD memory-destination immediate logic must compile bound");
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
fn word_memory_immediate_logical_store_compiles_replacement_explicit() {
    bound_admission(false, false);
}

#[test]
fn word_memory_immediate_logical_store_compiles_replacement_entry() {
    bound_admission(false, true);
}

#[test]
fn word_memory_immediate_logical_store_compiles_resident_explicit() {
    bound_admission(true, false);
}

#[test]
fn word_memory_immediate_logical_store_compiles_resident_entry() {
    bound_admission(true, true);
}

#[test]
fn word_memory_immediate_logical_store_mixed_64_instruction_block_fits_every_bound_profile() {
    let mut bytes = Vec::new();
    for index in 0..32 {
        let opcode = if index % 2 == 0 { 0x81 } else { 0x83 };
        bytes.extend(encoding(opcode, KINDS[index % KINDS.len()].0));
    }
    bytes.extend(std::iter::repeat_n(0x90, 31));
    bytes.extend([0xeb, 0]);
    for resident in [false, true] {
        for entries in [false, true] {
            assert_bound_admission(&bytes, resident, entries);
        }
    }
}

fn instruction(opcode: u8, extension: u8, tail: &[u8], immediate: u16) -> Vec<u8> {
    let mut bytes = vec![0x66, opcode, tail[0] | extension << 3];
    bytes.extend_from_slice(&tail[1..]);
    bytes.push(immediate as u8);
    if opcode == 0x81 {
        bytes.push((immediate >> 8) as u8);
    }
    bytes
}

fn code_space(pc: u32, bytes: &[u8]) -> AddressSpace {
    let pages = (u64::from(pc & 0xfff) + bytes.len() as u64).div_ceil(4096) as u32;
    let mut memory = AddressSpace::new(pages).unwrap();
    let range = PageRange::new(GuestAddress(pc & !0xfff), pages).unwrap();
    memory.map_zeroed(range, Permissions::ALL).unwrap();
    memory.write(GuestAddress(pc), bytes).unwrap();
    memory.protect(range, Permissions::EXECUTE).unwrap();
    memory
}

fn addresses() -> [(&'static [u8], EffectiveAddress); 13] {
    use Register32::{Ebp, Ebx, Ecx, Esi, Esp};
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
            address(Some(Esp), Some(Esi), 8, 0xf3f0_6766),
        ),
    ]
}

fn assert_operation(
    bytes: &[u8],
    kind: WordLogicalKind,
    immediate: u16,
    address: EffectiveAddress,
) {
    let memory = code_space(CODE, bytes);
    let decoded = decode_one(&memory, GuestAddress(CODE)).unwrap();
    assert_eq!(
        decoded.operation(),
        &Operation::MemoryLogicalWord {
            kind,
            address,
            source: WordValue::Immediate(immediate),
        },
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
fn word_memory_immediate_logical_store_address_classes_have_exact_ir() {
    let mut rows = 0;
    for (extension, kind) in KINDS {
        for (opcode, immediate) in [(0x81, 0xf0f3), (0x83, 0xff80)] {
            for (tail, address) in addresses() {
                assert_operation(
                    &instruction(opcode, extension, tail, immediate),
                    kind,
                    immediate,
                    address,
                );
                rows += 1;
            }
        }
    }
    assert_eq!(rows, 78);
}

#[test]
fn word_memory_immediate_logical_store_normalizes_values_and_prefix_looking_payloads() {
    let address = addresses()[0].1;
    for (extension, kind) in KINDS {
        for immediate in [0, 1, 0x7fff, 0x8000, 0xffff, 0x6766, 0xf0f3] {
            assert_operation(
                &instruction(0x81, extension, &[0x03], immediate),
                kind,
                immediate,
                address,
            );
        }
        for (encoded, normalized) in [
            (0, 0),
            (1, 1),
            (0x7f, 0x7f),
            (0x80, 0xff80),
            (0xff, 0xffff),
            (0x66, 0x66),
            (0x67, 0x67),
            (0xf0, 0xfff0),
            (0xf3, 0xfff3),
        ] {
            assert_operation(
                &instruction(0x83, extension, &[0x03], encoded),
                kind,
                normalized,
                address,
            );
        }
    }
}

#[test]
fn word_memory_immediate_logical_store_prefixes_stay_closed_and_group1_neighbors_stay_open() {
    let unsupported = DecodeError::Unsupported(UnsupportedFeature::Opcode);
    for opcode in [0x81, 0x83] {
        for (extension, _) in KINDS {
            let bytes = instruction(opcode, extension, &[0x03], 0xffff);
            for prefix in [0x66, 0x67, 0xf0, 0xf2, 0xf3] {
                let prefixed = [vec![prefix], bytes.clone()].concat();
                assert_eq!(
                    decode_one(&code_space(CODE, &prefixed), GuestAddress(CODE)).err(),
                    Some(unsupported),
                    "{prefixed:02x?}"
                );
            }
            for prefix in [0x26, 0x2e, 0x36, 0x3e, 0x64, 0x65] {
                let prefixed = [vec![prefix], bytes.clone()].concat();
                assert_eq!(
                    decode_one(&code_space(CODE, &prefixed), GuestAddress(CODE)).err(),
                    Some(DecodeError::Unsupported(UnsupportedFeature::Segment)),
                    "{prefixed:02x?}"
                );
            }
        }
        for extension in [0, 2, 3, 5, 7] {
            let bytes = instruction(opcode, extension, &[0x03], 0xffff);
            let memory = code_space(CODE, &bytes);
            let decoded = decode_one(&memory, GuestAddress(CODE)).unwrap();
            if extension == 7 {
                assert!(matches!(
                    decoded.operation(),
                    Operation::MemoryCompareWord { .. }
                ));
            } else {
                assert!(matches!(
                    decoded.operation(),
                    Operation::MemoryArithmeticWord { .. }
                ));
            }
        }
    }
    for opcode in [0x80, 0x82] {
        for (extension, _) in KINDS {
            let bytes = instruction(opcode, extension, &[0x03], 0xff);
            assert_eq!(
                decode_one(&code_space(CODE, &bytes), GuestAddress(CODE)).err(),
                Some(unsupported),
                "{bytes:02x?}"
            );
        }
    }
}

#[test]
fn decoded_word_memory_immediate_logical_stores_stay_closed_in_standalone_backend() {
    for opcode in [0x81, 0x83] {
        for (extension, _) in KINDS {
            let mut bytes = encoding(opcode, extension);
            bytes.extend([0xeb, 0]);
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
                prepare_entry_region(&memory, &[GuestAddress(CODE)], CompileLimits::default())
                    .err(),
                expected
            );
            assert_eq!(
                compile_entry_region(&memory, &[GuestAddress(CODE)], CompileLimits::default())
                    .err(),
                expected
            );
        }
    }
}

fn fetch_error(pc: u32, address: u32, reason: FaultReason, length: usize) -> DecodeError {
    DecodeError::MemoryFault {
        pc: GuestAddress(pc),
        fault: MemoryFault {
            address: GuestAddress(address),
            access: Access::Execute,
            reason,
        },
        length: length as u32,
    }
}

#[test]
fn word_memory_immediate_logical_store_fetch_checks_displacement_and_each_immediate_byte() {
    for opcode in [0x81, 0x83] {
        for (extension, _) in KINDS {
            let bytes = instruction(
                opcode,
                extension,
                &[0x84, 0xf4, 0x66, 0x67, 0xf0, 0xf3],
                0x6766,
            );
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
                assert!(memory.is_code_current(decoded.code_snapshot()));
            }
            for cut in 1..bytes.len() {
                let pc = 0x2000 - cut as u32;
                let memory = code_space(pc, &bytes[..cut]);
                assert_eq!(
                    decode_one(&memory, GuestAddress(pc)).err(),
                    Some(fetch_error(pc, 0x2000, FaultReason::Unmapped, cut + 1)),
                    "{opcode:02x}/{extension} cut={cut}"
                );
                let top = u32::MAX - cut as u32 + 1;
                let memory = code_space(top, &bytes[..cut]);
                assert_eq!(
                    decode_one(&memory, GuestAddress(top)).err(),
                    Some(fetch_error(top, top, FaultReason::AddressOverflow, cut + 1))
                );
            }
            let pc = 0x2000 - bytes.len() as u32 + 1;
            let mut memory = code_space(pc, &bytes);
            memory
                .protect(
                    PageRange::new(GuestAddress(0x2000), 1).unwrap(),
                    Permissions::READ,
                )
                .unwrap();
            assert_eq!(
                decode_one(&memory, GuestAddress(pc)).err(),
                Some(fetch_error(
                    pc,
                    0x2000,
                    FaultReason::Permission,
                    bytes.len()
                ))
            );
        }
    }
}

#[test]
fn word_memory_immediate_logical_store_late_refusal_preserves_arena_and_both_owners() {
    let mut bytes = Vec::new();
    for opcode in [0x81, 0x83] {
        for (extension, _) in KINDS {
            bytes.extend(encoding(opcode, extension));
        }
    }
    let refused_pc = CODE + bytes.len() as u32;
    bytes.extend([0x0f, 0x0b]);
    let refused_length = bytes.len() as u32;
    bytes.resize(256, 0x90);
    bytes.extend([0xeb, 0]);
    let mut engine = code(&bytes);
    let descriptor = &mut engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8];
    descriptor[..4].copy_from_slice(&(CODE + 256).to_le_bytes());
    descriptor[4..].copy_from_slice(&2_u32.to_le_bytes());
    let replacement = engine.compile(1).unwrap();
    let resident = engine.compile_resident(1).unwrap().get();
    let replacement_bytes = engine.artifact_bytes().unwrap().to_vec();
    let replacement_pointer = engine.artifact_bytes().unwrap().as_ptr();
    let resident_bytes = engine.resident_bytes(resident).unwrap().to_vec();
    let resident_pointer = engine.resident_bytes(resident).unwrap().as_ptr();
    let error = CompileError::Instruction {
        pc: GuestAddress(refused_pc),
        cause: InstructionError::Decode(DecodeError::Unsupported(UnsupportedFeature::Opcode)),
    };
    for is_resident in [false, true] {
        for entries in [false, true] {
            let descriptor = &mut engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8];
            descriptor[..4].copy_from_slice(&CODE.to_le_bytes());
            descriptor[4..]
                .copy_from_slice(&if entries { 0 } else { refused_length }.to_le_bytes());
            let before = engine.arena().to_vec();
            let result = match (is_resident, entries) {
                (false, false) => engine.compile(1).map(u64::from),
                (false, true) => engine.compile_entries(1, 0).map(u64::from),
                (true, false) => engine.compile_resident(1).map(|id| id.get()),
                (true, true) => engine.compile_resident_entries(1, 0).map(|id| id.get()),
            };
            assert_eq!(
                result,
                Err(if is_resident {
                    HostError::Resident(RegistryError::Compile(error))
                } else {
                    HostError::Compile(error)
                })
            );
            assert_eq!(engine.arena(), before);
            assert_eq!(engine.artifact_bytes().unwrap(), replacement_bytes);
            assert_eq!(
                engine.artifact_bytes().unwrap().as_ptr(),
                replacement_pointer
            );
            assert_eq!(engine.resident_bytes(resident).unwrap(), resident_bytes);
            assert_eq!(
                engine.resident_bytes(resident).unwrap().as_ptr(),
                resident_pointer
            );
            engine.guard(KEY, replacement).unwrap();
            engine.guard_resident(KEY, resident).unwrap();
        }
    }
}
