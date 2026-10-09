use ring3_engine::{
    abi::arena::TRANSFER_OFFSET,
    cpu::{
        dbt::{BlockSpec, CompileLimits, compile_entry_region, compile_region},
        x86::decode::decode_one,
    },
    memory::GuestAddress,
    process::EngineInstance,
};

const CODE: u32 = 0x1000;
const KEY: u64 = 0x574f_5244_4453_4831;

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
        .expect("WORD register double shift must decode");
    assert_eq!(
        (decoded.length(), decoded.next_pc()),
        (bytes.len() as u8, GuestAddress(CODE + bytes.len() as u32))
    );
    assert_eq!(engine.arena(), before);
    assert_eq!(engine.memory().unwrap().mapped_pages(), 1);
}

#[test]
fn word_shld_immediate_decodes() {
    decode_admission(&[0x66, 0x0f, 0xa4, 0xd0, 1]);
}

#[test]
fn word_shld_cl_decodes() {
    decode_admission(&[0x66, 0x0f, 0xa5, 0xd0]);
}

#[test]
fn word_shrd_immediate_decodes() {
    decode_admission(&[0x66, 0x0f, 0xac, 0xc9, 0xff]);
}

#[test]
fn word_shrd_cl_decodes() {
    decode_admission(&[0x66, 0x0f, 0xad, 0xd1]);
}

fn standalone_admission(entries: bool) {
    let bytes = [0x66, 0x0f, 0xa4, 0xd0, 1, 0xeb, 0];
    let engine = code(&bytes);
    let before = engine.arena().to_vec();
    let memory = engine.memory().unwrap();
    let artifact = if entries {
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
    .expect("WORD register double shift must compile standalone");
    assert!(!artifact.wasm_bytes(memory).unwrap().is_empty());
    assert_eq!(engine.arena(), before);
    assert_eq!(memory.mapped_pages(), 1);
}

#[test]
fn word_double_shift_compiles_standalone_explicit() {
    standalone_admission(false);
}

#[test]
fn word_double_shift_compiles_standalone_entry() {
    standalone_admission(true);
}

fn bound_admission(resident: bool, entries: bool) {
    let bytes = [0x66, 0x0f, 0xa4, 0xd0, 1, 0xeb, 0];
    let mut engine = code(&bytes);
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
    .expect("WORD register double shift must compile bound");
    let module = if resident {
        engine.resident_bytes(id).unwrap()
    } else {
        engine.artifact_bytes().unwrap()
    };
    assert!(!module.is_empty());
    assert_eq!(engine.arena(), before);
    assert_eq!(engine.memory().unwrap().mapped_pages(), 1);
}

#[test]
fn word_double_shift_compiles_replacement_explicit() {
    bound_admission(false, false);
}

#[test]
fn word_double_shift_compiles_replacement_entry() {
    bound_admission(false, true);
}

#[test]
fn word_double_shift_compiles_resident_explicit() {
    bound_admission(true, false);
}

#[test]
fn word_double_shift_compiles_resident_entry() {
    bound_admission(true, true);
}

use iced_x86::{Code, Decoder, DecoderOptions};
use ring3_engine::{
    abi::arena::TRANSFER_SIZE,
    cpu::{
        UnsupportedFeature,
        dbt::{CompileError, InstructionError, RegistryError},
        x86::{
            Register32,
            decode::DecodeError,
            ir::{DoubleShiftKind, Operation, ShiftCount},
        },
    },
    memory::{Access, AddressSpace, FaultReason, MemoryFault, PageRange, Permissions},
    process::HostError,
};

const KEEP: u32 = 0x3000;
const DATA: u32 = 0x5000;
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

fn encoding(kind: DoubleShiftKind, cl: bool, destination: u8, source: u8, raw: u8) -> Vec<u8> {
    let opcode = match (kind, cl) {
        (DoubleShiftKind::Left, false) => 0xa4,
        (DoubleShiftKind::Left, true) => 0xa5,
        (DoubleShiftKind::Right, false) => 0xac,
        (DoubleShiftKind::Right, true) => 0xad,
    };
    let mut bytes = vec![0x66, 0x0f, opcode, 0xc0 | source << 3 | destination];
    if !cl {
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
fn all_word_double_shift_raw_counts_and_parent_aliases_keep_typed_identity() {
    let mut memory = code_space(CODE, &[0x90]);
    let mut rows = 0;
    for kind in [DoubleShiftKind::Left, DoubleShiftKind::Right] {
        for (d, destination) in PARENTS.into_iter().enumerate() {
            for (s, source) in PARENTS.into_iter().enumerate() {
                for cl in [false, true] {
                    let counts: Vec<u8> = if cl { vec![0] } else { (0..=255).collect() };
                    for raw in counts {
                        let bytes = encoding(kind, cl, d as u8, s as u8, raw);
                        memory.write(GuestAddress(CODE), &bytes).unwrap();
                        let decoded = decode_one(&memory, GuestAddress(CODE)).unwrap();
                        assert_eq!(
                            decoded.operation(),
                            &Operation::DoubleShiftWord {
                                kind,
                                destination,
                                source,
                                count: if cl {
                                    ShiftCount::Cl
                                } else {
                                    ShiftCount::Immediate(raw)
                                },
                            },
                            "{bytes:02x?}"
                        );
                        let expected = match (kind, cl) {
                            (DoubleShiftKind::Left, false) => Code::Shld_rm16_r16_imm8,
                            (DoubleShiftKind::Left, true) => Code::Shld_rm16_r16_CL,
                            (DoubleShiftKind::Right, false) => Code::Shrd_rm16_r16_imm8,
                            (DoubleShiftKind::Right, true) => Code::Shrd_rm16_r16_CL,
                        };
                        assert_eq!(
                            Decoder::with_ip(32, &bytes, u64::from(CODE), DecoderOptions::NONE)
                                .decode()
                                .code(),
                            expected
                        );
                        assert_eq!(decoded.length() as usize, bytes.len());
                        assert_eq!(decoded.next_pc(), GuestAddress(CODE + bytes.len() as u32));
                        assert!(memory.is_code_current(decoded.code_snapshot()));
                        rows += 1;
                    }
                }
            }
        }
    }
    assert_eq!(rows, 32896);
}

fn instruction_error(pc: u32, error: DecodeError) -> CompileError {
    CompileError::Instruction {
        pc: GuestAddress(pc),
        cause: InstructionError::Decode(error),
    }
}

#[test]
fn word_double_shift_complete_prefix_and_memory_neighbors_remain_precisely_closed() {
    let opcode = DecodeError::Unsupported(UnsupportedFeature::Opcode);
    let segment = DecodeError::Unsupported(UnsupportedFeature::Segment);
    let mut rows = 0;
    for kind in [DoubleShiftKind::Left, DoubleShiftKind::Right] {
        for cl in [false, true] {
            let bytes = encoding(kind, cl, 1, 1, 0x66);
            let mut cases = Vec::new();
            for prefix in [0x66, 0x67, 0xf2, 0xf3] {
                cases.push(([vec![prefix], bytes.clone()].concat(), opcode));
            }
            cases.push(([vec![0x66, 0x67], bytes[1..].to_vec()].concat(), opcode));
            for prefix in [0x26, 0x2e, 0x36, 0x3e, 0x64, 0x65] {
                cases.push(([vec![prefix], bytes.clone()].concat(), segment));
            }
            cases.push((
                [vec![0xf0], bytes.clone()].concat(),
                DecodeError::InvalidEncoding,
            ));
            for tail in [
                &[0x03][..],
                &[0x04, 0x24],
                &[0x05, 0, 0x50, 0, 0],
                &[0x43, 0x80],
                &[0x83, 0x66, 0x67, 0xf0, 0xf3],
                &[0x04, 0x8d, 0, 0x50, 0, 0],
            ] {
                let mut memory_form = bytes[..3].to_vec();
                memory_form.extend_from_slice(tail);
                memory_form[3] |= 0x08;
                if !cl {
                    memory_form.push(0x66);
                }
                cases.push((memory_form, opcode));
            }
            for (mut bytes, error) in cases {
                bytes.extend([0xeb, 0]);
                let memory = code_space(CODE, &bytes);
                assert_eq!(
                    decode_one(&memory, GuestAddress(CODE)).err(),
                    Some(error),
                    "{bytes:02x?}"
                );
                assert_eq!(
                    compile_region(
                        &memory,
                        &[BlockSpec {
                            entry: GuestAddress(CODE),
                            byte_length: bytes.len() as u32
                        }],
                        CompileLimits::default()
                    )
                    .err(),
                    Some(instruction_error(CODE, error))
                );
                assert_eq!(
                    compile_entry_region(&memory, &[GuestAddress(CODE)], CompileLimits::default())
                        .err(),
                    Some(instruction_error(CODE, error))
                );
                rows += 1;
            }
        }
    }
    assert_eq!(rows, 72);
}

#[test]
fn word_double_shift_prefix_looking_immediate_bytes_remain_payload() {
    let mut rows = 0;
    for kind in [DoubleShiftKind::Left, DoubleShiftKind::Right] {
        for raw in [0x66, 0x67, 0xf0, 0xf3] {
            let bytes = encoding(kind, false, 1, 1, raw);
            let memory = code_space(CODE, &bytes);
            let decoded = decode_one(&memory, GuestAddress(CODE)).unwrap();
            assert_eq!(decoded.length(), 5);
            assert_eq!(
                decoded.operation(),
                &Operation::DoubleShiftWord {
                    kind,
                    destination: Register32::Ecx,
                    source: Register32::Ecx,
                    count: ShiftCount::Immediate(raw)
                }
            );
            rows += 1;
        }
    }
    assert_eq!(rows, 8);
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
fn word_double_shift_progressive_fetch_and_top_address_boundaries_remain_exact() {
    let mut rows = 0;
    for kind in [DoubleShiftKind::Left, DoubleShiftKind::Right] {
        for cl in [false, true] {
            let bytes = encoding(kind, cl, 1, 1, 0xf3);
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
                rows += 1;
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
                rows += 2;
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
            rows += 2;
        }
    }
    assert_eq!(rows, 48);
}

#[test]
fn word_double_shift_currency_tracks_consumed_bytes_without_fetching_next_page() {
    let mut rows = 0;
    for kind in [DoubleShiftKind::Left, DoubleShiftKind::Right] {
        for cl in [false, true] {
            let bytes = encoding(kind, cl, 1, 1, 0x66);
            for (offset, original) in bytes.iter().copied().enumerate() {
                for replacement in [original, original ^ 1] {
                    let mut memory = code_space(0x1fff, &bytes);
                    let decoded = decode_one(&memory, GuestAddress(0x1fff)).unwrap();
                    memory
                        .write(GuestAddress(0x1fff + offset as u32), &[replacement])
                        .unwrap();
                    assert!(!memory.is_code_current(decoded.code_snapshot()));
                    rows += 1;
                }
            }
            let mut memory = code_space(CODE, &bytes);
            let decoded = decode_one(&memory, GuestAddress(CODE)).unwrap();
            memory
                .write(GuestAddress(CODE + bytes.len() as u32), &[0x66])
                .unwrap();
            assert!(!memory.is_code_current(decoded.code_snapshot()));
            let pc = 0x2000 - bytes.len() as u32;
            let mut memory = code_space(pc, &bytes);
            let decoded = decode_one(&memory, GuestAddress(pc)).unwrap();
            memory
                .map_zeroed(
                    PageRange::new(GuestAddress(0x2000), 1).unwrap(),
                    Permissions::ALL,
                )
                .unwrap();
            memory
                .write(GuestAddress(0x2000), &[0x66, 0x67, 0xf0])
                .unwrap();
            memory
                .protect(
                    PageRange::new(GuestAddress(0x2000), 1).unwrap(),
                    Permissions::READ,
                )
                .unwrap();
            assert!(memory.is_code_current(decoded.code_snapshot()));
            rows += 2;
        }
    }
    assert_eq!(rows, 44);
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
fn representative_word_double_shifts_compile_in_six_pure_profiles_without_new_locals() {
    let mut bytes = Vec::new();
    let mut starts = Vec::new();
    for kind in [DoubleShiftKind::Left, DoubleShiftKind::Right] {
        for cl in [false, true] {
            for (i, destination) in [0, 1, 4, 7].into_iter().enumerate() {
                for source in [(destination + 3) % 8, destination] {
                    starts.push(CODE + bytes.len() as u32);
                    bytes.extend(encoding(kind, cl, destination, source, [0, 16, 17, 255][i]));
                }
            }
        }
    }
    bytes.extend([0xeb, 0]);
    let memory = code_space(CODE, &bytes);
    let mut profiles = 0;
    for entries in [false, true] {
        let compiled = if entries {
            compile_entry_region(&memory, &[GuestAddress(CODE)], CompileLimits::default())
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
            (compiled.metadata().blocks, compiled.metadata().instructions),
            (1, 33)
        );
        pure_module_interface(compiled.wasm_bytes(&memory).unwrap(), None);
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
                    .resolve(GuestAddress(DATA), Access::Read)
                    .is_err()
            );
            pure_module_interface(artifact(&engine, resident, id), Some(resident));
            if resident {
                engine.guard_resident(KEY, id).unwrap();
                for pc in &starts {
                    assert_eq!(engine.lookup_resident(*pc).unwrap().get(), id);
                }
            } else {
                engine.guard(KEY, id as u32).unwrap();
            }
            profiles += 1;
        }
    }
    assert_eq!(profiles, 6);
}

#[derive(Debug, PartialEq, Eq)]
struct Publication {
    arena: Vec<u8>,
    pointer: usize,
    generation: u32,
    modules: [(Vec<u8>, usize); 2],
    pages: [Vec<u8>; 3],
    mapped: u32,
}

fn publication(engine: &EngineInstance, id: u64) -> Publication {
    Publication {
        arena: engine.arena().to_vec(),
        pointer: engine.arena_address(),
        generation: engine.generation(),
        modules: [
            engine.artifact_bytes().unwrap(),
            engine.resident_bytes(id).unwrap(),
        ]
        .map(|bytes| (bytes.to_vec(), bytes.as_ptr() as usize)),
        pages: [CODE, KEEP, DATA].map(|pc| {
            let mut bytes = vec![0; 4096];
            engine
                .memory()
                .unwrap()
                .read(GuestAddress(pc), &mut bytes)
                .unwrap();
            bytes
        }),
        mapped: engine.memory().unwrap().mapped_pages(),
    }
}

fn previous_owners() -> (EngineInstance, u64) {
    let mut engine = EngineInstance::new(3, KEY).unwrap();
    for (pc, permissions) in [(CODE, 7), (KEEP, 7), (DATA, 3)] {
        engine.map(pc, 1, permissions).unwrap();
    }
    upload(&mut engine, DATA, &[0x5a; 4096]);
    upload(&mut engine, KEEP, &[0x90, 0xeb, 0]);
    describe(&mut engine, KEEP, 3, false);
    engine.compile(1).unwrap();
    let keep = engine.compile_resident(1).unwrap().get();
    engine
        .acknowledge_resident_installation(KEY, keep, 0)
        .unwrap();
    (engine, keep)
}

#[test]
fn late_word_double_shift_failures_and_caps_preserve_both_previous_owners() {
    let opcode = DecodeError::Unsupported(UnsupportedFeature::Opcode);
    let segment = DecodeError::Unsupported(UnsupportedFeature::Segment);
    let first = encoding(DoubleShiftKind::Left, false, 1, 1, 17);
    let mut cases = Vec::new();
    for (tail, error) in [
        (vec![0x0f, 0x0b], opcode),
        (vec![0x66, 0x66, 0x0f, 0xad, 0xc9], opcode),
        (vec![0x64, 0x0f, 0xa5, 0xc9], segment),
    ] {
        let mut bytes = first.clone();
        bytes.extend(tail);
        cases.push((
            CODE,
            bytes.clone(),
            bytes.len() as u32,
            instruction_error(CODE + 5, error),
        ));
    }
    let mut truncated = first.clone();
    truncated.extend([0x66, 0x0f, 0xa4, 0xc9]);
    cases.push((
        0x1ff7,
        truncated,
        10,
        instruction_error(
            0x1ffc,
            fetch_error(0x1ffc, 0x2000, FaultReason::Unmapped, 5),
        ),
    ));
    let mut capped = first.repeat(64);
    capped.extend([0x0f, 0x0b]);
    cases.push((
        CODE,
        capped.clone(),
        capped.len() as u32,
        CompileError::InstructionLimit,
    ));
    let mut failures = 0;
    for (pc, bytes, declared, cause) in cases {
        for resident in [false, true] {
            for entries in [false, true] {
                let (mut engine, keep) = previous_owners();
                upload(&mut engine, pc, &bytes);
                describe(&mut engine, pc, declared as usize, entries);
                let before = publication(&engine, keep);
                let expected = if resident {
                    HostError::Resident(RegistryError::Compile(cause))
                } else {
                    HostError::Compile(cause)
                };
                assert_eq!(bound(&mut engine, resident, entries), Err(expected));
                assert_eq!(publication(&engine, keep), before);
                engine.guard(KEY, before.generation).unwrap();
                engine.guard_resident(KEY, keep).unwrap();
                assert_eq!(engine.lookup_resident(KEEP).unwrap().get(), keep);
                let installed = engine.lookup_installed_resident(KEY, KEEP).unwrap();
                assert_eq!((installed.unit_id, installed.slot), (keep, 0));
                assert!(engine.lookup_resident(pc).is_err());
                failures += 1;
            }
        }
    }
    assert_eq!(failures, 20);
    let mut bytes = first;
    bytes.extend([0xeb, 0]);
    let memory = code_space(CODE, &bytes);
    for entries in [false, true] {
        for (limits, error) in [
            (
                CompileLimits {
                    instructions: 1,
                    ..CompileLimits::default()
                },
                CompileError::InstructionLimit,
            ),
            (
                CompileLimits {
                    wasm_bytes: 1,
                    ..CompileLimits::default()
                },
                CompileError::WasmLimit,
            ),
        ] {
            let result = if entries {
                compile_entry_region(&memory, &[GuestAddress(CODE)], limits)
            } else {
                compile_region(
                    &memory,
                    &[BlockSpec {
                        entry: GuestAddress(CODE),
                        byte_length: bytes.len() as u32,
                    }],
                    limits,
                )
            };
            assert_eq!(result.err(), Some(error));
        }
    }
}
