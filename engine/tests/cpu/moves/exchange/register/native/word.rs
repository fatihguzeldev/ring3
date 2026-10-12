use super::support_word_prefix as word_prefix;
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
            ir::{Location32, Operation, UnaryKind, Value32},
        },
    },
    memory::{
        Access, AddressSpace, FaultReason, GuestAddress, MemoryFault, PageRange, Permissions,
    },
    process::{EngineInstance, HostError},
};

const CODE: u32 = 0x1000;
const KEY: u64 = 0x5752_4547_5843_4831;

fn code(bytes: &[u8]) -> EngineInstance {
    let mut engine = EngineInstance::new(1, KEY).unwrap();
    engine.map(CODE, 1, 7).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(bytes);
    engine.upload(CODE, bytes.len() as u32).unwrap();
    engine.protect(CODE, 1, 4).unwrap();
    engine
}

fn decode_admission(bytes: &[u8]) {
    let engine = code(bytes);
    let before = engine.arena().to_vec();
    let decoded = decode_one(engine.memory().unwrap(), GuestAddress(CODE))
        .expect("WORD register exchange family must decode");
    assert_eq!(decoded.operation(), &expected(bytes));
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
fn word_register_exchange_modrm_decodes() {
    decode_admission(&[0x66, 0x87, 0xc8]);
}

#[test]
fn word_register_exchange_short_decodes() {
    decode_admission(&[0x66, 0x97]);
}

#[test]
fn word_register_exchange_nop_alias_decodes() {
    decode_admission(&[0x66, 0x90]);
}

fn bound_admission(resident: bool, entries: bool) {
    let bytes = [0x66, 0x87, 0xc8, 0x66, 0x97, 0x66, 0x90, 0xeb, 0];
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
    .expect("WORD register exchange family must compile bound");
    let module = if resident {
        engine.resident_bytes(id).unwrap()
    } else {
        engine.artifact_bytes().unwrap()
    };
    assert_eq!(&module[..8], b"\0asm\x01\0\0\0");
    assert_eq!(engine.arena(), before);
}

#[test]
fn word_register_exchange_compiles_replacement_explicit() {
    bound_admission(false, false);
}

#[test]
fn word_register_exchange_compiles_replacement_entry() {
    bound_admission(false, true);
}

#[test]
fn word_register_exchange_compiles_resident_explicit() {
    bound_admission(true, false);
}

#[test]
fn word_register_exchange_compiles_resident_entry() {
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

fn expected(bytes: &[u8]) -> Operation {
    match bytes {
        [0x66, 0x87, modrm] => Operation::ExchangeWord {
            left: REGISTERS[usize::from(modrm & 7)],
            right: REGISTERS[usize::from((modrm >> 3) & 7)],
        },
        [0x66, 0x90] => Operation::Nop,
        [0x66, opcode @ 0x91..=0x97] => Operation::ExchangeWord {
            left: REGISTERS[usize::from(opcode - 0x90)],
            right: Register32::Eax,
        },
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

fn assert_six_profiles(bytes: &[u8], instructions: usize) {
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
fn word_register_exchange_all_modrm_pairs_and_short_forms_have_exact_ir() {
    let mut rows = 0;
    for (left, left_register) in REGISTERS.into_iter().enumerate() {
        for (right, right_register) in REGISTERS.into_iter().enumerate() {
            let bytes = [0x66, 0x87, 0xc0 | (right as u8) << 3 | left as u8];
            let memory = code_space(CODE, &bytes);
            let decoded = decode_one(&memory, GuestAddress(CODE)).unwrap();
            assert_eq!(
                decoded.operation(),
                &Operation::ExchangeWord {
                    left: left_register,
                    right: right_register
                }
            );
            assert_eq!(
                (decoded.length(), decoded.next_pc()),
                (3, GuestAddress(CODE + 3))
            );
            assert!(memory.is_code_current(decoded.code_snapshot()));
            rows += 1;
        }
    }
    for (index, register) in REGISTERS.into_iter().enumerate() {
        let bytes = [0x66, 0x90 + index as u8];
        let memory = code_space(CODE, &bytes);
        let decoded = decode_one(&memory, GuestAddress(CODE)).unwrap();
        assert_eq!(
            decoded.operation(),
            &if index == 0 {
                Operation::Nop
            } else {
                Operation::ExchangeWord {
                    left: register,
                    right: Register32::Eax,
                }
            }
        );
        assert_eq!(
            (decoded.length(), decoded.next_pc()),
            (2, GuestAddress(CODE + 2))
        );
        assert!(memory.is_code_current(decoded.code_snapshot()));
        rows += 1;
    }
    assert_eq!(rows, 72);
}

#[test]
fn word_register_exchange_matrix_banks_compile_in_all_six_profiles() {
    for first in [0, 32] {
        let mut bytes = Vec::new();
        for ordinal in first..first + 32 {
            bytes.extend([0x66, 0x87, 0xc0 | ordinal]);
        }
        if first == 32 {
            for opcode in 0x90..=0x97 {
                bytes.extend([0x66, opcode]);
            }
        }
        bytes.extend([0xeb, 0]);
        assert_six_profiles(&bytes, if first == 0 { 33 } else { 41 });
    }
}

#[test]
fn word_register_exchange_prefixes_memory_lock_and_multibyte_nop_stay_precise() {
    let unsupported = DecodeError::Unsupported(UnsupportedFeature::Opcode);
    for bytes in [vec![0x66, 0x87, 0xc8], vec![0x66, 0x97], vec![0x66, 0x90]] {
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
                if matches!(prefix, 0x67 | 0x26 | 0x2e | 0x36 | 0x3e | 0x64 | 0x65) {
                    word_prefix::admitted(&prefixed);
                    continue;
                }
                if !word_prefix::admit_if_new_prefix(&prefixed) {
                    assert_eq!(
                        decode_one(&code_space(CODE, &prefixed), GuestAddress(CODE)).err(),
                        Some(expected),
                        "{prefixed:02x?}"
                    );
                }
            }
        }
        let memory = code_space(CODE, &bytes[1..]);
        assert!(matches!(
            decode_one(&memory, GuestAddress(CODE)).unwrap().operation(),
            Operation::Exchange { .. } | Operation::Nop
        ));
    }
    for tail in [
        &[0x03][..],
        &[0x04, 0x24],
        &[0x45, 0],
        &[0x43, 0x80],
        &[0x83, 0x66, 0x67, 0xf0, 0xf3],
        &[0x04, 0x8b],
        &[0x04, 0x8d, 0x66, 0x67, 0xf0, 0xf3],
        &[0x05, 0x66, 0x67, 0xf0, 0xf3],
    ] {
        let mut bytes = vec![0x66, 0x87, tail[0] | 1 << 3];
        bytes.extend_from_slice(&tail[1..]);
        for prefixed in [
            [vec![0xf0], bytes[1..].to_vec()].concat(),
            [vec![0xf0], bytes].concat(),
        ] {
            let (operation, length) = word_prefix::admitted(&prefixed);
            assert_eq!(length, prefixed.len());
            assert!(matches!(
                operation,
                Operation::MemoryExchange { .. } | Operation::MemoryExchangeWord { .. }
            ));
        }
    }
    for bytes in [
        &[0x66, 0x86, 0xc8][..],
        &[0x66, 0x0f, 0x1f, 0x00],
        &[0x66, 0x0f, 0x1f, 0xc0],
        &[0x66, 0x0f, 0x1f, 0x44, 0x00, 1],
    ] {
        if !word_prefix::admit_if_new_prefix(bytes) {
            assert_eq!(
                decode_one(&code_space(CODE, bytes), GuestAddress(CODE)).err(),
                Some(unsupported),
                "{bytes:02x?}"
            );
        }
    }
}

#[test]
fn word_register_exchange_fixed32_and_prefix_payload_adjacency_are_literal() {
    for target in [vec![0x66, 0x87, 0xc8], vec![0x66, 0x97], vec![0x66, 0x90]] {
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
        assert_eq!(next.operation(), &expected(&target));
        assert_eq!(
            (next.length() as usize, next.next_pc()),
            (target.len(), GuestAddress(CODE + 1 + target.len() as u32))
        );
        for payload in [[0x66, 0x67, 0xf0, 0xf3], [0xf2, 0x87, 0x90, 0x97]] {
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
            assert_eq!(next.operation(), &expected(&target));
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
fn word_register_exchange_fetch_checks_two_and_three_byte_page_top_execute_boundaries() {
    for bytes in [vec![0x66, 0x87, 0xc8], vec![0x66, 0x97], vec![0x66, 0x90]] {
        for pc in [0x1fff, u32::MAX - bytes.len() as u32 + 1] {
            let memory = code_space(pc, &bytes);
            let decoded = decode_one(&memory, GuestAddress(pc)).unwrap();
            assert_eq!(decoded.operation(), &expected(&bytes));
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
fn word_register_exchange_mixed_64_instruction_block_fits_all_six_profiles() {
    let mut bytes = Vec::new();
    for i in 0..32 {
        bytes.extend([0x66, 0x87, 0xc0 | i]);
    }
    bytes.extend(std::iter::repeat_n(0x90, 31));
    bytes.extend([0xeb, 0]);
    assert_six_profiles(&bytes, 64);
}

#[test]
fn word_register_exchange_late_refusal_preserves_full_arena_and_both_owners() {
    let mut bytes = vec![0x66, 0x87, 0xc8, 0x66, 0x97, 0x66, 0x90];
    let refused_pc = CODE + bytes.len() as u32;
    bytes.extend([0xf0, 0x90]);
    let refused_length = bytes.len() as u32;
    bytes.resize(256, 0x90);
    bytes.extend([0xeb, 0]);
    let mut engine = code(&bytes);
    assert_eq!(engine.arena().len(), ring3_engine::abi::arena::ARENA_SIZE);
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
        cause: InstructionError::Decode(DecodeError::InvalidEncoding),
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
