use iced_x86::{Code, Decoder, DecoderOptions};
use ring3_engine::{
    abi::arena::{TRANSFER_OFFSET, TRANSFER_SIZE},
    cpu::{
        UnsupportedFeature,
        dbt::{
            BlockSpec, CompileError, CompileLimits, InstructionError, compile_entry_region,
            compile_region,
        },
        x86::{
            Register32,
            decode::{DecodeError, decode_one},
            ir::{Operation, RotateKind, ShiftCount},
        },
    },
    memory::{
        Access, AddressSpace, FaultReason, GuestAddress, MemoryFault, PageRange, Permissions,
    },
    process::{EngineInstance, HostError},
};
use std::{fs, fs::OpenOptions, io::Write, path::PathBuf};

const CODE: u32 = 0x1000;
const KEY: u64 = 0x574f_5244_4341_5259;

fn admits(instruction: &[u8]) -> EngineInstance {
    let mut code = instruction.to_vec();
    code.extend([0xeb, 0]);
    let mut engine = EngineInstance::new(1, KEY).unwrap();
    engine.map(CODE, 1, 7).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + code.len()]
        .copy_from_slice(&code);
    engine.upload(CODE, code.len() as u32).unwrap();
    engine.protect(CODE, 1, 4).unwrap();
    let request = &mut engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8];
    request[..4].copy_from_slice(&CODE.to_le_bytes());
    request[4..].copy_from_slice(&(code.len() as u32).to_le_bytes());
    let before = engine.arena().to_vec();
    assert_eq!(
        engine.compile(1),
        Ok(1),
        "WORD register carry rotation must compile through the existing public API"
    );
    assert_eq!(engine.arena(), before);
    assert_eq!(engine.memory().unwrap().mapped_pages(), 1);
    assert!(!engine.artifact_bytes().unwrap().is_empty());
    engine
}

#[test]
fn word_carry_one_admits_in_bound_engine() {
    admits(&[0x66, 0xd1, 0xd0]);
}

#[test]
fn word_carry_immediate_admits_in_bound_engine() {
    admits(&[0x66, 0xc1, 0xd9, 18]);
}

#[test]
fn word_carry_cl_admits_in_bound_engine() {
    admits(&[0x66, 0xd3, 0xd1]);
}

#[test]
fn old_carry_and_word_rotate_modules_optionally_capture_without_execution() {
    let directory = std::env::var_os("RING3_WORD_CARRY_BASELINE_DIR").map(PathBuf::from);
    if let Some(directory) = &directory {
        let metadata = fs::symlink_metadata(directory).unwrap();
        assert!(metadata.is_dir() && !metadata.file_type().is_symlink());
    }
    let cases: [(&str, &[u8]); 18] = [
        ("byte-rcl-one", &[0xd0, 0xd0]),
        ("byte-rcr-one", &[0xd0, 0xd8]),
        ("dword-rcl-one", &[0xd1, 0xd0]),
        ("dword-rcr-one", &[0xd1, 0xd8]),
        ("byte-rcl-immediate", &[0xc0, 0xd0, 18]),
        ("byte-rcr-immediate", &[0xc0, 0xd8, 18]),
        ("dword-rcl-immediate", &[0xc1, 0xd0, 18]),
        ("dword-rcr-immediate", &[0xc1, 0xd8, 18]),
        ("byte-rcl-cl", &[0xd2, 0xd0]),
        ("byte-rcr-cl", &[0xd2, 0xd8]),
        ("dword-rcl-cl", &[0xd3, 0xd0]),
        ("dword-rcr-cl", &[0xd3, 0xd8]),
        ("word-rol-one", &[0x66, 0xd1, 0xc0]),
        ("word-ror-one", &[0x66, 0xd1, 0xc8]),
        ("word-rol-immediate", &[0x66, 0xc1, 0xc0, 0xff]),
        ("word-ror-immediate", &[0x66, 0xc1, 0xc8, 0xff]),
        ("word-rol-cl", &[0x66, 0xd3, 0xc0]),
        ("word-ror-cl", &[0x66, 0xd3, 0xc8]),
    ];
    for (name, instruction) in cases {
        let engine = admits(instruction);
        let bytes = engine.artifact_bytes().unwrap();
        assert_eq!(&bytes[..8], &[0, 97, 115, 109, 1, 0, 0, 0]);
        if let Some(directory) = &directory {
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(directory.join(format!("{name}.wasm")))
                .unwrap()
                .write_all(bytes)
                .unwrap();
        }
    }
}

const PARENTS: [Register32; 8] = [
    Register32::Eax,
    Register32::Ecx,
    Register32::Edx,
    Register32::Ebx,
    Register32::Esp,
    Register32::Ebp,
    Register32::Esi,
    Register32::Edi,
];

fn encoding(kind: RotateKind, opcode: u8, alias: u8, raw: u8) -> Vec<u8> {
    let field = if kind == RotateKind::Left { 0x10 } else { 0x18 };
    let mut bytes = vec![0x66, opcode, 0xc0 | field | alias];
    if opcode == 0xc1 {
        bytes.push(raw);
    }
    bytes
}

fn code_space(pc: u32, bytes: &[u8]) -> AddressSpace {
    let pages = (u64::from(pc & 0xfff) + bytes.len() as u64).div_ceil(4096) as u32;
    let mut memory = AddressSpace::new(pages + 1).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(pc & !0xfff), pages).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory.write(GuestAddress(pc), bytes).unwrap();
    memory
}

#[test]
fn all_word_carry_parents_and_raw_count_bytes_keep_typed_identity() {
    let mut memory = code_space(CODE, &[0x90]);
    let mut typed = 0;
    for kind in [RotateKind::Left, RotateKind::Right] {
        for (alias, destination) in PARENTS.into_iter().enumerate() {
            for opcode in [0xd1, 0xc1, 0xd3] {
                let raw_counts: Vec<u8> = if opcode == 0xc1 {
                    (0..=u8::MAX).collect()
                } else {
                    vec![1]
                };
                for raw in raw_counts {
                    let bytes = encoding(kind, opcode, alias as u8, raw);
                    memory.write(GuestAddress(CODE), &bytes).unwrap();
                    let decoded = decode_one(&memory, GuestAddress(CODE)).unwrap();
                    let count = if opcode == 0xd3 {
                        ShiftCount::Cl
                    } else {
                        ShiftCount::Immediate(raw)
                    };
                    assert_eq!(
                        decoded.operation(),
                        &Operation::RotateThroughCarryWord {
                            kind,
                            destination,
                            count,
                        },
                        "{bytes:02x?}"
                    );
                    let code = match (kind, opcode) {
                        (RotateKind::Left, 0xd1) => Code::Rcl_rm16_1,
                        (RotateKind::Right, 0xd1) => Code::Rcr_rm16_1,
                        (RotateKind::Left, 0xc1) => Code::Rcl_rm16_imm8,
                        (RotateKind::Right, 0xc1) => Code::Rcr_rm16_imm8,
                        (RotateKind::Left, 0xd3) => Code::Rcl_rm16_CL,
                        (RotateKind::Right, 0xd3) => Code::Rcr_rm16_CL,
                        _ => unreachable!(),
                    };
                    assert_eq!(
                        Decoder::with_ip(32, &bytes, u64::from(CODE), DecoderOptions::NONE)
                            .decode()
                            .code(),
                        code
                    );
                    assert_eq!(decoded.length() as usize, bytes.len());
                    assert_eq!(decoded.next_pc(), GuestAddress(CODE + bytes.len() as u32));
                    assert!(memory.is_code_current(decoded.code_snapshot()));
                    typed += 1;
                }
            }
        }
    }
    assert_eq!(typed, 4128);
}

#[test]
fn word_carry_extra_prefixes_and_memory_neighbors_remain_precisely_closed() {
    let opcode_error = DecodeError::Unsupported(UnsupportedFeature::Opcode);
    let segment_error = DecodeError::Unsupported(UnsupportedFeature::Segment);
    let mut memory = code_space(CODE, &[0x90]);
    let mut rejected = |bytes: &[u8], expected| {
        memory.write(GuestAddress(CODE), bytes).unwrap();
        assert_eq!(
            decode_one(&memory, GuestAddress(CODE)).err(),
            Some(expected),
            "{bytes:02x?}"
        );
    };
    for kind in [RotateKind::Left, RotateKind::Right] {
        for opcode in [0xd1, 0xc1, 0xd3] {
            let bytes = encoding(kind, opcode, 1, 18);
            for prefix in [0x66, 0x67, 0xf2, 0xf3] {
                rejected(&[vec![prefix], bytes.clone()].concat(), opcode_error);
            }
            rejected(
                &[vec![0x66, 0x67], bytes[1..].to_vec()].concat(),
                opcode_error,
            );
            for prefix in [0x26, 0x2e, 0x36, 0x3e, 0x64, 0x65] {
                rejected(&[vec![prefix], bytes.clone()].concat(), segment_error);
            }
            rejected(
                &[vec![0xf0], bytes.clone()].concat(),
                DecodeError::InvalidEncoding,
            );
            let mut byte = bytes.clone();
            byte[1] -= 1;
            rejected(&byte, opcode_error);
            for tail in [
                &[0x03][..],
                &[0x04, 0x24],
                &[0x05, 0x78, 0x56, 0x34, 0x12],
                &[0x43, 0x80],
                &[0x83, 0x78, 0x56, 0x34, 0x12],
                &[0x04, 0x8d, 0x78, 0x56, 0x34, 0x12],
            ] {
                let mut closed = vec![0x66, opcode];
                closed.extend_from_slice(tail);
                closed[2] |= if kind == RotateKind::Left { 0x10 } else { 0x18 };
                if opcode == 0xc1 {
                    closed.push(18);
                }
                closed.insert(0, 0x66);
                rejected(&closed, opcode_error);
            }
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
fn word_carry_fetch_truncation_and_top_address_boundaries_remain_exact() {
    for kind in [RotateKind::Left, RotateKind::Right] {
        for opcode in [0xd1, 0xc1, 0xd3] {
            let bytes = encoding(kind, opcode, 1, 0xf3);
            for pc in [
                0x2000 - bytes.len() as u32,
                0x1fff,
                u32::MAX - bytes.len() as u32 + 1,
            ] {
                let memory = code_space(pc, &bytes);
                let decoded = decode_one(&memory, GuestAddress(pc)).unwrap();
                assert_eq!(decoded.length() as usize, bytes.len());
                assert_eq!(
                    decoded.next_pc(),
                    GuestAddress(pc.wrapping_add(bytes.len() as u32))
                );
            }
            for cut in 1..bytes.len() {
                let pc = 0x2000 - cut as u32;
                assert_eq!(
                    decode_one(&code_space(pc, &bytes[..cut]), GuestAddress(pc)).err(),
                    Some(fetch_error(pc, 0x2000, FaultReason::Unmapped, cut + 1))
                );
                let pc = u32::MAX - cut as u32 + 1;
                assert_eq!(
                    decode_one(&code_space(pc, &bytes[..cut]), GuestAddress(pc)).err(),
                    Some(fetch_error(pc, pc, FaultReason::AddressOverflow, cut + 1))
                );
            }
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
        }
    }
}

#[test]
fn every_word_carry_consumed_byte_tracks_same_and_changed_writes() {
    for kind in [RotateKind::Left, RotateKind::Right] {
        for opcode in [0xd1, 0xc1, 0xd3] {
            let bytes = encoding(kind, opcode, 1, 0x66);
            for (offset, original) in bytes.iter().copied().enumerate() {
                for replacement in [original, original ^ 1] {
                    let mut memory = code_space(0x1fff, &bytes);
                    let decoded = decode_one(&memory, GuestAddress(0x1fff)).unwrap();
                    memory
                        .write(GuestAddress(0x1fff + offset as u32), &[replacement])
                        .unwrap();
                    assert!(!memory.is_code_current(decoded.code_snapshot()));
                }
            }
            let mut memory = code_space(CODE, &bytes);
            memory
                .map_zeroed(
                    PageRange::new(GuestAddress(0x5000), 1).unwrap(),
                    Permissions::ALL,
                )
                .unwrap();
            let decoded = decode_one(&memory, GuestAddress(CODE)).unwrap();
            memory
                .write(GuestAddress(0x5000), &[0x66, 0x67, 0xf0])
                .unwrap();
            memory
                .protect(
                    PageRange::new(GuestAddress(0x5000), 1).unwrap(),
                    Permissions::READ,
                )
                .unwrap();
            assert!(memory.is_code_current(decoded.code_snapshot()));
        }
    }
}

fn upload(engine: &mut EngineInstance, pc: u32, bytes: &[u8]) {
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(bytes);
    engine.upload(pc, bytes.len() as u32).unwrap();
}

fn fixture(bytes: &[u8]) -> EngineInstance {
    let mut engine = EngineInstance::new(3, KEY).unwrap();
    engine.map(CODE, 1, 7).unwrap();
    upload(&mut engine, CODE, bytes);
    engine.protect(CODE, 1, 4).unwrap();
    engine
}

fn describe(engine: &mut EngineInstance, pc: u32, length: usize, entries: bool) {
    let transfer =
        &mut engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + TRANSFER_SIZE];
    transfer.fill(0xa5);
    transfer[..4].copy_from_slice(&pc.to_le_bytes());
    if !entries {
        transfer[4..8].copy_from_slice(&(length as u32).to_le_bytes());
    }
}

fn bound(engine: &mut EngineInstance, resident: bool, entries: bool) -> Result<u64, HostError> {
    match (resident, entries) {
        (false, false) => engine.compile(1).map(u64::from),
        (false, true) => engine.compile_entries(1, 0).map(u64::from),
        (true, false) => engine.compile_resident(1).map(|id| id.get()),
        (true, true) => engine.compile_resident_entries(1, 0).map(|id| id.get()),
    }
}

fn artifact(engine: &EngineInstance, resident: bool, id: u64) -> &[u8] {
    if resident {
        engine.resident_bytes(id).unwrap()
    } else {
        engine.artifact_bytes().unwrap()
    }
}

struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn take(&mut self, length: usize) -> &'a [u8] {
        let (value, rest) = self.0.split_at(length);
        self.0 = rest;
        value
    }
    fn byte(&mut self) -> u8 {
        self.take(1)[0]
    }
    fn unsigned(&mut self) -> u32 {
        let mut value = 0;
        for shift in (0..35).step_by(7) {
            let byte = self.byte();
            assert!(shift < 28 || byte & 0xf0 == 0, "invalid u32 LEB128");
            value |= u32::from(byte & 127) << shift;
            if byte & 128 == 0 {
                return value;
            }
        }
        panic!("unterminated u32 LEB128")
    }
    fn name(&mut self) -> &'a str {
        let length = self.unsigned() as usize;
        std::str::from_utf8(self.take(length)).unwrap()
    }
    fn end(self) {
        assert!(self.0.is_empty());
    }
}

fn section(wasm: &[u8], requested: u8) -> Reader<'_> {
    assert_eq!(&wasm[..8], b"\0asm\x01\0\0\0");
    let mut reader = Reader(&wasm[8..]);
    while !reader.0.is_empty() {
        let kind = reader.byte();
        let length = reader.unsigned() as usize;
        let bytes = reader.take(length);
        if kind == requested {
            return Reader(bytes);
        }
    }
    panic!("missing Wasm section {requested}")
}

fn pure_module_interface(wasm: &[u8], resident: Option<bool>) {
    let mut types = section(wasm, 1);
    assert_eq!(types.unsigned(), if resident.is_some() { 2 } else { 1 });
    for arity in [4, if resident == Some(true) { 7 } else { 6 }]
        .into_iter()
        .take(if resident.is_some() { 2 } else { 1 })
    {
        assert_eq!(types.byte(), 0x60);
        assert_eq!(types.unsigned(), arity);
        assert!(types.take(arity as usize).iter().all(|byte| *byte == 0x7f));
        assert_eq!(types.unsigned(), 1);
        assert_eq!(types.byte(), 0x7f);
    }
    types.end();
    let mut imports = section(wasm, 2);
    assert_eq!(imports.unsigned(), if resident.is_some() { 2 } else { 1 });
    assert_eq!(
        (imports.name(), imports.name(), imports.byte()),
        ("env", "memory", 2)
    );
    assert_eq!((imports.unsigned(), imports.unsigned()), (0, 1));
    if let Some(resident) = resident {
        assert_eq!(
            (
                imports.name(),
                imports.name(),
                imports.byte(),
                imports.unsigned()
            ),
            (
                "ring3",
                if resident { "guard_resident" } else { "guard" },
                0,
                1
            )
        );
    }
    imports.end();
    let mut exports = section(wasm, 7);
    assert_eq!(exports.unsigned(), 1);
    assert_eq!(exports.name(), "run");
    assert_eq!(exports.byte(), 0);
    assert_eq!(exports.unsigned(), u32::from(resident.is_some()));
    exports.end();
    let mut bodies = section(wasm, 10);
    assert_eq!(bodies.unsigned(), 1);
    let length = bodies.unsigned() as usize;
    let mut body = Reader(bodies.take(length));
    bodies.end();
    assert_eq!(body.unsigned(), 2);
    assert_eq!((body.unsigned(), body.byte()), (16, 0x7f));
    assert_eq!((body.unsigned(), body.byte()), (1, 0x7e));
}

#[test]
fn all_word_carry_shapes_compile_in_six_pure_profiles_without_data_imports() {
    let mut profiles = 0;
    for kind in [RotateKind::Left, RotateKind::Right] {
        for opcode in [0xd1, 0xc1, 0xd3] {
            for alias in 0..8 {
                let mut bytes = encoding(kind, opcode, alias, 18);
                bytes.extend([0xeb, 0]);
                let memory = code_space(CODE, &bytes);
                for entries in [false, true] {
                    let region = if entries {
                        compile_entry_region(
                            &memory,
                            &[GuestAddress(CODE)],
                            CompileLimits::default(),
                        )
                    } else {
                        compile_region(
                            &memory,
                            &[BlockSpec {
                                entry: GuestAddress(CODE),
                                byte_length: bytes.len() as u32,
                            }],
                            CompileLimits::default(),
                        )
                    }
                    .unwrap();
                    assert_eq!(
                        (region.metadata().blocks, region.metadata().instructions),
                        (1, 2)
                    );
                    pure_module_interface(region.wasm_bytes(&memory).unwrap(), None);
                    profiles += 1;
                }
                for resident in [false, true] {
                    for entries in [false, true] {
                        let mut engine = fixture(&bytes);
                        describe(&mut engine, CODE, bytes.len(), entries);
                        let before = engine.arena().to_vec();
                        let id = bound(&mut engine, resident, entries).unwrap();
                        assert_eq!(engine.arena(), before);
                        assert_eq!(engine.memory().unwrap().mapped_pages(), 1);
                        assert!(
                            engine
                                .memory()
                                .unwrap()
                                .resolve(GuestAddress(0x5000), Access::Read)
                                .is_err()
                        );
                        pure_module_interface(artifact(&engine, resident, id), Some(resident));
                        if resident {
                            engine.guard_resident(KEY, id).unwrap();
                        } else {
                            engine.guard(KEY, id as u32).unwrap();
                        }
                        profiles += 1;
                    }
                }
            }
        }
    }
    assert_eq!(profiles, 288);
}

#[test]
fn late_word_carry_refusals_and_caps_preserve_both_previous_owners() {
    let keep_pc = 0x3000;
    let mut failures = 0;
    for resident in [false, true] {
        for entries in [false, true] {
            let mut engine = fixture(&[0x90, 0xeb, 0]);
            engine.map(keep_pc, 1, 7).unwrap();
            upload(&mut engine, keep_pc, &[0x90, 0xeb, 0]);
            describe(&mut engine, keep_pc, 3, false);
            engine.compile(1).unwrap();
            let keep = engine.compile_resident(1).unwrap().get();
            for kind in [RotateKind::Left, RotateKind::Right] {
                for opcode in [0xd1, 0xc1, 0xd3] {
                    let first = encoding(kind, opcode, 1, 18);
                    let mut bytes = first.clone();
                    bytes.extend([0x66, 0x66, 0xd1, 0xd0, 0xeb, 0]);
                    engine.protect(CODE, 1, 7).unwrap();
                    upload(&mut engine, CODE, &bytes);
                    engine.protect(CODE, 1, 4).unwrap();
                    describe(&mut engine, CODE, bytes.len(), entries);
                    let arena = engine.arena().to_vec();
                    let arena_pointer = engine.arena().as_ptr();
                    let generation = engine.generation();
                    let old = engine.artifact_bytes().unwrap().to_vec();
                    let old_pointer = engine.artifact_bytes().unwrap().as_ptr();
                    let current = engine.resident_bytes(keep).unwrap().to_vec();
                    let current_pointer = engine.resident_bytes(keep).unwrap().as_ptr();
                    let cause = CompileError::Instruction {
                        pc: GuestAddress(CODE + first.len() as u32),
                        cause: InstructionError::Decode(DecodeError::Unsupported(
                            UnsupportedFeature::Opcode,
                        )),
                    };
                    let expected = if resident {
                        HostError::Resident(ring3_engine::cpu::dbt::RegistryError::Compile(cause))
                    } else {
                        HostError::Compile(cause)
                    };
                    assert_eq!(bound(&mut engine, resident, entries), Err(expected));
                    assert_eq!(engine.arena(), arena);
                    assert_eq!(engine.arena().as_ptr(), arena_pointer);
                    assert_eq!(engine.generation(), generation);
                    assert_eq!(engine.artifact_bytes().unwrap(), old);
                    assert_eq!(engine.artifact_bytes().unwrap().as_ptr(), old_pointer);
                    assert_eq!(engine.resident_bytes(keep).unwrap(), current);
                    assert_eq!(
                        engine.resident_bytes(keep).unwrap().as_ptr(),
                        current_pointer
                    );
                    engine.guard(KEY, generation).unwrap();
                    engine.guard_resident(KEY, keep).unwrap();
                    failures += 1;
                }
            }
        }
    }
    assert_eq!(failures, 24);
    let mut bytes = encoding(RotateKind::Left, 0xd3, 1, 0);
    bytes.extend([0xeb, 0]);
    let memory = code_space(CODE, &bytes);
    let limits = CompileLimits {
        instructions: 1,
        ..CompileLimits::default()
    };
    assert_eq!(
        compile_region(
            &memory,
            &[BlockSpec {
                entry: GuestAddress(CODE),
                byte_length: bytes.len() as u32
            }],
            limits
        )
        .err(),
        Some(CompileError::InstructionLimit)
    );
    assert_eq!(
        compile_entry_region(&memory, &[GuestAddress(CODE)], limits).err(),
        Some(CompileError::InstructionLimit)
    );
}
