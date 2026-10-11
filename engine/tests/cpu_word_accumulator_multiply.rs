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
            ir::{EffectiveAddress, MultiplyKind, Operation},
        },
    },
    memory::{
        Access, AddressSpace, FaultReason, GuestAddress, MemoryFault, PageRange, Permissions,
    },
    process::{EngineInstance, HostError},
};

const CODE: u32 = 0x1000;
const KEY: u64 = 0x5741_4343_4d55_4c31;

fn code(bytes: &[u8]) -> EngineInstance {
    let mut engine = EngineInstance::new(1, KEY).unwrap();
    engine.map(CODE, 1, 7).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(bytes);
    engine.upload(CODE, bytes.len() as u32).unwrap();
    engine.protect(CODE, 1, 4).unwrap();
    engine
}

fn encoding(extension: u8, source: u8, memory: bool) -> Vec<u8> {
    vec![
        0x66,
        0xf7,
        extension << 3 | if memory { 3 } else { 0xc0 | source },
    ]
}

fn decode_admission(extension: u8, memory: bool) {
    for source in 0..if memory { 1 } else { 8 } {
        let bytes = encoding(extension, source, memory);
        let engine = code(&bytes);
        let before = engine.arena().to_vec();
        let decoded = decode_one(engine.memory().unwrap(), GuestAddress(CODE))
            .expect("WORD accumulator multiply must decode without accessing data");
        assert_eq!(
            decoded.operation(),
            &if memory {
                Operation::ReadWordMultiplyAccumulator {
                    kind: kind(extension),
                    address: addresses()[0].1,
                }
            } else {
                Operation::WordMultiplyAccumulator {
                    kind: kind(extension),
                    source: REGISTERS[usize::from(source)],
                }
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
fn word_register_mul_decodes() {
    decode_admission(4, false);
}

#[test]
fn word_register_implicit_imul_decodes() {
    decode_admission(5, false);
}

#[test]
fn word_memory_mul_decodes_without_reading_data() {
    decode_admission(4, true);
}

#[test]
fn word_memory_implicit_imul_decodes_without_reading_data() {
    decode_admission(5, true);
}

fn bound_admission(resident: bool, entries: bool) {
    let mut bytes = Vec::new();
    for extension in [4, 5] {
        for source in 0..8 {
            bytes.extend(encoding(extension, source, false));
        }
        bytes.extend(encoding(extension, 0, true));
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
    .expect("WORD accumulator multiply must compile bound without reading data");
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
fn word_accumulator_multiply_compiles_replacement_explicit() {
    bound_admission(false, false);
}

#[test]
fn word_accumulator_multiply_compiles_replacement_entry() {
    bound_admission(false, true);
}

#[test]
fn word_accumulator_multiply_compiles_resident_explicit() {
    bound_admission(true, false);
}

#[test]
fn word_accumulator_multiply_compiles_resident_entry() {
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

fn kind(extension: u8) -> MultiplyKind {
    match extension {
        4 => MultiplyKind::Unsigned,
        5 => MultiplyKind::Signed,
        _ => unreachable!(),
    }
}

fn instruction(extension: u8, tail: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0x66, 0xf7, tail[0] | extension << 3];
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
fn word_accumulator_multiply_all_sources_and_memory_addresses_have_exact_ir() {
    let (mut registers, mut memory) = (0, 0);
    for extension in [4, 5] {
        for (index, source) in REGISTERS.into_iter().enumerate() {
            assert_operation(
                &instruction(extension, &[0xc0 | index as u8]),
                Operation::WordMultiplyAccumulator {
                    kind: kind(extension),
                    source,
                },
            );
            registers += 1;
        }
        for (tail, address) in addresses() {
            assert_operation(
                &instruction(extension, tail),
                Operation::ReadWordMultiplyAccumulator {
                    kind: kind(extension),
                    address,
                },
            );
            memory += 1;
        }
    }
    assert_eq!((registers, memory), (16, 26));
}

#[test]
fn word_accumulator_multiply_payloads_and_implicit_destination_aliases_are_literal() {
    use Register32::{Eax, Edx};
    for extension in [4, 5] {
        for sib in [0x66, 0x67, 0xf0, 0xf2, 0xf3] {
            let index = (sib >> 3) & 7;
            assert_operation(
                &instruction(extension, &[0x44, sib, 0x80]),
                Operation::ReadWordMultiplyAccumulator {
                    kind: kind(extension),
                    address: EffectiveAddress {
                        base: Some(REGISTERS[usize::from(sib & 7)]),
                        index: (index != 4).then(|| REGISTERS[usize::from(index)]),
                        scale: 1 << (sib >> 6),
                        displacement: 0xffff_ff80,
                        ..Default::default()
                    },
                },
            );
        }
        for (tail, base, index, scale, displacement) in [
            (&[0x00][..], Some(Eax), None, 1, 0),
            (&[0x02][..], Some(Edx), None, 1, 0),
            (
                &[0x04, 0x85, 0x66, 0x67, 0xf0, 0xf3][..],
                None,
                Some(Eax),
                4,
                0xf3f0_6766,
            ),
            (
                &[0x04, 0x95, 0x66, 0x67, 0xf0, 0xf3][..],
                None,
                Some(Edx),
                4,
                0xf3f0_6766,
            ),
            (&[0x04, 0x80][..], Some(Eax), Some(Eax), 4, 0),
            (&[0x04, 0x92][..], Some(Edx), Some(Edx), 4, 0),
            (&[0x04, 0x90][..], Some(Eax), Some(Edx), 4, 0),
            (&[0x04, 0x82][..], Some(Edx), Some(Eax), 4, 0),
        ] {
            assert_operation(
                &instruction(extension, tail),
                Operation::ReadWordMultiplyAccumulator {
                    kind: kind(extension),
                    address: EffectiveAddress {
                        base,
                        index,
                        scale,
                        displacement,
                        ..Default::default()
                    },
                },
            );
        }
    }
}

#[test]
fn word_accumulator_multiply_prefixes_and_duplicate_division_neighbors_have_exact_categories() {
    let opcode = DecodeError::Unsupported(UnsupportedFeature::Opcode);
    for extension in [4, 5] {
        let tails: Vec<_> = (0..8)
            .map(|source| vec![0xc0 | source])
            .chain(addresses().into_iter().map(|(tail, _)| tail.to_vec()))
            .collect();
        for tail in tails {
            let bytes = instruction(extension, &tail);
            for prefix in [0x66, 0x67, 0xf2, 0xf3, 0xf0] {
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
                        opcode
                    }),
                    "{prefixed:02x?}"
                );
            }
            for prefix in [0x26, 0x2e, 0x36, 0x3e, 0x64, 0x65] {
                let prefixed = [vec![prefix], bytes.clone()].concat();
                word_prefix::admitted(&prefixed);
            }
        }
        for tail in [&[0xc2][..], &[0x03][..]] {
            let word = instruction(extension, tail);
            let memory = code_space(CODE, &word[1..]);
            let decoded = decode_one(&memory, GuestAddress(CODE)).unwrap();
            assert!(matches!(
                decoded.operation(),
                Operation::MultiplyAccumulator { .. } | Operation::ReadMultiplyAccumulator { .. }
            ));
        }
    }
    for extension in [6, 7] {
        for tail in [&[0xc0][..], &[0xc2][..], &[0x03][..], &[0x04, 0x8b][..]] {
            let mut bytes = instruction(extension, tail);
            bytes.insert(0, 0x66);
            assert_eq!(
                decode_one(&code_space(CODE, &bytes), GuestAddress(CODE)).err(),
                Some(opcode),
                "{bytes:02x?}"
            );
        }
    }
    for extension in [4, 5] {
        for modrm in [0xc0, 0x03] {
            let bytes = [0x66, 0xf6, modrm | extension << 3];
            assert_eq!(
                decode_one(&code_space(CODE, &bytes), GuestAddress(CODE)).err(),
                Some(opcode)
            );
        }
    }
}

#[test]
fn word_accumulator_register_bank_compiles_in_all_six_profiles() {
    let mut bytes = Vec::new();
    for extension in [4, 5] {
        for source in 0..8 {
            bytes.extend(instruction(extension, &[0xc0 | source]));
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

#[test]
fn decoded_word_accumulator_memory_multiply_stays_closed_in_standalone_backend() {
    for extension in [4, 5] {
        let mut bytes = instruction(extension, &[0x03]);
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
            prepare_entry_region(&memory, &[GuestAddress(CODE)], CompileLimits::default()).err(),
            expected
        );
        assert_eq!(
            compile_entry_region(&memory, &[GuestAddress(CODE)], CompileLimits::default()).err(),
            expected
        );
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
fn word_accumulator_multiply_fetch_checks_every_register_memory_byte() {
    for extension in [4, 5] {
        for tail in [&[0xc2][..], &[0x84, 0xf4, 0x66, 0x67, 0xf0, 0xf3][..]] {
            let bytes = instruction(extension, tail);
            assert_eq!(bytes.len(), if tail[0] & 0xc0 == 0xc0 { 3 } else { 8 });
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

#[test]
fn word_accumulator_multiply_mixed_64_instruction_block_fits_every_bound_profile() {
    let mut bytes = Vec::new();
    for extension in [4, 5] {
        for source in 0..8 {
            bytes.extend(instruction(extension, &[0xc0 | source]));
            bytes.extend(instruction(extension, &[0x04, 0x10]));
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
fn word_accumulator_multiply_late_refusal_preserves_full_arena_and_both_owners() {
    let mut bytes = Vec::new();
    for extension in [4, 5] {
        bytes.extend(instruction(extension, &[0xc0]));
        bytes.extend(instruction(extension, &[0xc2]));
        bytes.extend(instruction(extension, &[0x04, 0x10]));
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
