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
            ir::{Location32, Operation, UnaryKind, Value32},
        },
    },
    memory::{
        Access, AddressSpace, FaultReason, GuestAddress, MemoryFault, PageRange, Permissions,
    },
    process::{EngineInstance, HostError},
};

const CODE: u32 = 0x1000;
const KEY: u64 = 0x454e_5445_525a_4552;
const FORMS: [&[u8]; 2] = [&[0xc8, 0, 0, 0], &[0x66, 0xc8, 0, 0, 0]];

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
        .expect("exact ENTER0,0 must decode without stack access");
    assert_eq!(
        (decoded.length() as usize, decoded.next_pc()),
        (bytes.len(), GuestAddress(CODE + bytes.len() as u32))
    );
    assert_eq!(decoded.operation(), &operation(bytes.len() == 5));
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

#[test]
fn enter_zero_decodes_exact_dword_form() {
    decode_admission(&[0xc8, 0, 0, 0]);
}

#[test]
fn enter_zero_decodes_exact_word_form() {
    decode_admission(&[0x66, 0xc8, 0, 0, 0]);
}

fn bound_admission(resident: bool, entries: bool) {
    let bytes = [0xc8, 0, 0, 0, 0x66, 0xc8, 0, 0, 0, 0xeb, 0];
    assert_bound_admission(&bytes, resident, entries);
}

fn assert_bound_admission(bytes: &[u8], resident: bool, entries: bool) {
    let mut engine = code(bytes);
    let before = describe(&mut engine, CODE, bytes.len() as u32, entries);
    let id = bound_compile(&mut engine, resident, entries)
        .expect("exact ENTER0,0 forms must compile bound without a mapped stack");
    let module = if resident {
        engine.resident_bytes(id).unwrap()
    } else {
        engine.artifact_bytes().unwrap()
    };
    assert_eq!(&module[..8], b"\0asm\x01\0\0\0");
    let mut cursor = CODE;
    let mut needed = [false; 2];
    while cursor < CODE + bytes.len() as u32 {
        let decoded = decode_one(engine.memory().unwrap(), GuestAddress(cursor)).unwrap();
        match decoded.operation() {
            Operation::EnterZero => needed[0] = true,
            Operation::EnterZeroWord => needed[1] = true,
            _ => {}
        }
        cursor = decoded.next_pc().0;
    }
    for (required, suffix) in [(needed[0], b"32".as_slice()), (needed[1], b"16".as_slice())] {
        let name = [
            if resident {
                b"store_resident".as_slice()
            } else {
                b"store".as_slice()
            },
            suffix,
        ]
        .concat();
        assert_eq!(contains(module, &name), required);
    }
    for name in [
        b"read32".as_slice(),
        b"read16",
        b"read8",
        b"store8",
        b"store_resident8",
    ] {
        assert!(!contains(module, name));
    }
    assert_eq!(engine.arena(), before);
    assert_eq!(engine.memory().unwrap().mapped_pages(), 1);
}

fn contains(bytes: &[u8], name: &[u8]) -> bool {
    bytes.windows(name.len()).any(|part| part == name)
}

fn describe(engine: &mut EngineInstance, pc: u32, length: u32, entries: bool) -> Vec<u8> {
    let descriptor = &mut engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8];
    descriptor[..4].copy_from_slice(&pc.to_le_bytes());
    descriptor[4..].copy_from_slice(&if entries { 0_u32 } else { length }.to_le_bytes());
    engine.arena().to_vec()
}

fn bound_compile(
    engine: &mut EngineInstance,
    resident: bool,
    entries: bool,
) -> Result<u64, HostError> {
    match (resident, entries) {
        (false, false) => engine.compile(1).map(u64::from),
        (false, true) => engine.compile_entries(1, 0).map(u64::from),
        (true, false) => engine.compile_resident(1).map(|id| id.get()),
        (true, true) => engine.compile_resident_entries(1, 0).map(|id| id.get()),
    }
}

#[test]
fn enter_zero_compiles_replacement_explicit() {
    bound_admission(false, false);
}

#[test]
fn enter_zero_compiles_replacement_entry() {
    bound_admission(false, true);
}

#[test]
fn enter_zero_compiles_resident_explicit() {
    bound_admission(true, false);
}

#[test]
fn enter_zero_compiles_resident_entry() {
    bound_admission(true, true);
}

fn operation(word: bool) -> Operation {
    if word {
        Operation::EnterZeroWord
    } else {
        Operation::EnterZero
    }
}

fn code_space(pc: u32, bytes: &[u8]) -> AddressSpace {
    let pages = (u64::from(pc & 0xfff) + bytes.len() as u64).div_ceil(4096) as u32;
    let mut memory = AddressSpace::new(pages + 1).unwrap();
    let range = PageRange::new(GuestAddress(pc & !0xfff), pages).unwrap();
    memory.map_zeroed(range, Permissions::ALL).unwrap();
    memory.write(GuestAddress(pc), bytes).unwrap();
    memory.protect(range, Permissions::EXECUTE).unwrap();
    memory
}

#[test]
fn enter_zero_exact_ir_fixed32_and_prefix_payload_adjacency_are_literal() {
    for (word, form) in FORMS.into_iter().enumerate() {
        for pc in [CODE, 0x1234_fff0, 0_u32.wrapping_sub(form.len() as u32)] {
            let memory = code_space(pc, form);
            let decoded = decode_one(&memory, GuestAddress(pc)).unwrap();
            assert_eq!(decoded.operation(), &operation(word == 1));
            assert_eq!(
                (decoded.length() as usize, decoded.next_pc().0),
                (form.len(), pc.wrapping_add(form.len() as u32))
            );
            assert!(memory.is_code_current(decoded.code_snapshot()));
        }
        let bytes = [vec![0x48], form.to_vec()].concat();
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
        assert_eq!(
            decode_one(&memory, first.next_pc()).unwrap().operation(),
            &operation(word == 1)
        );
        for payload in [[0x66, 0x67, 0xf0, 0xf3], [0xc8, 0, 0, 0]] {
            let bytes = [vec![0xb8], payload.to_vec(), form.to_vec()].concat();
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
            let second = decode_one(&memory, first.next_pc()).unwrap();
            assert_eq!(second.operation(), &operation(word == 1));
            assert_eq!(
                (second.length() as usize, second.next_pc().0),
                (form.len(), CODE + 5 + form.len() as u32)
            );
        }
    }
}

#[test]
fn enter_zero_prefixes_and_nonzero_allocation_or_raw_nesting_stay_closed() {
    let opcode = DecodeError::Unsupported(UnsupportedFeature::Opcode);
    for form in FORMS {
        for prefix in [0x67, 0xf0, 0xf2, 0xf3, 0x26, 0x2e, 0x36, 0x3e, 0x64, 0x65] {
            let expected = match prefix {
                0xf0 => DecodeError::InvalidEncoding,
                0x26 | 0x2e | 0x36 | 0x3e | 0x64 | 0x65 => {
                    DecodeError::Unsupported(UnsupportedFeature::Segment)
                }
                _ => opcode,
            };
            let mut witnesses = vec![[vec![prefix], form.to_vec()].concat()];
            if form[0] == 0x66 {
                witnesses.push([vec![0x66, prefix], form[1..].to_vec()].concat());
            }
            for bytes in witnesses {
                assert_eq!(
                    decode_one(&code_space(CODE, &bytes), GuestAddress(CODE)).err(),
                    Some(expected),
                    "{bytes:02x?}"
                );
            }
        }
        let immediate = form.len() - 3;
        for allocation in [1_u16, 0x66, 0x67, 0x8000, 0xffff] {
            let mut bytes = form.to_vec();
            bytes[immediate..immediate + 2].copy_from_slice(&allocation.to_le_bytes());
            assert_eq!(
                decode_one(&code_space(CODE, &bytes), GuestAddress(CODE)).err(),
                Some(opcode),
                "{bytes:02x?}"
            );
        }
        for nesting in [1, 31, 32, 64, 128, 255] {
            let mut bytes = form.to_vec();
            *bytes.last_mut().unwrap() = nesting;
            assert_eq!(
                decode_one(&code_space(CODE, &bytes), GuestAddress(CODE)).err(),
                Some(opcode),
                "{bytes:02x?}"
            );
        }
    }
    let redundant = [0x66, 0x66, 0xc8, 0, 0, 0];
    assert_eq!(
        decode_one(&code_space(CODE, &redundant), GuestAddress(CODE)).err(),
        Some(opcode)
    );
    for bytes in [&[0x60][..], &[0x66, 0x60], &[0x9d], &[0x66, 0x9d]] {
        assert_eq!(
            decode_one(&code_space(CODE, bytes), GuestAddress(CODE)).err(),
            Some(opcode)
        );
    }
    for (bytes, expected) in [
        (&[0xc9][..], Operation::Leave),
        (&[0x66, 0xc9][..], Operation::LeaveWord),
    ] {
        assert_eq!(
            decode_one(&code_space(CODE, bytes), GuestAddress(CODE))
                .unwrap()
                .operation(),
            &expected
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
fn enter_zero_standalone_backend_refusal_precedes_trailing_decode() {
    for form in FORMS {
        for tail in [&[][..], &[0x0f, 0x0b][..]] {
            let bytes = [vec![0x90], form.to_vec(), tail.to_vec()].concat();
            let memory = code_space(CODE, &bytes);
            let specs = [BlockSpec {
                entry: GuestAddress(CODE),
                byte_length: bytes.len() as u32,
            }];
            let expected = Some(instruction_error(
                CODE + 1,
                InstructionError::BackendUnsupported,
            ));
            for actual in [
                prepare_region(&memory, &specs, CompileLimits::default()).err(),
                prepare_entry_region(&memory, &[GuestAddress(CODE)], CompileLimits::default())
                    .err(),
                compile_region(&memory, &specs, CompileLimits::default()).err(),
                compile_entry_region(&memory, &[GuestAddress(CODE)], CompileLimits::default())
                    .err(),
            ] {
                assert_eq!(actual, expected);
            }
        }
    }
}

#[test]
fn enter_zero_falls_through_to_following_instruction_in_all_bound_profiles() {
    for form in FORMS {
        let bytes = [form.to_vec(), vec![0x90, 0xeb, 0]].concat();
        let memory = code_space(CODE, &bytes);
        let next = decode_one(&memory, GuestAddress(CODE)).unwrap().next_pc();
        assert_eq!(
            decode_one(&memory, next).unwrap().operation(),
            &Operation::Nop
        );
        for resident in [false, true] {
            for entries in [false, true] {
                assert_bound_admission(&bytes, resident, entries);
            }
            assert_bound_admission(form, resident, false);
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
fn enter_zero_fetches_exact_four_or_five_bytes_without_wrapping_source() {
    for (word, form) in FORMS.into_iter().enumerate() {
        for cut in 1..form.len() {
            let pc = 0x2000 - cut as u32;
            let memory = code_space(pc, &form[..cut]);
            assert_eq!(
                decode_one(&memory, GuestAddress(pc)).err(),
                Some(fetch_error(pc, 0x2000, FaultReason::Unmapped, cut + 1))
            );
            let pc = 0_u32.wrapping_sub(cut as u32);
            let mut memory = code_space(pc, &form[..cut]);
            memory
                .map_zeroed(
                    PageRange::new(GuestAddress(0), 1).unwrap(),
                    Permissions::ALL,
                )
                .unwrap();
            memory.write(GuestAddress(0), &form[cut..]).unwrap();
            memory
                .protect(
                    PageRange::new(GuestAddress(0), 1).unwrap(),
                    Permissions::EXECUTE,
                )
                .unwrap();
            assert_eq!(
                decode_one(&memory, GuestAddress(pc)).err(),
                Some(fetch_error(pc, pc, FaultReason::AddressOverflow, cut + 1))
            );
        }
        for pc in [
            0x2000 - form.len() as u32,
            0x2001 - form.len() as u32,
            0_u32.wrapping_sub(form.len() as u32),
        ] {
            let memory = code_space(pc, form);
            let decoded = decode_one(&memory, GuestAddress(pc)).unwrap();
            assert_eq!(decoded.operation(), &operation(word == 1));
            assert_eq!(decoded.next_pc().0, pc.wrapping_add(form.len() as u32));
            assert!(memory.is_code_current(decoded.code_snapshot()));
        }
        let pc = 0x2001 - form.len() as u32;
        let mut memory = code_space(pc, form);
        memory
            .protect(
                PageRange::new(GuestAddress(0x2000), 1).unwrap(),
                Permissions::READ,
            )
            .unwrap();
        assert_eq!(
            decode_one(&memory, GuestAddress(pc)).err(),
            Some(fetch_error(pc, 0x2000, FaultReason::Permission, form.len()))
        );
        let mut memory = code_space(CODE, form);
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
fn enter_zero_currentness_tracks_consumed_code_but_ignores_old_frame_and_stack_pages() {
    for form in FORMS {
        let pc = 0x1ffe;
        let bytes = [form.to_vec(), vec![0xeb, 0]].concat();
        let mut engine = EngineInstance::new(3, KEY).unwrap();
        engine.map(CODE, 2, 7).unwrap();
        engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
            .copy_from_slice(&bytes);
        engine.upload(pc, bytes.len() as u32).unwrap();
        let decoded = decode_one(engine.memory().unwrap(), GuestAddress(pc)).unwrap();
        describe(&mut engine, pc, bytes.len() as u32, false);
        let replacement = bound_compile(&mut engine, false, false).unwrap() as u32;
        let resident = bound_compile(&mut engine, true, false).unwrap();
        let replacement_bytes = engine.artifact_bytes().unwrap().to_vec();
        let resident_bytes = engine.resident_bytes(resident).unwrap().to_vec();
        engine.map(0x8000, 1, 3).unwrap();
        engine.write32(0x8000, 0xffff_fffe).unwrap();
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
        assert_eq!(engine.artifact_bytes().unwrap(), replacement_bytes);
        assert_eq!(engine.resident_bytes(resident).unwrap(), resident_bytes);
        engine.arena_mut().unwrap()[TRANSFER_OFFSET] = bytes[2];
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
fn enter_zero_mixed_unit_fits_exactly_64_instructions() {
    let mut bytes = Vec::new();
    for _ in 0..8 {
        bytes.extend(FORMS[0]);
        bytes.extend(FORMS[1]);
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
fn enter_zero_late_refusal_preserves_dirty_full_arena_and_both_existing_owners() {
    let mut bytes = [FORMS[0], FORMS[1], &[0x0f, 0x0b]].concat();
    let refused_length = bytes.len() as u32;
    bytes.resize(256, 0x90);
    bytes.extend([0xeb, 0]);
    let mut engine = code(&bytes);
    describe(&mut engine, CODE + 256, 2, false);
    let replacement = bound_compile(&mut engine, false, false).unwrap() as u32;
    let resident = bound_compile(&mut engine, true, false).unwrap();
    let replacement_bytes = engine.artifact_bytes().unwrap().to_vec();
    let replacement_pointer = engine.artifact_bytes().unwrap().as_ptr();
    let resident_bytes = engine.resident_bytes(resident).unwrap().to_vec();
    let resident_pointer = engine.resident_bytes(resident).unwrap().as_ptr();
    let error = instruction_error(
        CODE + 9,
        InstructionError::Decode(DecodeError::Unsupported(UnsupportedFeature::Opcode)),
    );
    for is_resident in [false, true] {
        for entries in [false, true] {
            for (index, byte) in engine.arena_mut().unwrap().iter_mut().enumerate() {
                *byte = (index.wrapping_mul(37).wrapping_add(11) & 0xff) as u8;
            }
            let before = describe(&mut engine, CODE, refused_length, entries);
            assert_eq!(before.len(), 4612);
            assert_eq!(
                bound_compile(&mut engine, is_resident, entries),
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
