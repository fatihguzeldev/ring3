use super::support_fetch as fetch;
use super::support_prefix_admission as prefix_admission;

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
            ir::{Location32, Operation, Value32},
        },
    },
    memory::{Access, AddressSpace, FaultReason, GuestAddress, PageRange, Permissions},
    process::{EngineInstance, HostError},
};

const CODE: u32 = 0x1000;
const KEY: u64 = 0x5041_5553_454e_4154;
const PAUSE: [u8; 2] = [0xf3, 0x90];
const BANK: [u8; 4] = [0xf3, 0x90, 0xeb, 0];

fn code(bytes: &[u8]) -> EngineInstance {
    let mut engine = EngineInstance::new(2, KEY).unwrap();
    engine.map(CODE, 1, 7).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(bytes);
    engine.upload(CODE, bytes.len() as u32).unwrap();
    engine.protect(CODE, 1, 4).unwrap();
    engine
}

#[test]
fn pause_exact_form_decodes() {
    let engine = code(&PAUSE);
    let before = engine.arena().to_vec();
    let decoded =
        decode_one(engine.memory().unwrap(), GuestAddress(CODE)).expect("exact PAUSE must decode");
    assert_eq!(decoded.operation(), &Operation::Nop);
    assert_eq!(decoded.length(), 2);
    assert_eq!(decoded.next_pc(), GuestAddress(CODE + 2));
    assert_eq!(engine.arena(), before);
}

fn standalone_admission(entries: bool) {
    let actual = assert_standalone(&BANK, 2, entries);
    let reference = assert_standalone(&[0x66, 0x90, 0xeb, 0], 2, entries);
    assert_eq!(actual, reference);
}

fn assert_standalone(bytes: &[u8], instructions: usize, entries: bool) -> Vec<u8> {
    let engine = code(bytes);
    let memory = engine.memory().unwrap();
    let compiled = if entries {
        compile_entry_region(memory, &[GuestAddress(CODE)], CompileLimits::default())
    } else {
        compile_region(
            memory,
            &[BlockSpec {
                entry: GuestAddress(CODE),
                byte_length: bytes.len() as u32,
            }],
            CompileLimits::default(),
        )
    }
    .expect("exact PAUSE must compile standalone");
    assert_eq!(
        (compiled.metadata().blocks, compiled.metadata().instructions),
        (1, instructions)
    );
    let module = compiled.wasm_bytes(memory).unwrap();
    assert_eq!(&module[..8], b"\0asm\x01\0\0\0");
    assert_eq!(memory.mapped_pages(), 1);
    module.to_vec()
}

#[test]
fn pause_compiles_standalone_explicit() {
    standalone_admission(false);
}

#[test]
fn pause_compiles_standalone_entry() {
    standalone_admission(true);
}

fn bound_admission(resident: bool, entries: bool) {
    assert_bound(&BANK, resident, entries);
}

fn describe(engine: &mut EngineInstance, pc: u32, length: u32, entries: bool) {
    let descriptor = &mut engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8];
    descriptor[..4].copy_from_slice(&pc.to_le_bytes());
    descriptor[4..].copy_from_slice(&if entries { 0 } else { length }.to_le_bytes());
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

fn assert_bound(bytes: &[u8], resident: bool, entries: bool) {
    let mut engine = code(bytes);
    describe(&mut engine, CODE, bytes.len() as u32, entries);
    let before = engine.arena().to_vec();
    let id = bound_compile(&mut engine, resident, entries).expect("exact PAUSE must compile bound");
    let module = if resident {
        engine.resident_bytes(id).unwrap()
    } else {
        engine.artifact_bytes().unwrap()
    };
    assert_eq!(&module[..8], b"\0asm\x01\0\0\0");
    assert_eq!(engine.memory().unwrap().mapped_pages(), 1);
    assert_eq!(engine.arena(), before);
}

#[test]
fn pause_compiles_replacement_explicit() {
    bound_admission(false, false);
}

#[test]
fn pause_compiles_replacement_entry() {
    bound_admission(false, true);
}

#[test]
fn pause_compiles_resident_explicit() {
    bound_admission(true, false);
}

#[test]
fn pause_compiles_resident_entry() {
    bound_admission(true, true);
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
fn pause_exact_grammar_keeps_prefix_and_instruction_neighbors_closed() {
    for prefix in [
        0x66, 0x67, 0xf0, 0xf2, 0xf3, 0x26, 0x2e, 0x36, 0x3e, 0x64, 0x65,
    ] {
        let bytes = [prefix, 0xf3, 0x90];
        let expected = if prefix == 0xf0 {
            DecodeError::InvalidEncoding
        } else if matches!(prefix, 0x26 | 0x2e | 0x36 | 0x3e | 0x64 | 0x65) {
            DecodeError::Unsupported(UnsupportedFeature::Segment)
        } else {
            DecodeError::Unsupported(UnsupportedFeature::Opcode)
        };
        let (bytes, expected) =
            prefix_admission::refusal_after_prefix_admission(bytes.to_vec(), expected);
        crate::support_fetch::assert_decode_contract(
            (decode_one(&code_space(CODE, &bytes), GuestAddress(CODE)))
                .expect_err("instruction must fail to decode"),
            expected,
        );
    }
    for bytes in [
        &[0xf3, 0x66, 0x90][..],
        &[0xf3, 0x67, 0x90],
        &[0xf3, 0xf2, 0x90],
        &[0xf2, 0x90],
        &[0xf3, 0xcb],
    ] {
        let (bytes, expected) = prefix_admission::refusal_after_prefix_admission(
            bytes.to_vec(),
            DecodeError::Unsupported(UnsupportedFeature::Opcode),
        );
        crate::support_fetch::assert_decode_contract(
            (decode_one(&code_space(CODE, &bytes), GuestAddress(CODE)))
                .expect_err("instruction must fail to decode"),
            expected,
        );
    }
    assert!(matches!(
        prefix_admission::admitted(&[0xf3, 0xa4]).0,
        Operation::RepeatString { .. }
    ));
    for bytes in [&[0x90][..], &[0x66, 0x90], &PAUSE] {
        let decoded = decode_one(&code_space(CODE, bytes), GuestAddress(CODE)).unwrap();
        assert_eq!(decoded.operation(), &Operation::Nop);
        assert_eq!(decoded.length() as usize, bytes.len());
    }
}

fn fetch_error(pc: u32, address: u32, length: u32, reason: FaultReason) -> fetch::FetchFault {
    fetch::expected(pc, address, reason, length)
}

#[test]
fn pause_fetches_both_bytes_without_overreading_or_wrapping() {
    for pc in [0x1ffe, 0x1fff, 0xffff_fffe] {
        let memory = code_space(pc, &PAUSE);
        let decoded = decode_one(&memory, GuestAddress(pc)).unwrap();
        assert_eq!(decoded.operation(), &Operation::Nop);
        assert_eq!(
            (decoded.length(), decoded.next_pc()),
            (2, GuestAddress(pc.wrapping_add(2)))
        );
        assert!(memory.is_code_current(decoded.code_snapshot()));
        assert!(memory.resolve(GuestAddress(pc), Access::Read).is_err());
    }
    let mut memory = code_space(0x1fff, &PAUSE[..1]);
    assert_eq!(
        decode_one(&memory, GuestAddress(0x1fff))
            .err()
            .map(fetch::observed),
        Some(fetch_error(0x1fff, 0x2000, 2, FaultReason::Unmapped))
    );
    let next = PageRange::new(GuestAddress(0x2000), 1).unwrap();
    memory.map_zeroed(next, Permissions::ALL).unwrap();
    memory.write(GuestAddress(0x2000), &PAUSE[1..]).unwrap();
    memory.protect(next, Permissions::READ).unwrap();
    assert_eq!(
        decode_one(&memory, GuestAddress(0x1fff))
            .err()
            .map(fetch::observed),
        Some(fetch_error(0x1fff, 0x2000, 2, FaultReason::Permission))
    );

    let mut memory = code_space(u32::MAX, &PAUSE[..1]);
    let zero = PageRange::new(GuestAddress(0), 1).unwrap();
    memory.map_zeroed(zero, Permissions::ALL).unwrap();
    memory.write(GuestAddress(0), &PAUSE[1..]).unwrap();
    memory.protect(zero, Permissions::EXECUTE).unwrap();
    assert_eq!(
        decode_one(&memory, GuestAddress(u32::MAX))
            .err()
            .map(fetch::observed),
        Some(fetch_error(
            u32::MAX,
            u32::MAX,
            2,
            FaultReason::AddressOverflow
        ))
    );
    let mut memory = code_space(CODE, &PAUSE);
    memory
        .protect(
            PageRange::new(GuestAddress(CODE), 1).unwrap(),
            Permissions::READ,
        )
        .unwrap();
    assert_eq!(
        decode_one(&memory, GuestAddress(CODE))
            .err()
            .map(fetch::observed),
        Some(fetch_error(CODE, CODE, 1, FaultReason::Permission))
    );
}

#[test]
fn pause_falls_through_and_prefix_looking_immediates_remain_data() {
    let bytes = [0xb8, 0xf3, 0x90, 0x66, 0x67, 0xf3, 0x90, 0x90, 0xeb, 0];
    let memory = code_space(CODE, &bytes);
    let first = decode_one(&memory, GuestAddress(CODE)).unwrap();
    assert_eq!(
        first.operation(),
        &Operation::Move {
            destination: Location32::Register(Register32::Eax),
            source: Value32::Immediate(0x6766_90f3)
        }
    );
    assert_eq!(
        (first.length(), first.next_pc()),
        (5, GuestAddress(CODE + 5))
    );
    let pause = decode_one(&memory, first.next_pc()).unwrap();
    assert_eq!(
        (pause.operation(), pause.length(), pause.next_pc()),
        (&Operation::Nop, 2, GuestAddress(CODE + 7))
    );
    let next = decode_one(&memory, pause.next_pc()).unwrap();
    assert_eq!((next.operation(), next.length()), (&Operation::Nop, 1));
    for entries in [false, true] {
        let _ = assert_standalone(&bytes, 4, entries);
        for resident in [false, true] {
            assert_bound(&bytes, resident, entries);
        }
    }
}

#[test]
fn pause_snapshots_and_both_owners_track_each_source_byte_identity() {
    for changed in [0, 1] {
        let mut engine = code(&BANK);
        engine.protect(CODE, 1, 7).unwrap();
        engine.map(0x5000, 1, 3).unwrap();
        describe(&mut engine, CODE, BANK.len() as u32, false);
        let replacement = bound_compile(&mut engine, false, false).unwrap() as u32;
        let resident = bound_compile(&mut engine, true, false).unwrap();
        let decoded = decode_one(engine.memory().unwrap(), GuestAddress(CODE)).unwrap();
        engine.arena_mut().unwrap()[TRANSFER_OFFSET] = 0xa5;
        engine.upload(0x5000, 1).unwrap();
        assert!(
            engine
                .memory()
                .unwrap()
                .is_code_current(decoded.code_snapshot())
        );
        engine.guard(KEY, replacement).unwrap();
        engine.guard_resident(KEY, resident).unwrap();
        engine.arena_mut().unwrap()[TRANSFER_OFFSET] = PAUSE[changed];
        engine.upload(CODE + changed as u32, 1).unwrap();
        assert!(
            !engine
                .memory()
                .unwrap()
                .is_code_current(decoded.code_snapshot())
        );
        assert_eq!(
            engine.guard(KEY, replacement),
            Err(HostError::CodeInvalidated)
        );
        assert_eq!(
            engine.guard_resident(KEY, resident),
            Err(HostError::Resident(RegistryError::CodeInvalidated))
        );
    }
}

#[test]
fn pause_mixed_64_instruction_unit_fits_all_six_profiles() {
    let mut bytes = Vec::new();
    for index in 0..63 {
        if index % 2 == 0 {
            bytes.extend(PAUSE);
        } else {
            bytes.push(0x90);
        }
    }
    bytes.extend([0xeb, 0]);
    for entries in [false, true] {
        let _ = assert_standalone(&bytes, 64, entries);
        for resident in [false, true] {
            assert_bound(&bytes, resident, entries);
        }
    }
}

#[test]
fn pause_late_refusal_preserves_dirty_full_arena_and_both_owners() {
    let mut bytes = vec![0xf3, 0x90, 0xf0, 0x90];
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
    let error = CompileError::Instruction {
        pc: GuestAddress(CODE + 2),
        cause: InstructionError::Decode(DecodeError::InvalidEncoding),
    };
    let segment_offset = ring3_engine::abi::arena::SEGMENTS_OFFSET;
    let segment_record = engine.arena()
        [segment_offset..segment_offset + ring3_engine::abi::x86::SEGMENTS_SIZE]
        .to_vec();
    for is_resident in [false, true] {
        for entries in [false, true] {
            for (index, byte) in engine.arena_mut().unwrap().iter_mut().enumerate() {
                *byte = (index.wrapping_mul(37).wrapping_add(11) & 0xff) as u8;
            }
            engine.arena_mut().unwrap()[segment_offset..segment_offset + segment_record.len()]
                .copy_from_slice(&segment_record);
            describe(&mut engine, CODE, 4, entries);
            let before = engine.arena().to_vec();
            assert_eq!(before.len(), ring3_engine::abi::arena::ARENA_SIZE);
            crate::support_fetch::assert_host_contract(
                (bound_compile(&mut engine, is_resident, entries))
                    .expect_err("compilation must refuse this fixture"),
                if is_resident {
                    HostError::Resident(RegistryError::Compile(error))
                } else {
                    HostError::Compile(error)
                },
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
