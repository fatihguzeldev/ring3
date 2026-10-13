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
    memory::{
        Access, AddressSpace, FaultReason, GuestAddress, MemoryFault, PageRange, Permissions,
    },
    process::{EngineInstance, HostError},
};

const CODE: u32 = 0x1000;
const KEY: u64 = 0x414c_4947_4e4e_4f50;
const NOPS: [&[u8]; 3] = [
    &[0x66, 0x0f, 0x1f, 0x44, 0, 0],
    &[0x66, 0x0f, 0x1f, 0x84, 0, 0, 0, 0, 0],
    &[0x66, 0x2e, 0x0f, 0x1f, 0x84, 0, 0, 0, 0, 0],
];

fn code(bytes: &[u8]) -> EngineInstance {
    let mut engine = EngineInstance::new(2, KEY).unwrap();
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
    let memory = engine.memory().unwrap();
    let decoded = decode_one(memory, GuestAddress(CODE))
        .expect("canonical compiler alignment NOP must decode");
    assert_eq!(decoded.operation(), &Operation::Nop);
    assert_eq!(decoded.length() as usize, bytes.len());
    assert_eq!(decoded.next_pc(), GuestAddress(CODE + bytes.len() as u32));
    assert!(memory.is_code_current(decoded.code_snapshot()));
    assert_eq!(memory.mapped_pages(), 1);
    assert_eq!(engine.arena(), before);
}

#[test]
fn alignment_nop_six_byte_form_decodes() {
    decode_admission(NOPS[0]);
}

#[test]
fn alignment_nop_nine_byte_form_decodes() {
    decode_admission(NOPS[1]);
}

#[test]
fn alignment_nop_ten_byte_cs_form_decodes() {
    decode_admission(NOPS[2]);
}

fn bank() -> Vec<u8> {
    let mut bytes = NOPS.concat();
    bytes.extend([0xeb, 0]);
    bytes
}

fn standalone_admission(entries: bool) {
    let bytes = bank();
    assert_standalone(&bytes, 4, entries);
}

fn assert_standalone(bytes: &[u8], instructions: usize, entries: bool) {
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
    .expect("canonical compiler alignment NOPs must compile standalone");
    assert_eq!(
        (compiled.metadata().blocks, compiled.metadata().instructions),
        (1, instructions)
    );
    let module = compiled.wasm_bytes(memory).unwrap();
    assert_eq!(&module[..8], b"\0asm\x01\0\0\0");
    assert_helper_free(module);
    assert_eq!(memory.mapped_pages(), 1);
}

#[test]
fn alignment_nop_compiles_standalone_explicit() {
    standalone_admission(false);
}

#[test]
fn alignment_nop_compiles_standalone_entry() {
    standalone_admission(true);
}

fn bound_admission(resident: bool, entries: bool) {
    let bytes = bank();
    assert_bound(&bytes, resident, entries);
}

fn assert_bound(bytes: &[u8], resident: bool, entries: bool) {
    let mut engine = code(bytes);
    describe(&mut engine, CODE, bytes.len() as u32, entries);
    let before = engine.arena().to_vec();
    let id = bound_compile(&mut engine, resident, entries)
        .expect("canonical compiler alignment NOPs must compile bound");
    let module = if resident {
        engine.resident_bytes(id).unwrap()
    } else {
        engine.artifact_bytes().unwrap()
    };
    assert_eq!(&module[..8], b"\0asm\x01\0\0\0");
    assert_helper_free(module);
    assert_eq!(engine.memory().unwrap().mapped_pages(), 1);
    assert_eq!(engine.arena(), before);
}

#[test]
fn alignment_nop_compiles_replacement_explicit() {
    bound_admission(false, false);
}

#[test]
fn alignment_nop_compiles_replacement_entry() {
    bound_admission(false, true);
}

#[test]
fn alignment_nop_compiles_resident_explicit() {
    bound_admission(true, false);
}

#[test]
fn alignment_nop_compiles_resident_entry() {
    bound_admission(true, true);
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

fn assert_helper_free(module: &[u8]) {
    for name in [b"read".as_slice(), b"store", b"dispatch"] {
        assert!(!module.windows(name.len()).any(|bytes| bytes == name));
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
fn alignment_nop_exact_grammar_keeps_prefix_segment_and_payload_neighbors_closed() {
    let opcode = DecodeError::Unsupported(UnsupportedFeature::Opcode);

    for (index, bytes) in NOPS.into_iter().enumerate() {
        for prefix in [
            0x66, 0x67, 0xf0, 0xf2, 0xf3, 0x26, 0x2e, 0x36, 0x3e, 0x64, 0x65,
        ] {
            let prefixed = [vec![prefix], bytes.to_vec()].concat();
            let expected = if prefix == 0xf0 {
                DecodeError::InvalidEncoding
            } else {
                opcode
            };
            let (prefixed, expected) =
                if matches!(prefix, 0x67 | 0x26 | 0x2e | 0x36 | 0x3e | 0x64 | 0x65) {
                    assert_eq!(prefix_admission::admitted(&prefixed).0, Operation::Nop);
                    prefix_admission::malformed_fixture(prefixed.len())
                } else {
                    (prefixed, expected)
                };
            assert_eq!(
                decode_one(&code_space(CODE, &prefixed), GuestAddress(CODE)).err(),
                Some(expected),
                "{prefixed:02x?}"
            );
        }
        for changed in [
            bytes.len() - 1,
            bytes.len() - if index == 0 { 2 } else { 5 },
        ] {
            let mut neighbor = bytes.to_vec();
            neighbor[changed] = 1;
            assert_eq!(
                prefix_admission::admitted(&neighbor),
                (Operation::Nop, neighbor.len())
            );
            let (neighbor, expected) = prefix_admission::malformed_fixture(neighbor.len());
            assert_eq!(
                decode_one(&code_space(CODE, &neighbor), GuestAddress(CODE)).err(),
                Some(expected),
                "{neighbor:02x?}"
            );
        }
    }
    for bytes in [
        &[0x66, 0x0f, 0x1f, 0x00][..],
        &[0x66, 0x0f, 0x1f, 0xc0],
        &[0x2e, 0x66, 0x0f, 0x1f, 0x84, 0, 0, 0, 0, 0],
        &[0x2e, 0x8b, 0],
        &[0x66, 0x2e, 0x8b, 0],
    ] {
        prefix_admission::admitted(bytes);
        let (bytes, expected) = prefix_admission::malformed_fixture(bytes.len());
        assert_eq!(
            decode_one(&code_space(CODE, &bytes), GuestAddress(CODE)).err(),
            Some(expected),
            "{bytes:02x?}"
        );
    }
    for bytes in [
        &[0x90][..],
        &[0x66, 0x90],
        &[0x0f, 0x1f, 0],
        &[0x0f, 0x1f, 0xc0],
        &[0x0f, 0x1f, 0x44, 0, 0],
    ] {
        decode_admission(bytes);
    }
}

fn fetch_error(pc: u32, address: u32, length: usize, reason: FaultReason) -> DecodeError {
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
fn alignment_nop_fetches_every_byte_without_overreading_page_or_wrapping_top() {
    for bytes in NOPS {
        for pc in [
            0x2000 - bytes.len() as u32,
            0x1fff,
            u32::MAX - bytes.len() as u32 + 1,
        ] {
            let memory = code_space(pc, bytes);
            let decoded = decode_one(&memory, GuestAddress(pc)).unwrap();
            assert_eq!(decoded.operation(), &Operation::Nop);
            assert_eq!(
                (decoded.length() as usize, decoded.next_pc()),
                (
                    bytes.len(),
                    GuestAddress(pc.wrapping_add(bytes.len() as u32))
                )
            );
            assert!(memory.is_code_current(decoded.code_snapshot()));
            assert!(memory.resolve(GuestAddress(pc), Access::Read).is_err());
        }
        for cut in 1..bytes.len() {
            let pc = 0x2000 - cut as u32;
            let mut memory = code_space(pc, &bytes[..cut]);
            assert_eq!(
                decode_one(&memory, GuestAddress(pc)).err(),
                Some(fetch_error(pc, 0x2000, cut + 1, FaultReason::Unmapped))
            );
            memory
                .map_zeroed(
                    PageRange::new(GuestAddress(0x2000), 1).unwrap(),
                    Permissions::READ,
                )
                .unwrap();
            assert_eq!(
                decode_one(&memory, GuestAddress(pc)).err(),
                Some(fetch_error(pc, 0x2000, cut + 1, FaultReason::Permission))
            );
            let top = u32::MAX - cut as u32 + 1;
            let mut memory = code_space(top, &bytes[..cut]);
            let zero = PageRange::new(GuestAddress(0), 1).unwrap();
            memory.map_zeroed(zero, Permissions::ALL).unwrap();
            memory.write(GuestAddress(0), &bytes[cut..]).unwrap();
            memory.protect(zero, Permissions::EXECUTE).unwrap();
            assert_eq!(
                decode_one(&memory, GuestAddress(top)).err(),
                Some(fetch_error(top, top, cut + 1, FaultReason::AddressOverflow))
            );
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
            Some(fetch_error(CODE, CODE, 1, FaultReason::Permission))
        );
    }
}

#[test]
fn alignment_nop_is_ordinary_fallthrough_and_prefix_payloads_stay_instruction_data() {
    for bytes in NOPS {
        let payload = [0x66, 0x2e, 0xf0, 0x67];
        let encoded = [
            vec![0xb8],
            payload.to_vec(),
            bytes.to_vec(),
            vec![0x90, 0xeb, 0],
        ]
        .concat();
        let memory = code_space(CODE, &encoded);
        let first = decode_one(&memory, GuestAddress(CODE)).unwrap();
        assert_eq!(
            first.operation(),
            &Operation::Move {
                destination: Location32::Register(Register32::Eax),
                source: Value32::Immediate(0x67f0_2e66)
            }
        );
        assert_eq!(
            (first.length(), first.next_pc()),
            (5, GuestAddress(CODE + 5))
        );
        let nop = decode_one(&memory, first.next_pc()).unwrap();
        assert_eq!(nop.operation(), &Operation::Nop);
        assert_eq!(nop.length() as usize, bytes.len());
        let next = decode_one(&memory, nop.next_pc()).unwrap();
        assert_eq!((next.operation(), next.length()), (&Operation::Nop, 1));
        for entries in [false, true] {
            assert_standalone(&encoded, 4, entries);
            for resident in [false, true] {
                assert_bound(&encoded, resident, entries);
            }
        }
    }
}

#[test]
fn alignment_nop_source_prefix_and_final_displacement_identity_are_tracked() {
    for bytes in NOPS {
        for changed in [0, bytes.len() - 1] {
            let mut engine = code(bytes);
            engine.protect(CODE, 1, 7).unwrap();
            describe(&mut engine, CODE, bytes.len() as u32, false);
            let replacement = bound_compile(&mut engine, false, false).unwrap() as u32;
            let resident = bound_compile(&mut engine, true, false).unwrap();
            let decoded = decode_one(engine.memory().unwrap(), GuestAddress(CODE)).unwrap();
            engine.map(0x5000, 1, 3).unwrap();
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
            engine.arena_mut().unwrap()[TRANSFER_OFFSET] = bytes[changed];
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
        let mut engine = code(bytes);
        describe(&mut engine, CODE, bytes.len() as u32, false);
        let replacement = bound_compile(&mut engine, false, false).unwrap() as u32;
        let resident = bound_compile(&mut engine, true, false).unwrap();
        engine.protect(CODE, 1, 4).unwrap();
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
fn alignment_nop_exact_64_instruction_unit_fits_all_six_profiles() {
    let mut bytes = Vec::new();
    for index in 0..63 {
        bytes.extend(NOPS[index % 3]);
    }
    bytes.extend([0xeb, 0]);
    for entries in [false, true] {
        assert_standalone(&bytes, 64, entries);
        for resident in [false, true] {
            assert_bound(&bytes, resident, entries);
        }
    }
}

#[test]
fn alignment_nop_late_refusal_preserves_dirty_full_arena_and_both_owners() {
    let mut bytes = NOPS.concat();
    let refused_pc = CODE + bytes.len() as u32;
    bytes.extend([0xf0, 0x90]);
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
    let error = CompileError::Instruction {
        pc: GuestAddress(refused_pc),
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
            describe(&mut engine, CODE, refused_length, entries);
            let before = engine.arena().to_vec();
            assert_eq!(before.len(), ring3_engine::abi::arena::ARENA_SIZE);
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

#[test]
fn endbranch_forms_are_four_byte_nops_without_cet() {
    use iced_x86::{Code, Decoder, DecoderOptions};

    for (bytes, expected) in [
        ([0xf3, 0x0f, 0x1e, 0xfb], Code::Endbr32),
        ([0xf3, 0x0f, 0x1e, 0xfa], Code::Endbr64),
    ] {
        let decoded = Decoder::with_ip(32, &bytes, u64::from(CODE), DecoderOptions::NONE).decode();
        assert_eq!(decoded.code(), expected);
        assert_eq!(decoded.len(), 4);
        assert_eq!(decoded.op_count(), 0);
        decode_admission(&bytes);
    }
}

#[test]
fn endbranch_nops_compile_without_operand_helpers_in_all_six_profiles() {
    let bytes = [0xf3, 0x0f, 0x1e, 0xfb, 0xf3, 0x0f, 0x1e, 0xfa, 0xeb, 0];
    for entries in [false, true] {
        assert_standalone(&bytes, 3, entries);
        for resident in [false, true] {
            assert_bound(&bytes, resident, entries);
        }
    }
}

#[test]
fn endbranch_identity_requires_f3_and_lock_remains_invalid() {
    use iced_x86::{Code, Decoder, DecoderOptions};

    for final_byte in [0xfb, 0xfa] {
        for prefix in [&[][..], &[0xf2]] {
            let mut bytes = prefix.to_vec();
            bytes.extend([0x0f, 0x1e, final_byte]);
            let decoded =
                Decoder::with_ip(32, &bytes, u64::from(CODE), DecoderOptions::NONE).decode();
            assert!(!matches!(decoded.code(), Code::Endbr32 | Code::Endbr64));
        }
        let repeated = [0xf3, 0xf3, 0x0f, 0x1e, final_byte];
        assert_eq!(
            decode_one(&code_space(CODE, &repeated), GuestAddress(CODE)).err(),
            Some(DecodeError::Unsupported(UnsupportedFeature::Opcode))
        );
        let locked = [0xf0, 0xf3, 0x0f, 0x1e, final_byte];
        assert_eq!(
            decode_one(&code_space(CODE, &locked), GuestAddress(CODE)).err(),
            Some(DecodeError::InvalidEncoding)
        );
    }
}

#[test]
fn endbranch_fetch_requires_the_final_opcode_byte_before_nop_admission() {
    for bytes in [[0xf3, 0x0f, 0x1e, 0xfb], [0xf3, 0x0f, 0x1e, 0xfa]] {
        for pc in [0x1ffc, 0xffff_fffc] {
            let memory = code_space(pc, &bytes);
            let decoded = decode_one(&memory, GuestAddress(pc)).unwrap();
            assert_eq!(decoded.operation(), &Operation::Nop);
            assert_eq!(decoded.length(), 4);
            assert_eq!(decoded.next_pc(), GuestAddress(pc.wrapping_add(4)));
            assert!(memory.is_code_current(decoded.code_snapshot()));
        }

        let pc = 0x1ffd;
        let mut memory = code_space(pc, &bytes[..3]);
        assert_eq!(
            decode_one(&memory, GuestAddress(pc)).err(),
            Some(fetch_error(pc, 0x2000, 4, FaultReason::Unmapped))
        );
        let second = PageRange::new(GuestAddress(0x2000), 1).unwrap();
        memory.map_zeroed(second, Permissions::ALL).unwrap();
        memory.write(GuestAddress(0x2000), &bytes[3..]).unwrap();
        memory.protect(second, Permissions::READ).unwrap();
        assert_eq!(
            decode_one(&memory, GuestAddress(pc)).err(),
            Some(fetch_error(pc, 0x2000, 4, FaultReason::Permission))
        );
        memory.protect(second, Permissions::EXECUTE).unwrap();
        let decoded = decode_one(&memory, GuestAddress(pc)).unwrap();
        assert_eq!(decoded.operation(), &Operation::Nop);
        assert_eq!(
            (decoded.length(), decoded.next_pc()),
            (4, GuestAddress(0x2001))
        );
        assert!(memory.is_code_current(decoded.code_snapshot()));
    }
}
