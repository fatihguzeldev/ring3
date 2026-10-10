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
const KEY: u64 = 0x504f_5041_4c4c_3136;

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
        .expect("POP-all must decode without reading the guest stack");
    assert_eq!(decoded.operation(), &operation(bytes.len()));
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

#[test]
fn popad_decodes_without_reading_the_stack() {
    decode_admission(&[0x61]);
}

#[test]
fn popaw_decodes_exact_single_operand_prefix() {
    decode_admission(&[0x66, 0x61]);
}

fn bound_admission(resident: bool, entries: bool) {
    let bytes = [0x61, 0x66, 0x61, 0xeb, 0];
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
    .expect("POPAD and POPAW must compile bound without a mapped guest stack");
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
fn pop_all_compiles_replacement_explicit() {
    bound_admission(false, false);
}

#[test]
fn pop_all_compiles_replacement_entry() {
    bound_admission(false, true);
}

#[test]
fn pop_all_compiles_resident_explicit() {
    bound_admission(true, false);
}

#[test]
fn pop_all_compiles_resident_entry() {
    bound_admission(true, true);
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

fn operation(length: usize) -> Operation {
    if length == 1 {
        Operation::PopAll
    } else {
        Operation::PopAllWord
    }
}

#[test]
fn pop_all_typed_forms_keep_fixed32_and_immediate_payload_boundaries() {
    for form in [&[0x61][..], &[0x66, 0x61]] {
        let bytes = [vec![0x48], form.to_vec()].concat();
        let memory = code_space(CODE, &bytes);
        let first = decode_one(&memory, GuestAddress(CODE)).unwrap();
        assert_eq!(
            first.operation(),
            &Operation::Unary {
                kind: UnaryKind::Dec,
                destination: Location32::Register(Register32::Eax),
            }
        );
        assert_eq!(
            (first.length(), first.next_pc()),
            (1, GuestAddress(CODE + 1))
        );
        let next = decode_one(&memory, first.next_pc()).unwrap();
        assert_eq!(next.operation(), &operation(form.len()));
        assert_eq!(
            (next.length() as usize, next.next_pc()),
            (form.len(), GuestAddress(CODE + 1 + form.len() as u32))
        );
        for payload in [[0x61, 0x66, 0xf0, 0xf3], [0xf2, 0x61, 0x66, 0x67]] {
            let bytes = [vec![0xb8], payload.to_vec(), form.to_vec()].concat();
            let memory = code_space(CODE, &bytes);
            let first = decode_one(&memory, GuestAddress(CODE)).unwrap();
            assert_eq!(
                first.operation(),
                &Operation::Move {
                    destination: Location32::Register(Register32::Eax),
                    source: Value32::Immediate(u32::from_le_bytes(payload)),
                }
            );
            assert_eq!(
                (first.length(), first.next_pc()),
                (5, GuestAddress(CODE + 5))
            );
            let next = decode_one(&memory, first.next_pc()).unwrap();
            assert_eq!(next.operation(), &operation(form.len()));
            assert_eq!(
                (next.length() as usize, next.next_pc()),
                (form.len(), GuestAddress(CODE + 5 + form.len() as u32))
            );
        }
    }
}

#[test]
fn pop_all_prefixes_lock_segments_and_unopened_neighbors_stay_precise() {
    let opcode = DecodeError::Unsupported(UnsupportedFeature::Opcode);
    for prefix in [0x67, 0xf2, 0xf3, 0xf0, 0x26, 0x2e, 0x36, 0x3e, 0x64, 0x65] {
        let expected = match prefix {
            0xf0 => DecodeError::InvalidEncoding,
            0x26 | 0x2e | 0x36 | 0x3e | 0x64 | 0x65 => {
                DecodeError::Unsupported(UnsupportedFeature::Segment)
            }
            _ => opcode,
        };
        for bytes in [
            vec![prefix, 0x61],
            vec![prefix, 0x66, 0x61],
            vec![0x66, prefix, 0x61],
        ] {
            assert_eq!(
                decode_one(&code_space(CODE, &bytes), GuestAddress(CODE)).err(),
                Some(expected),
                "{bytes:02x?}"
            );
        }
    }
    for bytes in [
        &[0x66, 0x66, 0x61][..],
        &[0x60],
        &[0x66, 0x60],
        &[0x9d],
        &[0x66, 0x9d],
        &[0xc8, 1, 0, 0],
        &[0x66, 0xc8, 1, 0, 0],
    ] {
        assert_eq!(
            decode_one(&code_space(CODE, bytes), GuestAddress(CODE)).err(),
            Some(opcode),
            "{bytes:02x?}"
        );
    }
}

#[test]
fn pop_all_stays_excluded_in_both_standalone_profiles() {
    for form in [&[0x61][..], &[0x66, 0x61]] {
        let bytes = [vec![0x90], form.to_vec(), vec![0xeb, 0]].concat();
        let memory = code_space(CODE, &bytes);
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
fn pop_all_fetches_exact_one_or_two_bytes_at_page_top_and_execute_boundaries() {
    for bytes in [&[0x61][..], &[0x66, 0x61]] {
        for pc in [CODE, 0x1fff, u32::MAX - bytes.len() as u32 + 1] {
            let memory = code_space(pc, bytes);
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
        let mut memory = code_space(CODE, bytes);
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
    assert_eq!(
        decode_one(&code_space(0x1fff, &[0x66]), GuestAddress(0x1fff)).err(),
        Some(fetch_error(0x1fff, 0x2000, FaultReason::Unmapped, 2))
    );
    assert_eq!(
        decode_one(&code_space(u32::MAX, &[0x66]), GuestAddress(u32::MAX)).err(),
        Some(fetch_error(
            u32::MAX,
            u32::MAX,
            FaultReason::AddressOverflow,
            2
        ))
    );
    let mut memory = code_space(0x1fff, &[0x66, 0x61]);
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
}

#[test]
fn pop_all_code_currency_uses_only_consumed_pages() {
    for bytes in [&[0x61][..], &[0x66, 0x61]] {
        let mut memory = AddressSpace::new(3).unwrap();
        memory
            .map_zeroed(
                PageRange::new(GuestAddress(CODE), 3).unwrap(),
                Permissions::ALL,
            )
            .unwrap();
        memory.write(GuestAddress(0x1fff), bytes).unwrap();
        let decoded = decode_one(&memory, GuestAddress(0x1fff)).unwrap();
        let unused = if bytes.len() == 1 { 0x2000 } else { 0x3000 };
        memory.write(GuestAddress(unused), &[0x90]).unwrap();
        assert!(memory.is_code_current(decoded.code_snapshot()));
        memory.write(GuestAddress(0x1fff), &bytes[..1]).unwrap();
        assert!(!memory.is_code_current(decoded.code_snapshot()));
    }
}

#[test]
fn pop_all_mixed_64_instruction_block_fits_four_bound_profiles() {
    let mut bytes = vec![0x61, 0x66, 0x61];
    bytes.extend(std::iter::repeat_n(0x90, 61));
    bytes.extend([0xeb, 0]);
    for resident in [false, true] {
        for entries in [false, true] {
            assert_bound_admission(&bytes, resident, entries);
        }
    }
}

#[test]
fn pop_all_late_refusal_preserves_full_arena_and_both_owners() {
    let mut bytes = vec![0x61, 0x66, 0x61];
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
            for (index, byte) in engine.arena_mut().unwrap().iter_mut().enumerate() {
                *byte = (index.wrapping_mul(37).wrapping_add(11) & 0xff) as u8;
            }
            let descriptor = &mut engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8];
            descriptor[..4].copy_from_slice(&CODE.to_le_bytes());
            descriptor[4..]
                .copy_from_slice(&if entries { 0 } else { refused_length }.to_le_bytes());
            let before = engine.arena().to_vec();
            assert_eq!(before.len(), 4364);
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
