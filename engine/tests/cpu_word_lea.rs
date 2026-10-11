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
            ir::{EffectiveAddress, Location32, Operation, UnaryKind, Value32},
        },
    },
    memory::{
        Access, AddressSpace, FaultReason, GuestAddress, MemoryFault, PageRange, Permissions,
    },
    process::{EngineInstance, HostError},
};

const CODE: u32 = 0x1000;
const KEY: u64 = 0x574c_4541_3031_3631;

fn code(bytes: &[u8]) -> EngineInstance {
    let mut engine = EngineInstance::new(1, KEY).unwrap();
    engine.map(CODE, 1, 7).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(bytes);
    engine.upload(CODE, bytes.len() as u32).unwrap();
    engine.protect(CODE, 1, 4).unwrap();
    engine
}

#[test]
fn word_lea_decodes_all_destinations_without_reading_data() {
    for destination in 0..8 {
        let engine = code(&[0x66, 0x8d, destination << 3 | 3]);
        let before = engine.arena().to_vec();
        let decoded = decode_one(engine.memory().unwrap(), GuestAddress(CODE))
            .expect("WORD LEA must decode without reading its memory-shaped source");
        assert_eq!(
            decoded.operation(),
            &Operation::LeaWord {
                destination: REGISTERS[usize::from(destination)],
                address: addresses()[0].1
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
        assert!(
            engine
                .memory()
                .unwrap()
                .resolve(GuestAddress(0), Access::Read)
                .is_err()
        );
    }
}

fn bound_admission(resident: bool, entries: bool) {
    let mut bytes = Vec::new();
    for destination in 0..8 {
        bytes.extend([0x66, 0x8d, destination << 3 | 3]);
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
    .expect("WORD LEA must compile bound without a data read");
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
fn word_lea_compiles_replacement_explicit() {
    bound_admission(false, false);
}

#[test]
fn word_lea_compiles_replacement_entry() {
    bound_admission(false, true);
}

#[test]
fn word_lea_compiles_resident_explicit() {
    bound_admission(true, false);
}

#[test]
fn word_lea_compiles_resident_entry() {
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

fn encoding(destination: u8, tail: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0x66, 0x8d, tail[0] | destination << 3];
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
            &[0x05, 0xff, 0xff, 0xff, 0xff],
            address(None, None, 1, u32::MAX),
        ),
        (
            &[0x84, 0xf4, 0x66, 0x67, 0xf0, 0xf3],
            address(Some(Esp), Some(Esi), 8, 0xf3f0_6766),
        ),
    ]
}

fn assert_operation(bytes: &[u8], expected: Operation) {
    let memory = code_space(CODE, bytes);
    let decoded = decode_one(&memory, GuestAddress(CODE)).unwrap();
    assert_eq!(decoded.operation(), &expected, "{bytes:02x?}");
    assert_eq!(
        (decoded.length() as usize, decoded.next_pc()),
        (bytes.len(), GuestAddress(CODE + bytes.len() as u32))
    );
    assert!(memory.is_code_current(decoded.code_snapshot()));
    assert_eq!(memory.mapped_pages(), 1);
}

#[test]
fn word_lea_all_destinations_and_address_forms_have_exact_ir() {
    let mut rows = 0;
    for (index, destination) in REGISTERS.into_iter().enumerate() {
        for (tail, address) in addresses() {
            assert_operation(
                &encoding(index as u8, tail),
                Operation::LeaWord {
                    destination,
                    address,
                },
            );
            rows += 1;
        }
    }
    assert_eq!(rows, 104);
}

#[test]
fn word_lea_original_parent_aliases_and_prefix_looking_payloads_are_literal() {
    for sib in [0x66, 0x67, 0xf0, 0xf2, 0xf3] {
        let index = (sib >> 3) & 7;
        assert_operation(
            &encoding(1, &[0x04, sib]),
            Operation::LeaWord {
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
            &[0x04, 0x85, 0x66, 0x67, 0xf0, 0xf3][..],
            EffectiveAddress {
                base: None,
                index: Some(Register32::Eax),
                scale: 4,
                displacement: 0xf3f0_6766,
                ..Default::default()
            },
        ),
        (
            &[0x04, 0x80][..],
            EffectiveAddress {
                base: Some(Register32::Eax),
                index: Some(Register32::Eax),
                scale: 4,
                displacement: 0,
                ..Default::default()
            },
        ),
    ] {
        assert_operation(
            &encoding(0, tail),
            Operation::LeaWord {
                destination: Register32::Eax,
                address,
            },
        );
    }
}

#[test]
fn word_lea_prefixes_register_sources_lock_and_dword_neighbors_stay_precise() {
    let unsupported = DecodeError::Unsupported(UnsupportedFeature::Opcode);
    for destination in 0..8 {
        let bytes = encoding(destination, &[0x03]);
        for prefix in [
            0x66, 0x67, 0xf2, 0xf3, 0xf0, 0x26, 0x2e, 0x36, 0x3e, 0x64, 0x65,
        ] {
            let expected = match prefix {
                0xf0 => DecodeError::InvalidEncoding,
                0x26 | 0x2e | 0x36 | 0x3e | 0x64 | 0x65 => {
                    DecodeError::Unsupported(UnsupportedFeature::Segment)
                }
                _ => unsupported,
            };
            for prefixed in [
                [vec![prefix], bytes.clone()].concat(),
                [vec![0x66, prefix], bytes[1..].to_vec()].concat(),
            ] {
                assert_eq!(
                    decode_one(&code_space(CODE, &prefixed), GuestAddress(CODE)).err(),
                    Some(expected),
                    "{prefixed:02x?}"
                );
            }
        }
        assert_operation(
            &bytes[1..],
            Operation::Lea {
                destination: REGISTERS[usize::from(destination)],
                address: addresses()[0].1,
            },
        );
        for source in 0..8 {
            let bytes = encoding(destination, &[0xc0 | source]);
            assert_eq!(
                decode_one(&code_space(CODE, &bytes), GuestAddress(CODE)).err(),
                Some(DecodeError::InvalidEncoding),
                "{bytes:02x?}"
            );
        }
    }
}

fn assert_six_profiles(bytes: &[u8], instructions: usize) {
    assert!(instructions <= 64);
    let memory = code_space(CODE, bytes);
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
            (1, instructions)
        );
        assert_eq!(
            &compiled.wasm_bytes(&memory).unwrap()[..8],
            b"\0asm\x01\0\0\0"
        );
    }
    for resident in [false, true] {
        for entries in [false, true] {
            assert_bound_admission(bytes, resident, entries);
        }
    }
}

#[test]
fn word_lea_all_address_banks_compile_in_six_pure_profiles_without_data_mapping() {
    let mut rows = 0;
    for first in [0, 2, 4, 6] {
        let mut bytes = Vec::new();
        for destination in first..first + 2 {
            for (tail, _) in addresses() {
                bytes.extend(encoding(destination, tail));
                rows += 1;
            }
        }
        bytes.extend([0xeb, 0]);
        assert_six_profiles(&bytes, 27);
    }
    assert_eq!(rows, 104);
}

#[test]
fn word_lea_fixed32_and_prefix_payload_adjacency_are_literal() {
    for tail in [&[0x03][..], &[0x84, 0xf4, 0x66, 0x67, 0xf0, 0xf3]] {
        let target = encoding(0, tail);
        let address = if tail.len() == 1 {
            addresses()[0].1
        } else {
            addresses()[12].1
        };
        let expected = Operation::LeaWord {
            destination: Register32::Eax,
            address,
        };
        let bytes = [vec![0x48], target.clone()].concat();
        let memory = code_space(CODE, &bytes);
        let first = decode_one(&memory, GuestAddress(CODE)).unwrap();
        assert_eq!(
            first.operation(),
            &Operation::Unary {
                kind: UnaryKind::Dec,
                destination: Location32::Register(Register32::Eax)
            }
        );
        assert_eq!(
            (first.length(), first.next_pc()),
            (1, GuestAddress(CODE + 1))
        );
        let next = decode_one(&memory, first.next_pc()).unwrap();
        assert_eq!(next.operation(), &expected);
        assert_eq!(
            (next.length() as usize, next.next_pc()),
            (target.len(), GuestAddress(CODE + 1 + target.len() as u32))
        );
        for payload in [[0x66, 0x67, 0xf0, 0xf3], [0xf2, 0x8d, 0x66, 0x67]] {
            let bytes = [vec![0xb8], payload.to_vec(), target.clone()].concat();
            let memory = code_space(CODE, &bytes);
            let first = decode_one(&memory, GuestAddress(CODE)).unwrap();
            assert_eq!(
                first.operation(),
                &Operation::Move {
                    destination: Location32::Register(Register32::Eax),
                    source: Value32::Immediate(u32::from_le_bytes(payload))
                }
            );
            assert_eq!(
                (first.length(), first.next_pc()),
                (5, GuestAddress(CODE + 5))
            );
            let next = decode_one(&memory, first.next_pc()).unwrap();
            assert_eq!(next.operation(), &expected);
            assert_eq!(
                (next.length() as usize, next.next_pc()),
                (target.len(), GuestAddress(CODE + 5 + target.len() as u32))
            );
            assert!(memory.is_code_current(next.code_snapshot()));
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
fn word_lea_fetch_checks_three_and_eight_byte_page_top_execute_boundaries() {
    for tail in [&[0x03][..], &[0x84, 0xf4, 0x66, 0x67, 0xf0, 0xf3]] {
        let bytes = encoding(0, tail);
        assert_eq!(bytes.len(), if tail.len() == 1 { 3 } else { 8 });
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
        let mut memory = code_space(CODE, &bytes);
        memory
            .protect(
                PageRange::new(GuestAddress(CODE), 1).unwrap(),
                Permissions::READ,
            )
            .unwrap();
        assert_eq!(
            decode_one(&memory, GuestAddress(CODE)).err(),
            Some(fetch_error(CODE, CODE, FaultReason::Permission, 1))
        );
    }
}

#[test]
fn word_lea_mixed_64_instruction_block_fits_all_six_profiles() {
    let mut bytes = Vec::new();
    for destination in 0..8 {
        for (tail, _) in addresses().into_iter().take(4) {
            bytes.extend(encoding(destination, tail));
        }
    }
    bytes.extend(std::iter::repeat_n(0x90, 31));
    bytes.extend([0xeb, 0]);
    assert_six_profiles(&bytes, 64);
}

#[test]
fn word_lea_late_refusal_preserves_full_arena_and_both_owners() {
    let mut bytes = encoding(0, &[0x04, 0x80]);
    bytes.extend(encoding(4, &[0x84, 0xf4, 0x66, 0x67, 0xf0, 0xf3]));
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
            assert_eq!(before.len(), 4612);
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
