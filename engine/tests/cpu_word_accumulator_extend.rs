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
            ir::{
                ExtensionKind, Location32, Operation, SmallSource, SmallWidth, UnaryKind, Value32,
            },
        },
    },
    memory::{
        Access, AddressSpace, FaultReason, GuestAddress, MemoryFault, PageRange, Permissions,
    },
    process::{EngineInstance, HostError},
};

const CODE: u32 = 0x1000;
const KEY: u64 = 0x5741_4343_4558_5431;

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
    let bytes = [0x66, opcode];
    let engine = code(&bytes);
    let before = engine.arena().to_vec();
    let decoded = decode_one(engine.memory().unwrap(), GuestAddress(CODE))
        .expect("WORD accumulator sign extension must decode");
    assert_eq!(decoded.operation(), &operation(opcode));
    assert_eq!(
        (decoded.length(), decoded.next_pc()),
        (2, GuestAddress(CODE + 2))
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
fn cbw_decodes_exact_word_accumulator_form() {
    decode_admission(0x98);
}

#[test]
fn cwd_decodes_exact_word_accumulator_form() {
    decode_admission(0x99);
}

fn bound_admission(resident: bool, entries: bool) {
    let bytes = [0x66, 0x98, 0x66, 0x99, 0xeb, 0];
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
    .expect("CBW and CWD must compile bound without mapped data");
    let module = if resident {
        engine.resident_bytes(id).unwrap()
    } else {
        engine.artifact_bytes().unwrap()
    };
    assert_eq!(&module[..8], b"\0asm\x01\0\0\0");
    assert_eq!(engine.arena(), before);
}

#[test]
fn word_accumulator_extend_compiles_replacement_explicit() {
    bound_admission(false, false);
}

#[test]
fn word_accumulator_extend_compiles_replacement_entry() {
    bound_admission(false, true);
}

#[test]
fn word_accumulator_extend_compiles_resident_explicit() {
    bound_admission(true, false);
}

#[test]
fn word_accumulator_extend_compiles_resident_entry() {
    bound_admission(true, true);
}

fn operation(opcode: u8) -> Operation {
    match opcode {
        0x98 => Operation::SignExtendAccumulatorWord,
        0x99 => Operation::SignExtendHighWord,
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
fn word_accumulator_extend_exact_ir_and_fixed32_payload_adjacency_are_literal() {
    let bytes = [0x48, 0x66, 0x98, 0x66, 0x99];
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
    let second = decode_one(&memory, first.next_pc()).unwrap();
    assert_eq!(second.operation(), &Operation::SignExtendAccumulatorWord);
    assert_eq!(
        (second.length(), second.next_pc()),
        (2, GuestAddress(CODE + 3))
    );
    let third = decode_one(&memory, second.next_pc()).unwrap();
    assert_eq!(third.operation(), &Operation::SignExtendHighWord);
    assert_eq!(
        (third.length(), third.next_pc()),
        (2, GuestAddress(CODE + 5))
    );
    for opcode in [0x98, 0x99] {
        for payload in [[0x66, 0x67, 0xf0, 0xf3], [0xf2, 0x98, 0x99, 0x66]] {
            let bytes = [vec![0xb8], payload.to_vec(), vec![0x66, opcode]].concat();
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
            assert_eq!(next.operation(), &operation(opcode));
            assert_eq!((next.length(), next.next_pc()), (2, GuestAddress(CODE + 7)));
            assert!(memory.is_code_current(next.code_snapshot()));
        }
    }
}

#[test]
fn word_accumulator_extend_compact_bank_compiles_in_all_six_profiles() {
    assert_six_profiles(&[0x66, 0x98, 0x66, 0x99, 0xeb, 0], 3);
}

#[test]
fn word_accumulator_extend_strict_prefixes_and_dword_neighbors_keep_categories() {
    for opcode in [0x98, 0x99] {
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
            for bytes in [[prefix, 0x66, opcode], [0x66, prefix, opcode]] {
                assert_eq!(
                    decode_one(&code_space(CODE, &bytes), GuestAddress(CODE)).err(),
                    Some(expected),
                    "{bytes:02x?}"
                );
            }
        }
        let memory = code_space(CODE, &[opcode]);
        let decoded = decode_one(&memory, GuestAddress(CODE)).unwrap();
        assert_eq!(
            decoded.operation(),
            &if opcode == 0x98 {
                Operation::Extend {
                    kind: ExtensionKind::Sign,
                    destination: Register32::Eax,
                    source: SmallSource::Register {
                        register: Register32::Eax,
                        width: SmallWidth::Word,
                        high_byte: false,
                    },
                }
            } else {
                Operation::SignExtendHigh
            }
        );
        assert_eq!(
            (decoded.length(), decoded.next_pc()),
            (1, GuestAddress(CODE + 1))
        );
    }
}

fn fetch_error(pc: u32, address: u32, reason: FaultReason, length: u32) -> DecodeError {
    DecodeError::MemoryFault {
        pc: GuestAddress(pc),
        fault: MemoryFault {
            address: GuestAddress(address),
            access: Access::Execute,
            reason,
        },
        length,
    }
}

#[test]
fn word_accumulator_extend_two_byte_fetch_preserves_page_top_and_execute_boundaries() {
    for opcode in [0x98, 0x99] {
        let bytes = [0x66, opcode];
        for pc in [0x1fff, u32::MAX - 1] {
            let memory = code_space(pc, &bytes);
            let decoded = decode_one(&memory, GuestAddress(pc)).unwrap();
            assert_eq!(decoded.operation(), &operation(opcode));
            assert_eq!(
                (decoded.length(), decoded.next_pc()),
                (2, GuestAddress(pc.wrapping_add(2)))
            );
            assert!(memory.is_code_current(decoded.code_snapshot()));
        }
        for (pc, address, reason) in [
            (0x1fff, 0x2000, FaultReason::Unmapped),
            (u32::MAX, u32::MAX, FaultReason::AddressOverflow),
        ] {
            let memory = code_space(pc, &[0x66]);
            assert_eq!(
                decode_one(&memory, GuestAddress(pc)).err(),
                Some(fetch_error(pc, address, reason, 2))
            );
        }
        let mut memory = code_space(0x1fff, &bytes);
        memory
            .protect(
                PageRange::new(GuestAddress(0x2000), 1).unwrap(),
                Permissions::READ,
            )
            .unwrap();
        assert_eq!(
            decode_one(&memory, GuestAddress(0x1fff)).err(),
            Some(fetch_error(0x1fff, 0x2000, FaultReason::Permission, 2))
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
fn word_accumulator_extend_mixed_64_instruction_block_fits_all_six_profiles() {
    let mut bytes = [0x66, 0x98, 0x66, 0x99].repeat(16);
    bytes.extend(std::iter::repeat_n(0x90, 31));
    bytes.extend([0xeb, 0]);
    assert_six_profiles(&bytes, 64);
}

#[test]
fn word_accumulator_extend_late_refusal_preserves_full_arena_and_both_owners() {
    let mut bytes = vec![0x66, 0x98, 0x66, 0x99];
    let refused_pc = CODE + bytes.len() as u32;
    bytes.extend([0x0f, 0x0b]);
    let refused_length = bytes.len() as u32;
    bytes.resize(256, 0x90);
    bytes.extend([0xeb, 0]);
    let mut engine = code(&bytes);
    assert_eq!(engine.arena().len(), 4612);
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
