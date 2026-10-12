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
            ir::{EffectiveAddress, Operation},
        },
    },
    memory::{
        Access, AddressSpace, FaultReason, GuestAddress, MemoryFault, PageRange, Permissions,
    },
    process::{EngineInstance, HostError},
};

const CODE: u32 = 0x1000;
const KEY: u64 = 0x5749_4e44_4341_4c4c;
const FORMS: [&[u8]; 2] = [&[0x66, 0xff, 0xd4], &[0x66, 0xff, 0x14, 0x24]];

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
    let instruction = decode_one(engine.memory().unwrap(), GuestAddress(CODE))
        .expect("exact WORD indirect CALL must decode without target memory");
    assert_eq!(instruction.length() as usize, bytes.len());
    assert_eq!(
        instruction.next_pc(),
        GuestAddress(CODE + bytes.len() as u32)
    );
    assert_eq!(
        instruction.operation(),
        &if bytes.len() == 3 {
            Operation::CallRegisterWord {
                source: Register32::Esp,
            }
        } else {
            Operation::ReadCallWord {
                address: address(Some(Register32::Esp), None, 1, 0),
            }
        }
    );
    assert!(
        engine
            .memory()
            .unwrap()
            .is_code_current(instruction.code_snapshot())
    );
    assert_eq!(engine.arena(), before);
    assert_eq!(engine.memory().unwrap().mapped_pages(), 1);
}

#[test]
fn word_indirect_call_decodes_register_form() {
    decode_admission(FORMS[0]);
}

#[test]
fn word_indirect_call_decodes_memory_form() {
    decode_admission(FORMS[1]);
}

fn describe(engine: &mut EngineInstance, blocks: &[(u32, u32)], entries: bool) {
    let descriptor = &mut engine.arena_mut().unwrap()[TRANSFER_OFFSET..];
    for (index, &(pc, length)) in blocks.iter().enumerate() {
        let offset = index * if entries { 4 } else { 8 };
        descriptor[offset..offset + 4].copy_from_slice(&pc.to_le_bytes());
        if !entries {
            descriptor[offset + 4..offset + 8].copy_from_slice(&length.to_le_bytes());
        }
    }
}

fn bound_compile(
    engine: &mut EngineInstance,
    blocks: &[(u32, u32)],
    resident: bool,
    entries: bool,
) -> Result<u64, HostError> {
    describe(engine, blocks, entries);
    match (resident, entries) {
        (false, false) => engine.compile(blocks.len() as u32).map(u64::from),
        (false, true) => engine
            .compile_entries(blocks.len() as u32, 0)
            .map(u64::from),
        (true, false) => engine
            .compile_resident(blocks.len() as u32)
            .map(|id| id.get()),
        (true, true) => engine
            .compile_resident_entries(blocks.len() as u32, 0)
            .map(|id| id.get()),
    }
}

fn bound_admission(resident: bool, entries: bool) {
    let bytes = [0x66, 0xff, 0xd4, 0xcc, 0x66, 0xff, 0x14, 0x24];
    let blocks = [(CODE, 3), (CODE + 4, 4)];
    let mut engine = code(&bytes);
    describe(&mut engine, &blocks, entries);
    let before = engine.arena().to_vec();
    let id = bound_compile(&mut engine, &blocks, resident, entries)
        .expect("WORD indirect register and memory CALL must compile bound");
    let module = if resident {
        engine.resident_bytes(id).unwrap()
    } else {
        engine.artifact_bytes().unwrap()
    };
    assert_eq!(&module[..8], b"\0asm\x01\0\0\0");
    assert_narrow_imports(module, resident, true);
    assert_eq!(engine.arena(), before);
    assert_eq!(engine.memory().unwrap().mapped_pages(), 1);
    assert!(
        engine
            .memory()
            .unwrap()
            .resolve(GuestAddress(0x8000), Access::Read)
            .is_err()
    );
}

#[test]
fn word_indirect_call_compiles_replacement_explicit() {
    bound_admission(false, false);
}

#[test]
fn word_indirect_call_compiles_replacement_entry() {
    bound_admission(false, true);
}

#[test]
fn word_indirect_call_compiles_resident_explicit() {
    bound_admission(true, false);
}

#[test]
fn word_indirect_call_compiles_resident_entry() {
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

fn range(pc: u32, pages: u32) -> PageRange {
    PageRange::new(GuestAddress(pc), pages).unwrap()
}

fn spec(pc: u32, length: u32) -> BlockSpec {
    BlockSpec {
        entry: GuestAddress(pc),
        byte_length: length,
    }
}

fn code_space(pc: u32, bytes: &[u8]) -> AddressSpace {
    let pages = (u64::from(pc & 0xfff) + bytes.len() as u64).div_ceil(4096) as u32;
    let mut memory = AddressSpace::new(pages).unwrap();
    let pages = range(pc & !0xfff, pages);
    memory.map_zeroed(pages, Permissions::ALL).unwrap();
    memory.write(GuestAddress(pc), bytes).unwrap();
    memory.protect(pages, Permissions::EXECUTE).unwrap();
    memory
}

fn address(
    base: Option<Register32>,
    index: Option<Register32>,
    scale: u8,
    displacement: u32,
) -> EffectiveAddress {
    EffectiveAddress {
        base,
        index,
        scale,
        displacement,
        ..Default::default()
    }
}

fn addresses() -> [(&'static [u8], EffectiveAddress); 13] {
    use Register32::{Ebp, Ebx, Ecx, Esi, Esp};
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

fn memory_encoding(tail: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0x66, 0xff, tail[0] | 0x10];
    bytes.extend_from_slice(&tail[1..]);
    bytes
}

fn assert_operation(bytes: &[u8], expected: Operation) {
    let memory = code_space(CODE, bytes);
    let instruction = decode_one(&memory, GuestAddress(CODE)).unwrap();
    assert_eq!(instruction.operation(), &expected, "{bytes:02x?}");
    assert_eq!(
        (instruction.length() as usize, instruction.next_pc()),
        (bytes.len(), GuestAddress(CODE + bytes.len() as u32))
    );
    assert!(memory.is_code_current(instruction.code_snapshot()));
}

fn assert_narrow_imports(module: &[u8], resident: bool, reads: bool) {
    let store: &[u8] = if resident {
        b"store_resident16"
    } else {
        b"store16"
    };
    assert!(module.windows(store.len()).any(|window| window == store));
    assert_eq!(module.windows(6).any(|window| window == b"read16"), reads);
    for name in [&b"read32"[..], &b"store32"[..], &b"store_resident32"[..]] {
        assert!(!module.windows(name.len()).any(|window| window == name));
    }
}

fn assert_bound(bytes: &[u8], blocks: &[(u32, u32)], reads: bool) {
    for resident in [false, true] {
        for entries in [false, true] {
            let mut engine = code(bytes);
            describe(&mut engine, blocks, entries);
            let before = engine.arena().to_vec();
            let id = bound_compile(&mut engine, blocks, resident, entries).unwrap();
            let module = if resident {
                engine.resident_bytes(id).unwrap()
            } else {
                engine.artifact_bytes().unwrap()
            };
            assert_eq!(&module[..8], b"\0asm\x01\0\0\0");
            assert_narrow_imports(module, resident, reads);
            assert_eq!(engine.arena(), before);
            assert_eq!(engine.memory().unwrap().mapped_pages(), 1);
            assert!(
                engine
                    .memory()
                    .unwrap()
                    .resolve(GuestAddress(0x8000), Access::Read)
                    .is_err()
            );
        }
    }
}

#[test]
fn word_indirect_call_all_eight_registers_and_thirteen_eas_have_exact_ir_and_bound_admission() {
    let register_forms: Vec<_> = REGISTERS
        .into_iter()
        .enumerate()
        .map(|(selector, source)| {
            let bytes = vec![0x66, 0xff, 0xd0 | selector as u8];
            assert_operation(&bytes, Operation::CallRegisterWord { source });
            bytes
        })
        .collect();
    let memory_forms: Vec<_> = addresses()
        .into_iter()
        .map(|(tail, address)| {
            let bytes = memory_encoding(tail);
            assert_operation(&bytes, Operation::ReadCallWord { address });
            bytes
        })
        .collect();
    assert_eq!(register_forms.len() + memory_forms.len(), 21);
    for (bank_kind, forms) in [register_forms, memory_forms].into_iter().enumerate() {
        for bank in forms.chunks(8) {
            let mut bytes = vec![0xcc; bank.len() * 16];
            let blocks: Vec<_> = bank
                .iter()
                .enumerate()
                .map(|(index, form)| {
                    bytes[index * 16..index * 16 + form.len()].copy_from_slice(form);
                    (CODE + index as u32 * 16, form.len() as u32)
                })
                .collect();
            assert_bound(&bytes, &blocks, bank_kind == 1);
        }
    }
}

#[test]
fn word_indirect_call_full32_aliases_and_prefix_looking_address_payloads_are_literal() {
    for sib in [0x66_u8, 0x67, 0xf0, 0xf2, 0xf3] {
        let index = (sib >> 3) & 7;
        assert_operation(
            &memory_encoding(&[0x04, sib]),
            Operation::ReadCallWord {
                address: address(
                    Some(REGISTERS[usize::from(sib & 7)]),
                    if index == 4 {
                        None
                    } else {
                        Some(REGISTERS[usize::from(index)])
                    },
                    1 << (sib >> 6),
                    0,
                ),
            },
        );
    }
    for (tail, address) in [
        (&[0x00][..], address(Some(Register32::Eax), None, 1, 0)),
        (
            &[0x04, 0x85, 0x66, 0x67, 0xf0, 0xf3][..],
            address(None, Some(Register32::Eax), 4, 0xf3f0_6766),
        ),
        (
            &[0x04, 0x80][..],
            address(Some(Register32::Eax), Some(Register32::Eax), 4, 0),
        ),
        (
            &[0x44, 0x24, 0xf0][..],
            address(Some(Register32::Esp), None, 1, 0xffff_fff0),
        ),
    ] {
        assert_operation(&memory_encoding(tail), Operation::ReadCallWord { address });
    }
    for displacement in [
        0_u32,
        0x8000_0000,
        u32::MAX,
        0x6666_6666,
        0x6767_6767,
        0xf0f0_f0f0,
        0xf2f2_f2f2,
        0xf3f3_f3f3,
    ] {
        let mut tail = vec![0x05];
        tail.extend(displacement.to_le_bytes());
        assert_operation(
            &memory_encoding(&tail),
            Operation::ReadCallWord {
                address: address(None, None, 1, displacement),
            },
        );
    }
}

#[test]
fn word_indirect_call_prefixes_lock_and_adjacent_ff_groups_remain_precise() {
    let unsupported = DecodeError::Unsupported(UnsupportedFeature::Opcode);
    for form in FORMS {
        for prefix in [
            0x66, 0x67, 0xf0, 0xf2, 0xf3, 0x26, 0x2e, 0x36, 0x3e, 0x64, 0x65,
        ] {
            if matches!(prefix, 0x67 | 0x26 | 0x2e | 0x36 | 0x3e | 0x64 | 0x65) {
                word_prefix::admitted(&[vec![prefix], form.to_vec()].concat());
                continue;
            }
            let expected = match prefix {
                0xf0 => DecodeError::InvalidEncoding,
                0x26 | 0x2e | 0x36 | 0x3e | 0x64 | 0x65 => {
                    DecodeError::Unsupported(UnsupportedFeature::Segment)
                }
                _ => unsupported,
            };
            for bytes in [
                [vec![prefix], form.to_vec()].concat(),
                [vec![0x66, prefix], form[1..].to_vec()].concat(),
            ] {
                assert_eq!(
                    decode_one(&code_space(CODE, &bytes), GuestAddress(CODE)).err(),
                    Some(expected),
                    "{bytes:02x?}"
                );
            }
        }
    }
    let bytes = [0x66, 0xff, 0x1b];
    let engine = code(&bytes);
    let before = engine.arena().to_vec();
    let decoded = decode_one(engine.memory().unwrap(), GuestAddress(CODE)).unwrap();
    assert_eq!(
        decoded.operation(),
        &Operation::FarCall {
            width: ring3_engine::cpu::x86::FarWidth::Word,
            target: ring3_engine::cpu::x86::ir::FarPointer::Memory(address(
                Some(Register32::Ebx),
                None,
                1,
                0,
            )),
        },
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
    assert_operation(
        &[0x66, 0xff, 0x2b],
        Operation::FarJump {
            width: ring3_engine::cpu::x86::FarWidth::Word,
            target: ring3_engine::cpu::x86::ir::FarPointer::Memory(address(
                Some(Register32::Ebx),
                None,
                1,
                0,
            )),
        },
    );
    for bytes in [
        &[0x66, 0xff, 0xe8][..],
        &[0x66, 0xff, 0xf8],
        &[0x66, 0xff, 0x3b],
    ] {
        assert_eq!(
            decode_one(&code_space(CODE, bytes), GuestAddress(CODE)).err(),
            Some(DecodeError::InvalidEncoding)
        );
    }
    for bytes in [
        &[0x66, 0xff, 0xc0][..],
        &[0x66, 0xff, 0xcb],
        &[0x66, 0xff, 0xf4],
        &[0x66, 0xff, 0x03],
        &[0x66, 0xff, 0x0b],
        &[0x66, 0xff, 0x33],
        &[0x66, 0xff, 0xe4],
        &[0x66, 0xff, 0x24, 0x24],
        &[0xff, 0xd4],
        &[0xff, 0x14, 0x24],
    ] {
        assert!(
            decode_one(&code_space(CODE, bytes), GuestAddress(CODE)).is_ok(),
            "{bytes:02x?}"
        );
    }
}

fn instruction_error(pc: u32, cause: InstructionError) -> CompileError {
    CompileError::Instruction {
        pc: GuestAddress(pc),
        cause,
    }
}

#[test]
fn word_indirect_call_standalone_refusals_precede_trailing_decode() {
    for form in FORMS {
        for trailing in [false, true] {
            let mut bytes = vec![0x90];
            bytes.extend(form);
            if trailing {
                bytes.extend([0xf0, 0x90]);
            }
            let memory = code_space(CODE, &bytes);
            let snapshot = memory
                .snapshot_code(GuestAddress(CODE), bytes.len())
                .unwrap();
            let expected = instruction_error(CODE + 1, InstructionError::BackendUnsupported);
            let blocks = [spec(CODE, bytes.len() as u32)];
            let limits = CompileLimits::default();
            assert_eq!(
                prepare_region(&memory, &blocks, limits).err(),
                Some(expected)
            );
            assert_eq!(
                compile_region(&memory, &blocks, limits).err(),
                Some(expected)
            );
            let entry_error = instruction_error(CODE + 1, InstructionError::BackendUnsupported);
            assert_eq!(
                prepare_entry_region(&memory, &[GuestAddress(CODE)], limits).err(),
                Some(entry_error)
            );
            assert_eq!(
                compile_entry_region(&memory, &[GuestAddress(CODE)], limits).err(),
                Some(entry_error)
            );
            assert!(memory.is_code_current(&snapshot));
        }
    }
}

#[test]
fn word_indirect_call_bound_entries_stop_before_trailing_invalid_code_and_spans_remain_exact() {
    for form in FORMS {
        let mut bytes = vec![0x90];
        bytes.extend(form);
        bytes.extend([0xf0, 0x90]);
        let consumed = form.len() as u32 + 1;
        for resident in [false, true] {
            let mut engine = code(&bytes);
            bound_compile(&mut engine, &[(CODE, consumed)], resident, true).unwrap();
            for length in [consumed - 1, bytes.len() as u32] {
                let error = instruction_error(CODE + 1, InstructionError::InvalidBlockEnd);
                assert_eq!(
                    bound_compile(&mut engine, &[(CODE, length)], resident, false),
                    Err(if resident {
                        HostError::Resident(RegistryError::Compile(error))
                    } else {
                        HostError::Compile(error)
                    })
                );
            }
        }
        assert_bound(&bytes, &[(CODE, consumed)], form[2] & 0xc0 != 0xc0);
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
fn word_indirect_call_fetches_exact_three_or_eight_bytes_with_original_overflow_priority() {
    for form in [
        &[0x66, 0xff, 0xd4][..],
        &[0x66, 0xff, 0x94, 0xf4, 0x66, 0x67, 0xf0, 0xf3],
    ] {
        let length = form.len() as u32;
        for pc in [0x1234_1fff, u32::MAX - length + 1] {
            let memory = code_space(pc, form);
            let instruction = decode_one(&memory, GuestAddress(pc)).unwrap();
            assert_eq!(
                (instruction.length(), instruction.next_pc()),
                (length as u8, GuestAddress(pc.wrapping_add(length)))
            );
            assert!(memory.is_code_current(instruction.code_snapshot()));
        }
        for cut in 1..form.len() {
            let pc = 0x2000 - cut as u32;
            assert_eq!(
                decode_one(&code_space(pc, &form[..cut]), GuestAddress(pc)).err(),
                Some(fetch_error(
                    pc,
                    0x2000,
                    FaultReason::Unmapped,
                    cut as u32 + 1
                ))
            );
            let top = u32::MAX - cut as u32 + 1;
            let mut memory = AddressSpace::new(2).unwrap();
            memory
                .map_zeroed(range(0xffff_f000, 1), Permissions::ALL)
                .unwrap();
            memory.map_zeroed(range(0, 1), Permissions::ALL).unwrap();
            memory.write(GuestAddress(top), &form[..cut]).unwrap();
            memory.write(GuestAddress(0), &form[cut..]).unwrap();
            memory
                .protect(range(0xffff_f000, 1), Permissions::EXECUTE)
                .unwrap();
            memory.protect(range(0, 1), Permissions::EXECUTE).unwrap();
            assert_eq!(
                decode_one(&memory, GuestAddress(top)).err(),
                Some(fetch_error(
                    top,
                    top,
                    FaultReason::AddressOverflow,
                    cut as u32 + 1
                ))
            );
        }
        for (pc, denied, attempted) in [(CODE, CODE, 1), (0x2001 - length, 0x2000, length)] {
            let mut memory = code_space(pc, form);
            memory.protect(range(denied, 1), Permissions::READ).unwrap();
            assert_eq!(
                decode_one(&memory, GuestAddress(pc)).err(),
                Some(fetch_error(pc, denied, FaultReason::Permission, attempted))
            );
        }
    }
}

#[test]
fn word_indirect_call_currentness_tracks_source_without_capturing_operand_or_target_pages() {
    for form in FORMS {
        let mut engine = EngineInstance::new(3, KEY).unwrap();
        engine.map(CODE, 2, 7).unwrap();
        engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + form.len()]
            .copy_from_slice(form);
        engine.upload(0x1fff, form.len() as u32).unwrap();
        let decoded = decode_one(engine.memory().unwrap(), GuestAddress(0x1fff)).unwrap();
        let blocks = [(0x1fff, form.len() as u32)];
        let replacement = bound_compile(&mut engine, &blocks, false, false).unwrap() as u32;
        let resident = bound_compile(&mut engine, &blocks, true, true).unwrap();
        engine.map(0x8000, 1, 3).unwrap();
        engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 2]
            .copy_from_slice(&[0xff, 0xff]);
        engine.upload(0x8000, 2).unwrap();
        engine.protect(0x8000, 1, 0).unwrap();
        engine.unmap(0x8000, 1).unwrap();
        assert!(
            engine
                .memory()
                .unwrap()
                .is_code_current(decoded.code_snapshot())
        );
        engine.guard(KEY, replacement).unwrap();
        engine.guard_resident(KEY, resident).unwrap();
        engine.artifact_bytes().unwrap();
        engine.resident_bytes(resident).unwrap();
        engine.arena_mut().unwrap()[TRANSFER_OFFSET] = form[1];
        engine.upload(0x2000, 1).unwrap();
        assert!(
            !engine
                .memory()
                .unwrap()
                .is_code_current(decoded.code_snapshot())
        );
        assert!(engine.guard(KEY, replacement).is_err());
        assert!(engine.guard_resident(KEY, resident).is_err());
        assert!(engine.artifact_bytes().is_err());
        assert!(engine.resident_bytes(resident).is_err());
    }
}

#[test]
fn word_indirect_call_mixed_register_and_memory_unit_fits_exactly_64_instructions() {
    let mut bytes = vec![0x90; 31];
    bytes.extend(FORMS[0]);
    let second = bytes.len();
    bytes.extend(std::iter::repeat_n(0x90, 31));
    bytes.extend(FORMS[1]);
    assert_bound(
        &bytes,
        &[
            (CODE, second as u32),
            (CODE + second as u32, (bytes.len() - second) as u32),
        ],
        true,
    );
}

#[test]
fn word_indirect_call_late_refusal_preserves_dirty_full_arena_and_both_existing_owners() {
    let mut bytes = vec![0x90; 258];
    bytes[..3].copy_from_slice(FORMS[0]);
    bytes[16..20].copy_from_slice(FORMS[1]);
    bytes[33..35].copy_from_slice(&[0xf0, 0x90]);
    bytes[256..258].copy_from_slice(&[0xeb, 0]);
    let mut engine = code(&bytes);
    let replacement = bound_compile(&mut engine, &[(CODE + 256, 2)], false, false).unwrap() as u32;
    let resident = bound_compile(&mut engine, &[(CODE + 256, 2)], true, false).unwrap();
    let replacement_bytes = engine.artifact_bytes().unwrap().to_vec();
    let replacement_pointer = engine.artifact_bytes().unwrap().as_ptr();
    let resident_bytes = engine.resident_bytes(resident).unwrap().to_vec();
    let resident_pointer = engine.resident_bytes(resident).unwrap().as_ptr();
    let blocks = [(CODE, 3), (CODE + 16, 4), (CODE + 32, 3)];
    let error = instruction_error(
        CODE + 33,
        InstructionError::Decode(DecodeError::InvalidEncoding),
    );
    for is_resident in [false, true] {
        for entries in [false, true] {
            for (index, byte) in engine.arena_mut().unwrap().iter_mut().enumerate() {
                *byte = (index.wrapping_mul(37).wrapping_add(11) & 0xff) as u8;
            }
            describe(&mut engine, &blocks, entries);
            let before = engine.arena().to_vec();
            assert_eq!(before.len(), ring3_engine::abi::arena::ARENA_SIZE);
            assert_eq!(
                bound_compile(&mut engine, &blocks, is_resident, entries),
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
