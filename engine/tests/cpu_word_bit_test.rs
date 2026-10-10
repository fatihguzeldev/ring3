use ring3_engine::{
    abi::arena::TRANSFER_OFFSET,
    cpu::{
        UnsupportedFeature,
        dbt::{
            BlockSpec, CompileError, CompileLimits, InstructionError, RegistryError,
            compile_entry_region, compile_region,
        },
        x86::{
            Register32,
            decode::{DecodeError, decode_one},
            ir::{BitIndex, BitTestKind, Operation},
        },
    },
    memory::{
        Access, AddressSpace, FaultReason, GuestAddress, MemoryFault, PageRange, Permissions,
    },
    process::{EngineInstance, HostError},
};

const CODE: u32 = 0x1000;
const KEY: u64 = 0x5742_4954_3136_3031;
const FORMS: [(u8, u8); 4] = [(0xa3, 4), (0xab, 5), (0xb3, 6), (0xbb, 7)];

fn code(bytes: &[u8]) -> EngineInstance {
    let mut engine = EngineInstance::new(1, KEY).unwrap();
    engine.map(CODE, 1, 7).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(bytes);
    engine.upload(CODE, bytes.len() as u32).unwrap();
    engine.protect(CODE, 1, 4).unwrap();
    engine
}

fn register_instruction(opcode: u8, destination: u8, index: u8) -> Vec<u8> {
    vec![0x66, 0x0f, opcode, 0xc0 | index << 3 | destination]
}

fn immediate_instruction(extension: u8, destination: u8, index: u8) -> Vec<u8> {
    vec![0x66, 0x0f, 0xba, 0xc0 | extension << 3 | destination, index]
}

fn decode_admission(immediate: bool) {
    for (opcode, extension) in FORMS {
        for destination in 0..8 {
            let bytes = if immediate {
                immediate_instruction(extension, destination, 0xf3)
            } else {
                register_instruction(opcode, destination, (destination + 1) & 7)
            };
            let engine = code(&bytes);
            let before = engine.arena().to_vec();
            let decoded = decode_one(engine.memory().unwrap(), GuestAddress(CODE))
                .expect("WORD register bit test family must decode");
            assert_eq!(
                decoded.operation(),
                &Operation::BitTestWord {
                    kind: kind(opcode),
                    destination: REGISTERS[usize::from(destination)],
                    index: if immediate {
                        BitIndex::Immediate(0xf3)
                    } else {
                        BitIndex::Register(REGISTERS[usize::from((destination + 1) & 7)])
                    },
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
    }
}

#[test]
fn word_bit_test_register_index_forms_decode() {
    decode_admission(false);
}

#[test]
fn word_bit_test_immediate_index_forms_decode() {
    decode_admission(true);
}

fn bound_admission(resident: bool, entries: bool) {
    assert_bound_admission(&bank(), resident, entries);
}

fn bank() -> Vec<u8> {
    let mut bytes = Vec::new();
    for (opcode, extension) in FORMS {
        bytes.extend(register_instruction(opcode, 0, 1));
        bytes.extend(register_instruction(opcode, 2, 2));
        bytes.extend(immediate_instruction(extension, 0, 0x66));
        bytes.extend(immediate_instruction(extension, 2, 0xf3));
    }
    bytes.extend([0xeb, 0]);
    bytes
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
    .expect("WORD register bit test family must compile bound");
    let module = if resident {
        engine.resident_bytes(id).unwrap()
    } else {
        engine.artifact_bytes().unwrap()
    };
    assert_eq!(&module[..8], b"\0asm\x01\0\0\0");
    assert_eq!(engine.arena(), before);
}

#[test]
fn word_bit_test_compiles_replacement_explicit() {
    bound_admission(false, false);
}

#[test]
fn word_bit_test_compiles_replacement_entry() {
    bound_admission(false, true);
}

#[test]
fn word_bit_test_compiles_resident_explicit() {
    bound_admission(true, false);
}

#[test]
fn word_bit_test_compiles_resident_entry() {
    bound_admission(true, true);
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

fn kind(opcode: u8) -> BitTestKind {
    match opcode {
        0xa3 => BitTestKind::Test,
        0xab => BitTestKind::Set,
        0xb3 => BitTestKind::Reset,
        0xbb => BitTestKind::Complement,
        _ => unreachable!(),
    }
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

fn assert_operation(bytes: &[u8], operation: Operation) {
    let memory = code_space(CODE, bytes);
    let decoded = decode_one(&memory, GuestAddress(CODE)).unwrap();
    assert_eq!(decoded.operation(), &operation, "{bytes:02x?}");
    assert_eq!(
        (decoded.length() as usize, decoded.next_pc()),
        (bytes.len(), GuestAddress(CODE + bytes.len() as u32))
    );
    assert!(memory.is_code_current(decoded.code_snapshot()));
    assert_eq!(memory.mapped_pages(), 1);
}

#[test]
fn word_bit_test_all_register_pairs_and_raw_immediate_indexes_have_exact_ir() {
    let raw_indexes = [
        0, 1, 15, 16, 17, 31, 32, 33, 0x66, 0x67, 0xf0, 0xf2, 0xf3, 255,
    ];
    let (mut pairs, mut immediates) = (0, 0);
    for (opcode, extension) in FORMS {
        for (destination_index, destination) in REGISTERS.into_iter().enumerate() {
            for (index_number, index) in REGISTERS.into_iter().enumerate() {
                assert_operation(
                    &register_instruction(opcode, destination_index as u8, index_number as u8),
                    Operation::BitTestWord {
                        kind: kind(opcode),
                        destination,
                        index: BitIndex::Register(index),
                    },
                );
                pairs += 1;
            }
            for raw in raw_indexes {
                assert_operation(
                    &immediate_instruction(extension, destination_index as u8, raw),
                    Operation::BitTestWord {
                        kind: kind(opcode),
                        destination,
                        index: BitIndex::Immediate(raw),
                    },
                );
                immediates += 1;
            }
        }
    }
    assert_eq!((pairs, immediates), (256, 448));
}

#[test]
fn word_bit_test_prefixes_and_reserved_ba_fields_have_precise_categories() {
    for (opcode, extension) in FORMS {
        for destination in 0..8 {
            for bytes in [
                register_instruction(opcode, destination, destination),
                immediate_instruction(extension, destination, 0xf3),
            ] {
                for prefix in [0x66, 0x67, 0xf2, 0xf3, 0xf0] {
                    let prefixed = [vec![prefix], bytes.clone()].concat();
                    assert_eq!(
                        decode_one(&code_space(CODE, &prefixed), GuestAddress(CODE)).err(),
                        Some(if prefix == 0xf0 {
                            DecodeError::InvalidEncoding
                        } else {
                            DecodeError::Unsupported(UnsupportedFeature::Opcode)
                        }),
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
                let memory = code_space(CODE, &bytes[1..]);
                assert!(matches!(
                    decode_one(&memory, GuestAddress(CODE)).unwrap().operation(),
                    Operation::BitTest { .. }
                ));
            }
        }
    }
    for extension in 0..4 {
        for modrm in [0xc3, 0x03] {
            let bytes = [0x66, 0x0f, 0xba, modrm | extension << 3, 0xf3];
            assert_eq!(
                decode_one(&code_space(CODE, &bytes), GuestAddress(CODE)).err(),
                Some(DecodeError::InvalidEncoding),
                "{bytes:02x?}"
            );
        }
    }
}

#[test]
fn word_bit_test_duplicate_memory_prefixes_and_lock_categories_stay_precise() {
    let tails: [&[u8]; 13] = [
        &[0x00],
        &[0x02],
        &[0x03],
        &[0x04, 0x24],
        &[0x45, 0],
        &[0x43, 0x80],
        &[0x43, 0x7f],
        &[0x83, 0x66, 0x67, 0xf0, 0xf3],
        &[0x04, 0x80],
        &[0x04, 0x92],
        &[0x04, 0x8d, 0x66, 0x67, 0xf0, 0xf3],
        &[0x05, 0x66, 0x67, 0xf0, 0xf3],
        &[0x84, 0xf4, 0x66, 0x67, 0xf0, 0xf3],
    ];
    let mut rows = 0;
    for (opcode, extension) in FORMS {
        for immediate in [false, true] {
            for tail in tails {
                let mut bytes = vec![
                    0x66,
                    0x0f,
                    if immediate { 0xba } else { opcode },
                    tail[0] | (if immediate { extension } else { 2 }) << 3,
                ];
                bytes.extend_from_slice(&tail[1..]);
                if immediate {
                    bytes.push(0xf3);
                }
                let mut duplicate = bytes.clone();
                duplicate.insert(0, 0x66);
                assert_eq!(
                    decode_one(&code_space(CODE, &duplicate), GuestAddress(CODE)).err(),
                    Some(DecodeError::Unsupported(UnsupportedFeature::Opcode)),
                    "{duplicate:02x?}"
                );
                let locked = [vec![0xf0], bytes].concat();
                assert_eq!(
                    decode_one(&code_space(CODE, &locked), GuestAddress(CODE)).err(),
                    Some(if kind(opcode) == BitTestKind::Test {
                        DecodeError::InvalidEncoding
                    } else {
                        DecodeError::Unsupported(UnsupportedFeature::Opcode)
                    }),
                    "{locked:02x?}"
                );
                rows += 1;
            }
        }
    }
    assert_eq!(rows, 104);
}

#[test]
fn word_bit_test_compact_bank_compiles_in_all_six_profiles() {
    let bytes = bank();
    let memory = code_space(CODE, &bytes);
    for compiled in [
        compile_region(
            &memory,
            &[BlockSpec {
                entry: GuestAddress(CODE),
                byte_length: bytes.len() as u32,
            }],
            CompileLimits::default(),
        )
        .unwrap(),
        compile_entry_region(&memory, &[GuestAddress(CODE)], CompileLimits::default()).unwrap(),
    ] {
        assert_eq!(
            (compiled.metadata().blocks, compiled.metadata().instructions),
            (1, 17)
        );
        assert_eq!(
            &compiled.wasm_bytes(&memory).unwrap()[..8],
            b"\0asm\x01\0\0\0"
        );
    }
    for resident in [false, true] {
        for entries in [false, true] {
            assert_bound_admission(&bytes, resident, entries);
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
fn word_bit_test_fetch_checks_every_opcode_modrm_and_raw_immediate_byte() {
    for (opcode, extension) in FORMS {
        for destination in 0..8 {
            for bytes in [
                register_instruction(opcode, destination, destination),
                immediate_instruction(extension, destination, 0xf3),
            ] {
                assert_eq!(bytes.len(), if bytes[2] == 0xba { 5 } else { 4 });
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
                    assert_eq!(
                        decode_one(&code_space(pc, &bytes[..cut]), GuestAddress(pc)).err(),
                        Some(fetch_error(pc, 0x2000, FaultReason::Unmapped, cut + 1))
                    );
                    let top = u32::MAX - cut as u32 + 1;
                    assert_eq!(
                        decode_one(&code_space(top, &bytes[..cut]), GuestAddress(top)).err(),
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
}

#[test]
fn word_bit_test_mixed_64_instruction_block_fits_every_bound_profile() {
    let once = bank();
    let mut bytes = once[..once.len() - 2].repeat(2);
    bytes.extend(std::iter::repeat_n(0x90, 31));
    bytes.extend([0xeb, 0]);
    for resident in [false, true] {
        for entries in [false, true] {
            assert_bound_admission(&bytes, resident, entries);
        }
    }
}

#[test]
fn word_bit_test_late_refusal_preserves_full_arena_and_both_owners() {
    let mut bytes = Vec::new();
    for (opcode, extension) in FORMS {
        bytes.extend(register_instruction(opcode, 0, 0));
        bytes.extend(immediate_instruction(extension, 2, 0xf3));
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
