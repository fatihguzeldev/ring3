use super::support_word_prefix as word_prefix;
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
            ir::{BitIndex, BitTestKind, EffectiveAddress, Operation},
        },
    },
    memory::{
        Access, AddressSpace, FaultReason, GuestAddress, MemoryFault, PageRange, Permissions,
    },
    process::{EngineInstance, HostError},
};

const CODE: u32 = 0x1000;
const KEY: u64 = 0x5742_4d45_4d31_3631;
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

fn instruction(opcode: u8, extension: u8, immediate: bool) -> Vec<u8> {
    if immediate {
        vec![0x66, 0x0f, 0xba, extension << 3 | 3, 0xf3]
    } else {
        vec![0x66, 0x0f, opcode, 1 << 3 | 3]
    }
}

fn decode_admission(immediate: bool) {
    for (opcode, extension) in FORMS {
        let bytes = instruction(opcode, extension, immediate);
        let engine = code(&bytes);
        let before = engine.arena().to_vec();
        let decoded = decode_one(engine.memory().unwrap(), GuestAddress(CODE))
            .expect("WORD memory bit test family must decode without mapped data");
        assert_eq!(
            decoded.operation(),
            &operation(
                opcode,
                addresses()[0].1,
                if immediate {
                    BitIndex::Immediate(0xf3)
                } else {
                    BitIndex::Register(Register32::Ecx)
                },
            )
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

#[test]
fn word_memory_bit_test_register_index_forms_decode() {
    decode_admission(false);
}

#[test]
fn word_memory_bit_test_immediate_index_forms_decode() {
    decode_admission(true);
}

fn bank() -> Vec<u8> {
    let mut bytes = Vec::new();
    for (opcode, extension) in FORMS {
        bytes.extend(instruction(opcode, extension, false));
        bytes.extend(instruction(opcode, extension, true));
    }
    bytes.extend([0xeb, 0]);
    bytes
}

fn bound_admission(resident: bool, entries: bool) {
    let bytes = bank();
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
    .expect("WORD memory bit test family must compile bound without mapped data");
    let module = if resident {
        engine.resident_bytes(id).unwrap()
    } else {
        engine.artifact_bytes().unwrap()
    };
    assert_eq!(&module[..8], b"\0asm\x01\0\0\0");
    assert_eq!(engine.arena(), before);
}

#[test]
fn word_memory_bit_test_compiles_replacement_explicit() {
    bound_admission(false, false);
}

#[test]
fn word_memory_bit_test_compiles_replacement_entry() {
    bound_admission(false, true);
}

#[test]
fn word_memory_bit_test_compiles_resident_explicit() {
    bound_admission(true, false);
}

#[test]
fn word_memory_bit_test_compiles_resident_entry() {
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

fn kind(opcode: u8) -> BitTestKind {
    match opcode {
        0xa3 => BitTestKind::Test,
        0xab => BitTestKind::Set,
        0xb3 => BitTestKind::Reset,
        0xbb => BitTestKind::Complement,
        _ => unreachable!(),
    }
}

fn operation(opcode: u8, address: EffectiveAddress, index: BitIndex) -> Operation {
    if opcode == 0xa3 {
        Operation::ReadBitTestWord { address, index }
    } else {
        Operation::MemoryBitMutationWord {
            kind: kind(opcode),
            address,
            index,
        }
    }
}

fn encoded(opcode: u8, extension: u8, index: BitIndex, tail: &[u8]) -> Vec<u8> {
    let field = match index {
        BitIndex::Register(source) => REGISTERS
            .iter()
            .position(|candidate| *candidate == source)
            .unwrap() as u8,
        BitIndex::Immediate(_) => extension,
    };
    let mut bytes = vec![
        0x66,
        0x0f,
        if matches!(index, BitIndex::Immediate(_)) {
            0xba
        } else {
            opcode
        },
        tail[0] | field << 3,
    ];
    bytes.extend_from_slice(&tail[1..]);
    if let BitIndex::Immediate(raw) = index {
        bytes.push(raw);
    }
    bytes
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
fn word_memory_bit_test_all_register_indexes_immediates_and_addresses_have_exact_ir() {
    let (mut registers, mut immediates) = (0, 0);
    for (opcode, extension) in FORMS {
        for (tail, address) in addresses() {
            for source in REGISTERS {
                let index = BitIndex::Register(source);
                assert_operation(
                    &encoded(opcode, extension, index, tail),
                    operation(opcode, address, index),
                );
                registers += 1;
            }
            let index = BitIndex::Immediate(0xf3);
            assert_operation(
                &encoded(opcode, extension, index, tail),
                operation(opcode, address, index),
            );
            immediates += 1;
        }
    }
    assert_eq!((registers, immediates), (416, 52));
}

#[test]
fn word_memory_bit_test_raw_indexes_and_prefix_looking_address_payloads_stay_literal() {
    for (opcode, extension) in FORMS {
        for raw in [
            0, 1, 15, 16, 17, 31, 32, 33, 0x66, 0x67, 0xf0, 0xf2, 0xf3, 255,
        ] {
            let index = BitIndex::Immediate(raw);
            assert_operation(
                &encoded(opcode, extension, index, &[0x03]),
                operation(opcode, addresses()[0].1, index),
            );
        }
        for index in [
            BitIndex::Register(Register32::Ecx),
            BitIndex::Immediate(0xf3),
        ] {
            assert_operation(
                &encoded(opcode, extension, index, &[0x04, 0x89]),
                operation(
                    opcode,
                    EffectiveAddress {
                        base: Some(Register32::Ecx),
                        index: Some(Register32::Ecx),
                        scale: 4,
                        displacement: 0,
                        ..Default::default()
                    },
                    index,
                ),
            );
            for sib in [0x66, 0x67, 0xf0, 0xf2, 0xf3] {
                let index_field = (sib >> 3) & 7;
                let address = EffectiveAddress {
                    base: Some(REGISTERS[usize::from(sib & 7)]),
                    index: (index_field != 4).then(|| REGISTERS[usize::from(index_field)]),
                    scale: 1 << (sib >> 6),
                    displacement: 0xffff_ff80,
                    ..Default::default()
                };
                assert_operation(
                    &encoded(opcode, extension, index, &[0x44, sib, 0x80]),
                    operation(opcode, address, index),
                );
            }
        }
        let index = BitIndex::Register(Register32::Eax);
        assert_operation(
            &encoded(opcode, extension, index, &[0x04, 0x80]),
            operation(
                opcode,
                EffectiveAddress {
                    base: Some(Register32::Eax),
                    index: Some(Register32::Eax),
                    scale: 4,
                    displacement: 0,
                    ..Default::default()
                },
                index,
            ),
        );
    }
}

#[test]
fn word_memory_bit_test_prefixes_reserved_fields_and_admitted_neighbors_are_precise() {
    let unsupported = DecodeError::Unsupported(UnsupportedFeature::Opcode);
    for (opcode, extension) in FORMS {
        for index in [
            BitIndex::Register(Register32::Ecx),
            BitIndex::Immediate(0xf3),
        ] {
            let bytes = encoded(opcode, extension, index, &[0x03]);
            for prefix in [0x66, 0x67, 0xf2, 0xf3, 0xf0] {
                let prefixed = [vec![prefix], bytes.clone()].concat();
                if prefix == 0x67 || prefix == 0xf0 && opcode != 0xa3 {
                    word_prefix::admitted(&prefixed);
                    continue;
                }
                assert_eq!(
                    decode_one(&code_space(CODE, &prefixed), GuestAddress(CODE)).err(),
                    Some(if prefix == 0xf0 && opcode == 0xa3 {
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
            let memory = code_space(CODE, &bytes[1..]);
            assert!(matches!(
                decode_one(&memory, GuestAddress(CODE)).unwrap().operation(),
                Operation::ReadBitTest { .. } | Operation::MemoryBitMutation { .. }
            ));
            let register = encoded(opcode, extension, index, &[0xc3]);
            assert_operation(
                &register,
                Operation::BitTestWord {
                    kind: kind(opcode),
                    destination: Register32::Ebx,
                    index,
                },
            );
            let locked = [vec![0xf0], register].concat();
            assert_eq!(
                decode_one(&code_space(CODE, &locked), GuestAddress(CODE)).err(),
                Some(DecodeError::InvalidEncoding)
            );
        }
    }
    for extension in 0..4 {
        for modrm in [0x03, 0xc3] {
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
fn decoded_word_memory_bit_tests_stay_closed_in_standalone_backend() {
    for (opcode, extension) in FORMS {
        for immediate in [false, true] {
            let mut bytes = instruction(opcode, extension, immediate);
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
fn word_memory_bit_test_fetch_checks_every_address_and_raw_immediate_byte() {
    for (opcode, extension) in FORMS {
        for index in [
            BitIndex::Register(Register32::Ecx),
            BitIndex::Immediate(0xf3),
        ] {
            let bytes = encoded(opcode, extension, index, addresses()[12].0);
            assert_eq!(
                bytes.len(),
                if matches!(index, BitIndex::Immediate(_)) {
                    10
                } else {
                    9
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
                    Some(fetch_error(pc, 0x2000, FaultReason::Unmapped, cut + 1)),
                    "{bytes:02x?} cut={cut}"
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
fn word_memory_bit_test_mixed_64_instruction_block_fits_every_bound_profile() {
    let once = bank();
    let mut bytes = once[..once.len() - 2].repeat(4);
    bytes.extend(std::iter::repeat_n(0x90, 31));
    bytes.extend([0xeb, 0]);
    for resident in [false, true] {
        for entries in [false, true] {
            assert_bound_admission(&bytes, resident, entries);
        }
    }
}

#[test]
fn word_memory_bit_test_late_refusal_preserves_full_arena_and_both_owners() {
    let admitted = bank();
    let mut bytes = admitted[..admitted.len() - 2].to_vec();
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
