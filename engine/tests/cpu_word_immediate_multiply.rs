#[path = "support/word_prefix.rs"]
mod word_prefix;
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
            ir::{EffectiveAddress, Operation},
        },
    },
    memory::{
        Access, AddressSpace, FaultReason, GuestAddress, MemoryFault, PageRange, Permissions,
    },
    process::{EngineInstance, HostError},
};

const CODE: u32 = 0x1000;
const KEY: u64 = 0x5749_4d4d_554c_3136;

fn code(bytes: &[u8]) -> EngineInstance {
    let mut engine = EngineInstance::new(1, KEY).unwrap();
    engine.map(CODE, 1, 7).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(bytes);
    engine.upload(CODE, bytes.len() as u32).unwrap();
    engine.protect(CODE, 1, 4).unwrap();
    engine
}

fn encoding(opcode: u8, destination: u8, memory: bool) -> Vec<u8> {
    let modrm = destination << 3
        | if memory {
            3
        } else {
            0xc0 | ((destination + 1) & 7)
        };
    let mut bytes = vec![0x66, opcode, modrm, 0x80];
    if opcode == 0x69 {
        bytes.push(0x92);
    }
    bytes
}

fn decode_admission(opcode: u8, memory: bool) {
    for destination in 0..8 {
        let bytes = encoding(opcode, destination, memory);
        let engine = code(&bytes);
        let before = engine.arena().to_vec();
        let decoded = decode_one(engine.memory().unwrap(), GuestAddress(CODE))
            .expect("WORD immediate IMUL must decode without accessing data");
        assert_eq!(
            decoded.operation(),
            &if memory {
                Operation::ReadSignedMultiplyImmediateWord {
                    destination: REGISTERS[usize::from(destination)],
                    address: addresses()[0].1,
                    immediate: normalized(opcode, if opcode == 0x69 { 0x9280 } else { 0x80 }),
                }
            } else {
                Operation::SignedMultiplyImmediateWord {
                    destination: REGISTERS[usize::from(destination)],
                    source: REGISTERS[usize::from((destination + 1) & 7)],
                    immediate: normalized(opcode, if opcode == 0x69 { 0x9280 } else { 0x80 }),
                }
            }
        );
        assert_eq!(
            (decoded.length() as usize, decoded.next_pc()),
            (bytes.len(), GuestAddress(CODE + bytes.len() as u32))
        );
        assert_eq!(engine.arena(), before);
        assert_eq!(engine.memory().unwrap().mapped_pages(), 1);
        assert!(
            engine
                .memory()
                .unwrap()
                .resolve(GuestAddress(0), Access::Read)
                .is_err()
        );
    }
}

#[test]
fn word_register_imul_imm16_decodes() {
    decode_admission(0x69, false);
}

#[test]
fn word_register_imul_sign_extended_imm8_decodes() {
    decode_admission(0x6b, false);
}

#[test]
fn word_memory_imul_imm16_decodes_without_reading_data() {
    decode_admission(0x69, true);
}

#[test]
fn word_memory_imul_sign_extended_imm8_decodes_without_reading_data() {
    decode_admission(0x6b, true);
}

fn bound_admission(resident: bool, entries: bool) {
    let mut bytes = Vec::new();
    for opcode in [0x69, 0x6b] {
        for destination in 0..8 {
            for memory in [false, true] {
                bytes.extend(encoding(opcode, destination, memory));
            }
        }
    }
    bytes.extend([0xeb, 0]);
    assert_bound_admission(&bytes, resident, entries);
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
    .expect("WORD immediate IMUL must compile bound without reading data");
    let module = if resident {
        engine.resident_bytes(id).unwrap()
    } else {
        engine.artifact_bytes().unwrap()
    };
    assert_eq!(&module[..8], b"\0asm\x01\0\0\0");
    assert_eq!(engine.arena(), before);
    assert_eq!(engine.memory().unwrap().mapped_pages(), 1);
}

#[test]
fn word_immediate_multiply_compiles_replacement_explicit() {
    bound_admission(false, false);
}

#[test]
fn word_immediate_multiply_compiles_replacement_entry() {
    bound_admission(false, true);
}

#[test]
fn word_immediate_multiply_compiles_resident_explicit() {
    bound_admission(true, false);
}

#[test]
fn word_immediate_multiply_compiles_resident_entry() {
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

fn normalized(opcode: u8, raw: u16) -> u16 {
    if opcode == 0x69 {
        raw
    } else {
        raw as u8 as i8 as i16 as u16
    }
}

fn instruction(opcode: u8, destination: u8, tail: &[u8], raw: u16) -> Vec<u8> {
    let mut bytes = vec![0x66, opcode, tail[0] | destination << 3];
    bytes.extend_from_slice(&tail[1..]);
    if opcode == 0x69 {
        bytes.extend(raw.to_le_bytes());
    } else {
        bytes.push(raw as u8);
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
        ..Default::default()
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
fn word_immediate_imul_all_register_pairs_and_memory_addresses_have_exact_ir() {
    let (mut registers, mut memory) = (0, 0);
    for opcode in [0x69, 0x6b] {
        let raw = if opcode == 0x69 { 0x9280 } else { 0x80 };
        for (destination_index, destination) in REGISTERS.into_iter().enumerate() {
            for (source_index, source) in REGISTERS.into_iter().enumerate() {
                assert_operation(
                    &instruction(
                        opcode,
                        destination_index as u8,
                        &[0xc0 | source_index as u8],
                        raw,
                    ),
                    Operation::SignedMultiplyImmediateWord {
                        destination,
                        source,
                        immediate: normalized(opcode, raw),
                    },
                );
                registers += 1;
            }
            for (tail, address) in addresses() {
                assert_operation(
                    &instruction(opcode, destination_index as u8, tail, raw),
                    Operation::ReadSignedMultiplyImmediateWord {
                        destination,
                        address,
                        immediate: normalized(opcode, raw),
                    },
                );
                memory += 1;
            }
        }
    }
    assert_eq!((registers, memory), (128, 208));
}

#[test]
fn word_immediate_imul_imm16_identity_and_imm8_sign_extension_stay_exact() {
    assert_operation(
        &[0x66, 0x69, 0xc3, 0x80, 0x92],
        Operation::SignedMultiplyImmediateWord {
            destination: Register32::Eax,
            source: Register32::Ebx,
            immediate: 0x9280,
        },
    );
    assert_operation(
        &[0x66, 0x6b, 0x03, 0x80],
        Operation::ReadSignedMultiplyImmediateWord {
            destination: Register32::Eax,
            address: addresses()[0].1,
            immediate: 0xff80,
        },
    );
    for opcode in [0x69, 0x6b] {
        let values: &[u16] = if opcode == 0x69 {
            &[0, 1, 0x7fff, 0x8000, 0xffff, 0x9280, 0xf366, 0xf066]
        } else {
            &[0, 1, 0x7f, 0x80, 0xff, 0x66, 0x67, 0xf0, 0xf2, 0xf3]
        };
        for (index, destination) in REGISTERS.into_iter().enumerate() {
            let source_index = (index + 1) & 7;
            for &raw in values {
                assert_operation(
                    &instruction(opcode, index as u8, &[0xc0 | source_index as u8], raw),
                    Operation::SignedMultiplyImmediateWord {
                        destination,
                        source: REGISTERS[source_index],
                        immediate: normalized(opcode, raw),
                    },
                );
                assert_operation(
                    &instruction(opcode, index as u8, &[0x03], raw),
                    Operation::ReadSignedMultiplyImmediateWord {
                        destination,
                        address: addresses()[0].1,
                        immediate: normalized(opcode, raw),
                    },
                );
            }
        }
    }
}

#[test]
fn word_immediate_imul_payloads_and_destination_address_aliases_are_literal() {
    for opcode in [0x69, 0x6b] {
        let raw = if opcode == 0x69 { 0xf366 } else { 0xf3 };
        for sib in [0x66, 0x67, 0xf0, 0xf2, 0xf3] {
            let index = (sib >> 3) & 7;
            assert_operation(
                &instruction(opcode, 1, &[0x04, sib], raw),
                Operation::ReadSignedMultiplyImmediateWord {
                    destination: Register32::Ecx,
                    address: EffectiveAddress {
                        base: Some(REGISTERS[usize::from(sib & 7)]),
                        index: if index == 4 {
                            None
                        } else {
                            Some(REGISTERS[usize::from(index)])
                        },
                        scale: 1 << (sib >> 6),
                        displacement: 0,
                        ..Default::default()
                    },
                    immediate: normalized(opcode, raw),
                },
            );
        }
        for (tail, address) in [
            (
                &[0x00][..],
                EffectiveAddress {
                    base: Some(Register32::Eax),
                    index: None,
                    scale: 1,
                    displacement: 0,
                    ..Default::default()
                },
            ),
            (
                &[0x04, 0x05, 0x66, 0x67, 0xf0, 0xf3],
                EffectiveAddress {
                    base: None,
                    index: Some(Register32::Eax),
                    scale: 1,
                    displacement: 0xf3f0_6766,
                    ..Default::default()
                },
            ),
            (
                &[0x04, 0x00],
                EffectiveAddress {
                    base: Some(Register32::Eax),
                    index: Some(Register32::Eax),
                    scale: 1,
                    displacement: 0,
                    ..Default::default()
                },
            ),
        ] {
            assert_operation(
                &instruction(opcode, 0, tail, raw),
                Operation::ReadSignedMultiplyImmediateWord {
                    destination: Register32::Eax,
                    address,
                    immediate: normalized(opcode, raw),
                },
            );
        }
    }
}

#[test]
fn word_immediate_imul_prefixes_and_admitted_neighbors_have_precise_boundaries() {
    let unsupported = DecodeError::Unsupported(UnsupportedFeature::Opcode);
    for opcode in [0x69, 0x6b] {
        for destination in 0..8 {
            for memory in [false, true] {
                let bytes = encoding(opcode, destination, memory);
                for prefix in [0x66, 0x67, 0xf0, 0xf2, 0xf3] {
                    let prefixed = [vec![prefix], bytes.clone()].concat();
                    if matches!(prefix, 0x67) {
                        word_prefix::admitted(&prefixed);
                        continue;
                    }
                    assert_eq!(
                        decode_one(&code_space(CODE, &prefixed), GuestAddress(CODE)).err(),
                        Some(if prefix == 0xf0 {
                            DecodeError::InvalidEncoding
                        } else {
                            unsupported
                        }),
                        "{prefixed:02x?}"
                    );
                }
                for prefix in [0x26, 0x2e, 0x36, 0x3e, 0x64, 0x65] {
                    let prefixed = [vec![prefix], bytes.clone()].concat();
                    word_prefix::admitted(&prefixed);
                }
            }
        }
    }
    for bytes in [
        [0x66, 0x66, 0xf7, 0xeb],
        [0x66, 0x66, 0xf7, 0x2b],
        [0x66, 0x66, 0xf7, 0xe3],
        [0x66, 0x66, 0xf7, 0x23],
    ] {
        assert_eq!(
            decode_one(&code_space(CODE, &bytes), GuestAddress(CODE)).err(),
            Some(unsupported)
        );
    }
    assert_operation(
        &[0x66, 0x0f, 0xaf, 0xc3],
        Operation::SignedMultiplyWord {
            destination: Register32::Eax,
            source: Register32::Ebx,
        },
    );
    assert_operation(
        &[0x66, 0x0f, 0xaf, 0x03],
        Operation::ReadSignedMultiplyWord {
            destination: Register32::Eax,
            address: addresses()[0].1,
        },
    );
}

#[test]
fn word_immediate_imul_register_bank_compiles_in_all_six_profiles() {
    let mut bytes = Vec::new();
    for opcode in [0x69, 0x6b] {
        for destination in 0..8 {
            for source in [destination, (destination + 1) & 7] {
                bytes.extend(instruction(opcode, destination, &[0xc0 | source], 0));
            }
        }
    }
    bytes.extend([0xeb, 0]);
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
            (1, 33)
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

#[test]
fn decoded_word_immediate_memory_imul_stays_closed_in_standalone_backend() {
    for opcode in [0x69, 0x6b] {
        for destination in 0..8 {
            let mut bytes = instruction(opcode, destination, &[0x03], 0);
            bytes.extend([0xeb, 0]);
            let memory = code_space(CODE, &bytes);
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
fn word_immediate_imul_fetch_checks_every_register_memory_and_immediate_byte() {
    for opcode in [0x69, 0x6b] {
        for destination in 0..8 {
            for tail in [&[0xc3][..], &[0x84, 0xf4, 0x66, 0x67, 0xf0, 0xf3]] {
                let bytes = instruction(opcode, destination, tail, 0xf366);
                assert_eq!(
                    bytes.len(),
                    match (opcode, tail[0] & 0xc0 == 0xc0) {
                        (0x69, true) => 5,
                        (0x6b, true) => 4,
                        (0x69, false) => 10,
                        (0x6b, false) => 9,
                        _ => unreachable!(),
                    }
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
fn word_immediate_imul_mixed_64_instruction_block_fits_every_bound_profile() {
    let mut bytes = Vec::new();
    for opcode in [0x69, 0x6b] {
        for destination in 0..8 {
            bytes.extend(instruction(
                opcode,
                destination,
                &[0xc0 | destination],
                0x80,
            ));
            bytes.extend(instruction(opcode, destination, &[0x03], 0));
        }
    }
    bytes.extend(std::iter::repeat_n(0x90, 31));
    bytes.extend([0xeb, 0]);
    for resident in [false, true] {
        for entries in [false, true] {
            assert_bound_admission(&bytes, resident, entries);
        }
    }
}

#[test]
fn word_immediate_imul_late_refusal_preserves_full_arena_and_both_owners() {
    let mut bytes = Vec::new();
    for opcode in [0x69, 0x6b] {
        bytes.extend(instruction(opcode, 1, &[0xc1], 0x80));
        bytes.extend(instruction(opcode, 1, &[0x04, 0x09], 0));
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
