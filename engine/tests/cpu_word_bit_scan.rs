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
            ir::{BitScanKind, EffectiveAddress, Operation},
        },
    },
    memory::{
        Access, AddressSpace, FaultReason, GuestAddress, MemoryFault, PageRange, Permissions,
    },
    process::{EngineInstance, HostError},
};

const CODE: u32 = 0x1000;
const KEY: u64 = 0x5742_4954_5343_3031;

fn code(bytes: &[u8]) -> EngineInstance {
    let mut engine = EngineInstance::new(1, KEY).unwrap();
    engine.map(CODE, 1, 7).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(bytes);
    engine.upload(CODE, bytes.len() as u32).unwrap();
    engine.protect(CODE, 1, 4).unwrap();
    engine
}

fn decode_admission(opcode: u8, memory: bool) {
    for destination in 0..8 {
        let modrm = destination << 3
            | if memory {
                3
            } else {
                0xc0 | ((destination + 1) & 7)
            };
        let engine = code(&[0x66, 0x0f, opcode, modrm]);
        let before = engine.arena().to_vec();
        let decoded = decode_one(engine.memory().unwrap(), GuestAddress(CODE))
            .expect("WORD bit scan must decode without accessing data");
        assert_eq!(
            decoded.operation(),
            &if memory {
                Operation::ReadBitScanWord {
                    kind: kind(opcode),
                    destination: REGISTERS[usize::from(destination)],
                    address: addresses()[0].1,
                }
            } else {
                Operation::BitScanWord {
                    kind: kind(opcode),
                    destination: REGISTERS[usize::from(destination)],
                    source: REGISTERS[usize::from((destination + 1) & 7)],
                }
            }
        );
        assert_eq!(
            (decoded.length(), decoded.next_pc()),
            (4, GuestAddress(CODE + 4))
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
fn word_register_bsf_decodes() {
    decode_admission(0xbc, false);
}

#[test]
fn word_register_bsr_decodes() {
    decode_admission(0xbd, false);
}

#[test]
fn word_memory_bsf_decodes_without_reading_data() {
    decode_admission(0xbc, true);
}

#[test]
fn word_memory_bsr_decodes_without_reading_data() {
    decode_admission(0xbd, true);
}

fn bound_admission(resident: bool, entries: bool) {
    let mut bytes = Vec::new();
    for opcode in [0xbc, 0xbd] {
        for destination in 0..8 {
            bytes.extend([
                0x66,
                0x0f,
                opcode,
                0xc0 | destination << 3 | ((destination + 1) & 7),
            ]);
            bytes.extend([0x66, 0x0f, opcode, destination << 3 | 3]);
        }
    }
    bytes.extend([0xeb, 0]);
    assert_bound_admission(&bytes, resident, entries);
}

fn assert_bound_admission(bytes: &[u8], resident: bool, entries: bool) {
    let mut engine = code(bytes);
    let descriptor = &mut engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8];
    descriptor[..4].copy_from_slice(&CODE.to_le_bytes());
    descriptor[4..].copy_from_slice(&if entries { 0 } else { bytes.len() as u32 }.to_le_bytes());
    let before = engine.arena().to_vec();
    let id = match (resident, entries) {
        (false, false) => engine.compile(1).map(u64::from),
        (false, true) => engine.compile_entries(1, 0).map(u64::from),
        (true, false) => engine.compile_resident(1).map(|id| id.get()),
        (true, true) => engine.compile_resident_entries(1, 0).map(|id| id.get()),
    }
    .expect("WORD register and memory bit scans must compile bound without reading data");
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
fn word_bit_scan_compiles_replacement_explicit() {
    bound_admission(false, false);
}

#[test]
fn word_bit_scan_compiles_replacement_entry() {
    bound_admission(false, true);
}

#[test]
fn word_bit_scan_compiles_resident_explicit() {
    bound_admission(true, false);
}

#[test]
fn word_bit_scan_compiles_resident_entry() {
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

fn kind(opcode: u8) -> BitScanKind {
    match opcode {
        0xbc => BitScanKind::Forward,
        0xbd => BitScanKind::Reverse,
        _ => unreachable!(),
    }
}

fn encoding(opcode: u8, destination: u8, tail: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0x66, 0x0f, opcode, tail[0] | destination << 3];
    bytes.extend_from_slice(&tail[1..]);
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
fn word_bit_scan_all_register_pairs_and_memory_addresses_have_exact_ir() {
    let (mut registers, mut memory) = (0, 0);
    for opcode in [0xbc, 0xbd] {
        for (destination_index, destination) in REGISTERS.into_iter().enumerate() {
            for (source_index, source) in REGISTERS.into_iter().enumerate() {
                assert_operation(
                    &encoding(
                        opcode,
                        destination_index as u8,
                        &[0xc0 | source_index as u8],
                    ),
                    Operation::BitScanWord {
                        kind: kind(opcode),
                        destination,
                        source,
                    },
                );
                registers += 1;
            }
            for (tail, address) in addresses() {
                assert_operation(
                    &encoding(opcode, destination_index as u8, tail),
                    Operation::ReadBitScanWord {
                        kind: kind(opcode),
                        destination,
                        address,
                    },
                );
                memory += 1;
            }
        }
    }
    assert_eq!((registers, memory), (128, 208));
}

#[test]
fn word_bit_scan_prefix_looking_payloads_and_destination_address_aliases_are_literal() {
    for opcode in [0xbc, 0xbd] {
        for sib in [0x66, 0x67, 0xf0, 0xf2, 0xf3] {
            let index = (sib >> 3) & 7;
            assert_operation(
                &encoding(opcode, 1, &[0x04, sib]),
                Operation::ReadBitScanWord {
                    kind: kind(opcode),
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
                    },
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
                },
            ),
            (
                &[0x04, 0x05, 0x66, 0x67, 0xf0, 0xf3],
                EffectiveAddress {
                    base: None,
                    index: Some(Register32::Eax),
                    scale: 1,
                    displacement: 0xf3f0_6766,
                },
            ),
            (
                &[0x04, 0x00],
                EffectiveAddress {
                    base: Some(Register32::Eax),
                    index: Some(Register32::Eax),
                    scale: 1,
                    displacement: 0,
                },
            ),
        ] {
            assert_operation(
                &encoding(opcode, 0, tail),
                Operation::ReadBitScanWord {
                    kind: kind(opcode),
                    destination: Register32::Eax,
                    address,
                },
            );
        }
    }
}

#[test]
fn word_bit_scan_prefixes_and_admitted_neighbors_have_precise_boundaries() {
    let unsupported = DecodeError::Unsupported(UnsupportedFeature::Opcode);
    for opcode in [0xbc, 0xbd] {
        for destination in 0..8 {
            for tail in [&[0xc3][..], &[0x03]] {
                let bytes = encoding(opcode, destination, tail);
                for prefix in [0x66, 0x67, 0xf0, 0xf2, 0xf3] {
                    let prefixed = [vec![prefix], bytes.clone()].concat();
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
                    assert_eq!(
                        decode_one(&code_space(CODE, &prefixed), GuestAddress(CODE)).err(),
                        Some(DecodeError::Unsupported(UnsupportedFeature::Segment)),
                        "{prefixed:02x?}"
                    );
                }
                for rep in [0xf2, 0xf3] {
                    let prefixed = [vec![0x66, rep], bytes[1..].to_vec()].concat();
                    assert_eq!(
                        decode_one(&code_space(CODE, &prefixed), GuestAddress(CODE)).err(),
                        Some(unsupported)
                    );
                }
                let dword = &bytes[1..];
                assert_operation(
                    dword,
                    if tail[0] & 0xc0 == 0xc0 {
                        Operation::BitScan {
                            kind: kind(opcode),
                            destination: REGISTERS[usize::from(destination)],
                            source: Register32::Ebx,
                        }
                    } else {
                        Operation::ReadBitScan {
                            kind: kind(opcode),
                            destination: REGISTERS[usize::from(destination)],
                            address: addresses()[0].1,
                        }
                    },
                );
            }
        }
        for tail in [0xc3, 0x03] {
            let bytes = [0xf3, 0x0f, opcode, tail];
            assert_eq!(
                decode_one(&code_space(CODE, &bytes), GuestAddress(CODE)).err(),
                Some(unsupported)
            );
        }
    }
    for bytes in [
        [0x66, 0x0f, 0xaf, 0xc3],
        [0x66, 0x0f, 0xaf, 0x03],
        [0x66, 0x0f, 0xa3, 0xc3],
        [0x66, 0x0f, 0xa3, 0x03],
    ] {
        assert_eq!(
            decode_one(&code_space(CODE, &bytes), GuestAddress(CODE)).err(),
            Some(unsupported)
        );
    }
}

#[test]
fn word_register_bit_scan_bank_compiles_in_all_six_profiles() {
    let mut bytes = Vec::new();
    for opcode in [0xbc, 0xbd] {
        for destination in 0..8 {
            for source in [destination, (destination + 1) & 7] {
                bytes.extend(encoding(opcode, destination, &[0xc0 | source]));
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
fn decoded_word_memory_bit_scans_stay_closed_in_standalone_backend() {
    for opcode in [0xbc, 0xbd] {
        for destination in 0..8 {
            let mut bytes = encoding(opcode, destination, &[0x03]);
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
fn word_bit_scan_fetch_checks_every_register_and_memory_encoding_byte() {
    for opcode in [0xbc, 0xbd] {
        for destination in 0..8 {
            for tail in [&[0xc3][..], &[0x84, 0xf4, 0x66, 0x67, 0xf0, 0xf3]] {
                let bytes = encoding(opcode, destination, tail);
                assert_eq!(bytes.len(), if tail[0] & 0xc0 == 0xc0 { 4 } else { 9 });
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
fn word_bit_scan_mixed_64_instruction_block_fits_every_bound_profile() {
    let mut bytes = Vec::new();
    for opcode in [0xbc, 0xbd] {
        for destination in 0..8 {
            bytes.extend(encoding(opcode, destination, &[0xc0 | destination]));
            bytes.extend(encoding(opcode, destination, &[0x03]));
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
fn word_bit_scan_late_refusal_preserves_full_arena_and_both_owners() {
    let mut bytes = Vec::new();
    for opcode in [0xbc, 0xbd] {
        bytes.extend(encoding(opcode, 1, &[0xc1]));
        bytes.extend(encoding(opcode, 1, &[0x04, 0x09]));
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
