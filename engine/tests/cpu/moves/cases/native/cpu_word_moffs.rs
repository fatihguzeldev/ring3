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
            ir::{
                ByteRegister, ByteValue, EffectiveAddress, Location32, Operation, UnaryKind,
                Value32, WordValue,
            },
        },
    },
    memory::{
        Access, AddressSpace, FaultReason, GuestAddress, MemoryFault, PageRange, Permissions,
    },
    process::{EngineInstance, HostError},
};

const CODE: u32 = 0x1000;
const KEY: u64 = 0x574d_4f46_4653_3136;

fn code(bytes: &[u8]) -> EngineInstance {
    let mut engine = EngineInstance::new(1, KEY).unwrap();
    engine.map(CODE, 1, 7).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(bytes);
    engine.upload(CODE, bytes.len() as u32).unwrap();
    engine.protect(CODE, 1, 4).unwrap();
    engine
}

fn decode_admission(opcode: u8) {
    let engine = code(&[0x66, opcode, 0x66, 0x67, 0xf0, 0xf3]);
    let before = engine.arena().to_vec();
    let decoded = decode_one(engine.memory().unwrap(), GuestAddress(CODE))
        .expect("WORD accumulator moffs32 must decode without accessing its absolute operand");
    assert_eq!(decoded.operation(), &operation(opcode, 0xf3f0_6766));
    assert_eq!(
        (decoded.length(), decoded.next_pc()),
        (6, GuestAddress(CODE + 6))
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
            .resolve(GuestAddress(0xf3f0_6766), Access::Read)
            .is_err()
    );
}

#[test]
fn word_moffs_load_decodes_absolute32_operand() {
    decode_admission(0xa1);
}

#[test]
fn word_moffs_store_decodes_absolute32_operand() {
    decode_admission(0xa3);
}

fn bound_admission(resident: bool, entries: bool) {
    let bytes = [
        0x66, 0xa1, 0, 0x50, 0, 0, 0x66, 0xa3, 0xff, 0xff, 0xff, 0xff, 0xeb, 0,
    ];
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
    .expect("WORD accumulator moffs32 must compile bound without data mapping");
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
fn word_moffs_compiles_replacement_explicit() {
    bound_admission(false, false);
}

#[test]
fn word_moffs_compiles_replacement_entry() {
    bound_admission(false, true);
}

#[test]
fn word_moffs_compiles_resident_explicit() {
    bound_admission(true, false);
}

#[test]
fn word_moffs_compiles_resident_entry() {
    bound_admission(true, true);
}

const ADDRESSES: [u32; 10] = [
    0,
    1,
    0x5010,
    0x5fff,
    0x5678,
    0x9234_5678,
    0x8000_0000,
    0xffff_fffe,
    u32::MAX,
    0xf3f0_6766,
];

fn absolute(address: u32) -> EffectiveAddress {
    EffectiveAddress {
        base: None,
        index: None,
        scale: 1,
        displacement: address,
        ..Default::default()
    }
}

fn operation(opcode: u8, address: u32) -> Operation {
    if opcode == 0xa1 {
        Operation::LoadWord {
            destination: Register32::Eax,
            address: absolute(address),
        }
    } else {
        Operation::StoreWord {
            address: absolute(address),
            source: WordValue::Register(Register32::Eax),
        }
    }
}

fn encoding(opcode: u8, address: u32) -> Vec<u8> {
    let mut bytes = vec![0x66, opcode];
    bytes.extend(address.to_le_bytes());
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
fn word_moffs_full32_absolute_displacements_and_prefix_looking_payloads_have_exact_ir() {
    let mut rows = 0;
    for opcode in [0xa1, 0xa3] {
        for address in ADDRESSES {
            assert_operation(&encoding(opcode, address), operation(opcode, address));
            rows += 1;
        }
        for payload in [
            [0x66, 0x67, 0xf0, 0xf3],
            [0xf2, 0xa1, 0x66, 0xa3],
            [0x67, 0x66, 0xf3, 0xf0],
        ] {
            let address = u32::from_le_bytes(payload);
            assert_operation(&encoding(opcode, address), operation(opcode, address));
        }
    }
    assert_eq!(rows, 20);
}

#[test]
fn word_moffs_prefixes_lock_and_ignored_byte_width_neighbors_stay_precise() {
    for opcode in [0xa1, 0xa3] {
        let bytes = encoding(opcode, 0xf3f0_6766);
        for prefix in [
            0x66, 0x67, 0xf2, 0xf3, 0xf0, 0x26, 0x2e, 0x36, 0x3e, 0x64, 0x65,
        ] {
            let expected = match prefix {
                0xf0 => DecodeError::InvalidEncoding,
                0x26 | 0x2e | 0x36 | 0x3e | 0x64 | 0x65 => {
                    DecodeError::Unsupported(UnsupportedFeature::Segment)
                }
                _ => DecodeError::Unsupported(UnsupportedFeature::Opcode),
            };
            for prefixed in [
                [vec![prefix], bytes.clone()].concat(),
                [vec![0x66, prefix], bytes[1..].to_vec()].concat(),
            ] {
                if matches!(prefix, 0x67 | 0x26 | 0x2e | 0x36 | 0x3e | 0x64 | 0x65) {
                    word_prefix::admitted(&prefixed);
                    continue;
                }
                assert_eq!(
                    decode_one(&code_space(CODE, &prefixed), GuestAddress(CODE)).err(),
                    Some(expected),
                    "{prefixed:02x?}"
                );
            }
        }
        let bytes = encoding(opcode - 1, 0xf3f0_6766);
        assert_eq!(
            decode_one(&code_space(CODE, &bytes), GuestAddress(CODE)).err(),
            Some(DecodeError::Unsupported(UnsupportedFeature::Opcode)),
            "{bytes:02x?}"
        );
    }
}

#[test]
fn word_moffs_byte_dword_modrm_and_fixed32_payload_neighbors_are_literal() {
    for opcode in [0xa1, 0xa3] {
        let address = 0xf3f0_6766;
        let bytes = encoding(opcode, address);
        let expected = operation(opcode, address);
        let dword = if opcode == 0xa1 {
            Operation::Move {
                destination: Location32::Register(Register32::Eax),
                source: Value32::Memory(absolute(address)),
            }
        } else {
            Operation::Move {
                destination: Location32::Memory(absolute(address)),
                source: Value32::Register(Register32::Eax),
            }
        };
        assert_operation(&bytes[1..], dword);
        let mut byte = vec![opcode - 1];
        byte.extend(address.to_le_bytes());
        assert_operation(
            &byte,
            if opcode == 0xa1 {
                Operation::LoadByte {
                    destination: ByteRegister::Al,
                    address: absolute(address),
                }
            } else {
                Operation::StoreByte {
                    address: absolute(address),
                    source: ByteValue::Register(ByteRegister::Al),
                }
            },
        );
        let mut modrm = vec![0x66, if opcode == 0xa1 { 0x8b } else { 0x89 }, 0x05];
        modrm.extend(address.to_le_bytes());
        assert_operation(&modrm, expected);
        let fixed = [vec![0x48], bytes.clone()].concat();
        let memory = code_space(CODE, &fixed);
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
        assert_eq!((next.length(), next.next_pc()), (6, GuestAddress(CODE + 7)));
        for payload in [[0x66, 0x67, 0xf0, 0xf3], [0xf2, 0xa1, 0x66, 0xa3]] {
            let literal = [vec![0xb8], payload.to_vec(), bytes.clone()].concat();
            let memory = code_space(CODE, &literal);
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
                (next.length(), next.next_pc()),
                (6, GuestAddress(CODE + 11))
            );
        }
    }
}

#[test]
fn word_moffs_full_address_banks_compile_in_four_bound_profiles_without_data_access() {
    let mut bytes = Vec::new();
    for opcode in [0xa1, 0xa3] {
        for address in ADDRESSES {
            bytes.extend(encoding(opcode, address));
        }
    }
    bytes.extend([0xeb, 0]);
    for resident in [false, true] {
        for entries in [false, true] {
            assert_bound_admission(&bytes, resident, entries);
        }
    }
}

#[test]
fn decoded_word_moffs_stay_closed_in_all_four_standalone_apis() {
    for opcode in [0xa1, 0xa3] {
        let bytes = [vec![0x90], encoding(opcode, 0xf3f0_6766), vec![0xeb, 0]].concat();
        let memory = code_space(CODE, &bytes);
        let snapshot = memory
            .snapshot_code(GuestAddress(CODE), bytes.len())
            .unwrap();
        let specs = [BlockSpec {
            entry: GuestAddress(CODE),
            byte_length: bytes.len() as u32,
        }];
        let expected = Some(CompileError::Instruction {
            pc: GuestAddress(CODE + 1),
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
        assert!(memory.is_code_current(&snapshot));
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
fn word_moffs_fetch_checks_all_six_byte_page_top_and_execute_boundaries() {
    for opcode in [0xa1, 0xa3] {
        let bytes = encoding(opcode, 0xf3f0_6766);
        assert_eq!(bytes.len(), 6);
        for pc in [0x1fff, u32::MAX - 5] {
            let memory = code_space(pc, &bytes);
            let decoded = decode_one(&memory, GuestAddress(pc)).unwrap();
            assert_eq!(decoded.operation(), &operation(opcode, 0xf3f0_6766));
            assert_eq!(
                (decoded.length(), decoded.next_pc()),
                (6, GuestAddress(pc.wrapping_add(6)))
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
        let pc = 0x1ffb;
        let mut memory = code_space(pc, &bytes);
        memory
            .protect(
                PageRange::new(GuestAddress(0x2000), 1).unwrap(),
                Permissions::READ,
            )
            .unwrap();
        assert_eq!(
            decode_one(&memory, GuestAddress(pc)).err(),
            Some(fetch_error(pc, 0x2000, FaultReason::Permission, 6))
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
fn word_moffs_mixed_64_instruction_block_fits_four_bound_profiles() {
    let mut bytes = Vec::new();
    for _ in 0..8 {
        bytes.extend(encoding(0xa1, 0x5010));
        bytes.extend(encoding(0xa3, u32::MAX));
    }
    bytes.extend(std::iter::repeat_n(0x90, 47));
    bytes.extend([0xeb, 0]);
    for resident in [false, true] {
        for entries in [false, true] {
            assert_bound_admission(&bytes, resident, entries);
        }
    }
}

#[test]
fn word_moffs_late_refusal_preserves_full_arena_and_both_owners() {
    let mut bytes = encoding(0xa1, 0xf3f0_6766);
    bytes.extend(encoding(0xa3, u32::MAX));
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
