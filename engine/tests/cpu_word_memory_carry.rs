use ring3_engine::{
    abi::arena::TRANSFER_OFFSET,
    cpu::x86::decode::decode_one,
    memory::{Access, GuestAddress},
    process::EngineInstance,
};

const CODE: u32 = 0x1000;
const KEY: u64 = 0x574d_4341_5252_5931;

fn code(bytes: &[u8]) -> EngineInstance {
    let mut engine = EngineInstance::new(1, KEY).unwrap();
    engine.map(CODE, 1, 7).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(bytes);
    engine.upload(CODE, bytes.len() as u32).unwrap();
    engine.protect(CODE, 1, 4).unwrap();
    assert!(
        engine
            .memory()
            .unwrap()
            .resolve(GuestAddress(0), Access::Read)
            .is_err()
    );
    engine
}

fn decode_admission(bytes: &[u8]) {
    let engine = code(bytes);
    let before = engine.arena().to_vec();
    let decoded = decode_one(engine.memory().unwrap(), GuestAddress(CODE))
        .expect("WORD register-from-memory ADC/SBB must decode without reading data");
    assert_eq!(
        (decoded.length(), decoded.next_pc()),
        (bytes.len() as u8, GuestAddress(CODE + bytes.len() as u32))
    );
    assert_eq!(engine.arena(), before);
    assert_eq!(engine.memory().unwrap().mapped_pages(), 1);
}

#[test]
fn word_register_adc_memory_decodes_without_reading_data() {
    decode_admission(&[0x66, 0x13, 0x03]);
}

#[test]
fn word_register_sbb_memory_decodes_without_reading_data() {
    decode_admission(&[0x66, 0x1b, 0x03]);
}

fn bound_admission(resident: bool, entries: bool) {
    for opcode in [0x13, 0x1b] {
        let bytes = [0x66, opcode, 0x03, 0xeb, 0];
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
        .expect("WORD register-from-memory ADC/SBB must compile bound without reading data");
        let module = if resident {
            engine.resident_bytes(id).unwrap()
        } else {
            engine.artifact_bytes().unwrap()
        };
        assert!(!module.is_empty());
        assert_eq!(engine.arena(), before);
        assert_eq!(engine.memory().unwrap().mapped_pages(), 1);
    }
}

#[test]
fn word_memory_carry_compiles_replacement_explicit() {
    bound_admission(false, false);
}

#[test]
fn word_memory_carry_compiles_replacement_entry() {
    bound_admission(false, true);
}

#[test]
fn word_memory_carry_compiles_resident_explicit() {
    bound_admission(true, false);
}

#[test]
fn word_memory_carry_compiles_resident_entry() {
    bound_admission(true, true);
}

use ring3_engine::{
    abi::arena::TRANSFER_SIZE,
    cpu::{
        UnsupportedFeature,
        dbt::{
            BlockSpec, CompileError, CompileLimits, InstructionError, RegistryError,
            compile_entry_region, compile_region,
        },
        x86::{
            Register32,
            decode::DecodeError,
            ir::{
                EffectiveAddress, Operation, WordArithmeticKind, WordReadArithmeticKind, WordValue,
            },
        },
    },
    memory::{AddressSpace, FaultReason, MemoryFault, PageRange, Permissions},
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
const KINDS: [WordReadArithmeticKind; 2] =
    [WordReadArithmeticKind::Adc, WordReadArithmeticKind::Sbb];
fn encoding(kind: u8, destination: u8, tail: &[u8]) -> Vec<u8> {
    let mut bytes = vec![
        0x66,
        [0x13, 0x1b][kind as usize],
        tail[0] | destination << 3,
    ];
    bytes.extend_from_slice(&tail[1..]);
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

fn addresses() -> [(&'static [u8], EffectiveAddress); 17] {
    use Register32::{Eax, Ebp, Ebx, Ecx, Esi, Esp};
    let address = |base, index, scale, displacement| EffectiveAddress {
        base,
        index,
        scale,
        displacement,
    };
    [
        (&[0x00], address(Some(Eax), None, 1, 0)),
        (&[0x03], address(Some(Ebx), None, 1, 0)),
        (&[0x04, 0x24], address(Some(Esp), None, 1, 0)),
        (&[0x45, 0], address(Some(Ebp), None, 1, 0)),
        (&[0x43, 0], address(Some(Ebx), None, 1, 0)),
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
            &[0x05, 0x78, 0x56, 0x34, 0x92],
            address(None, None, 1, 0x9234_5678),
        ),
        (
            &[0x44, 0xc0, 0xff],
            address(Some(Eax), Some(Eax), 8, u32::MAX),
        ),
        (&[0x44, 0xec, 0x7f], address(Some(Esp), Some(Ebp), 8, 127)),
        (
            &[0x84, 0xf4, 0, 0, 0, 0x80],
            address(Some(Esp), Some(Esi), 8, 0x8000_0000),
        ),
    ]
}

#[test]
fn all_word_memory_carry_kinds_destinations_and_address_classes_have_exact_ir() {
    let mut memory = code_space(CODE, &[0x90]);
    let mut rows = 0;
    for (index, kind) in KINDS.into_iter().enumerate() {
        for (destination, parent) in PARENTS.into_iter().enumerate() {
            for (tail, address) in addresses() {
                let bytes = encoding(index as u8, destination as u8, tail);
                memory.write(GuestAddress(CODE), &bytes).unwrap();
                let decoded = decode_one(&memory, GuestAddress(CODE)).unwrap();
                assert_eq!(
                    decoded.operation(),
                    &Operation::ReadArithmeticWord {
                        kind,
                        destination: parent,
                        address
                    },
                    "{bytes:02x?}"
                );
                assert_eq!(
                    (decoded.length() as usize, decoded.next_pc()),
                    (bytes.len(), GuestAddress(CODE + bytes.len() as u32))
                );
                assert!(memory.is_code_current(decoded.code_snapshot()));
                rows += 1;
            }
        }
        let bytes = [0x66, [0x13, 0x1b][index], 0xc3];
        memory.write(GuestAddress(CODE), &bytes).unwrap();
        assert_eq!(
            decode_one(&memory, GuestAddress(CODE)).unwrap().operation(),
            &Operation::ArithmeticWord {
                kind: [WordArithmeticKind::Adc, WordArithmeticKind::Sbb][index],
                destination: Register32::Eax,
                source: WordValue::Register(Register32::Ebx)
            }
        );
        let bytes = [[0x13, 0x1b][index], 0x03];
        memory.write(GuestAddress(CODE), &bytes).unwrap();
        assert!(decode_one(&memory, GuestAddress(CODE)).is_ok());
    }
    assert_eq!(rows, 272);
}

#[test]
fn word_memory_carry_prefix_looking_displacements_are_payload() {
    for (index, kind) in KINDS.into_iter().enumerate() {
        let bytes = encoding(index as u8, 4, &[0x84, 0xf4, 0x66, 0x67, 0xf0, 0xf3]);
        let decoded = decode_one(&code_space(CODE, &bytes), GuestAddress(CODE)).unwrap();
        assert_eq!(
            decoded.operation(),
            &Operation::ReadArithmeticWord {
                kind,
                destination: Register32::Esp,
                address: EffectiveAddress {
                    base: Some(Register32::Esp),
                    index: Some(Register32::Esi),
                    scale: 8,
                    displacement: 0xf3f0_6766
                }
            }
        );
        assert_eq!(
            (decoded.length(), decoded.next_pc()),
            (8, GuestAddress(CODE + 8))
        );
    }
}

fn instruction_error(pc: u32, error: DecodeError) -> CompileError {
    CompileError::Instruction {
        pc: GuestAddress(pc),
        cause: InstructionError::Decode(error),
    }
}

#[test]
fn word_memory_carry_complete_prefixes_and_adjacent_families_stay_closed() {
    let opcode = DecodeError::Unsupported(UnsupportedFeature::Opcode);
    let segment = DecodeError::Unsupported(UnsupportedFeature::Segment);
    let mut rows = 0;
    for cc in 0..2 {
        let original = encoding(cc, cc % 8, &[0x03]);
        let mut cases = Vec::new();
        for prefix in [0x66, 0x67, 0xf2, 0xf3] {
            cases.push(([vec![prefix], original.clone()].concat(), opcode));
        }
        cases.push(([vec![0x66, 0x67], original[1..].to_vec()].concat(), opcode));
        for prefix in [0x26, 0x2e, 0x36, 0x3e, 0x64, 0x65] {
            cases.push(([vec![prefix], original.clone()].concat(), segment));
        }
        cases.push((
            [vec![0xf0], original].concat(),
            DecodeError::InvalidEncoding,
        ));
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
    assert_eq!(rows, 24);
    for opcode in [0x11, 0x19, 0x23, 0x0b, 0x33] {
        let mut bytes = vec![0x66, opcode, 0x03];
        if matches!(opcode, 0x23 | 0x0b | 0x33) {
            bytes.insert(0, 0x66);
        }
        assert_eq!(
            decode_one(&code_space(CODE, &bytes), GuestAddress(CODE)).err(),
            Some(DecodeError::Unsupported(UnsupportedFeature::Opcode))
        );
    }
    for extension in [2, 3] {
        for opcode in [0x81, 0x83] {
            let mut bytes = vec![0x66, opcode, 0x03 | extension << 3, 0];
            if opcode == 0x81 {
                bytes.push(0);
            }
            assert_eq!(
                decode_one(&code_space(CODE, &bytes), GuestAddress(CODE)).err(),
                Some(DecodeError::Unsupported(UnsupportedFeature::Opcode))
            );
        }
    }
    for (opcode, kind) in [
        (0x03, WordReadArithmeticKind::Add),
        (0x2b, WordReadArithmeticKind::Sub),
    ] {
        let bytes = [0x66, opcode, 0x03];
        assert_eq!(
            decode_one(&code_space(CODE, &bytes), GuestAddress(CODE))
                .unwrap()
                .operation(),
            &Operation::ReadArithmeticWord {
                kind,
                destination: Register32::Eax,
                address: EffectiveAddress {
                    base: Some(Register32::Ebx),
                    index: None,
                    scale: 1,
                    displacement: 0,
                },
            }
        );
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
fn word_memory_carry_progressive_fetch_and_top_end_remain_exact() {
    let mut rows = 0;
    for cc in [0, 1] {
        for tail in [
            &[0x03][..],
            &[0x44, 0x8b, 0x20],
            &[0x84, 0xf4, 0x66, 0x67, 0xf0, 0xf3],
        ] {
            let bytes = encoding(cc, cc % 8, tail);
            for pc in [
                0x2000 - bytes.len() as u32,
                0x1fff,
                u32::MAX - bytes.len() as u32 + 1,
            ] {
                let memory = code_space(pc, &bytes);
                let decoded = decode_one(&memory, GuestAddress(pc)).unwrap();
                assert_eq!(
                    (decoded.length() as usize, decoded.next_pc()),
                    (
                        bytes.len(),
                        GuestAddress(pc.wrapping_add(bytes.len() as u32))
                    )
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
    assert_eq!(rows, 82);
}

#[test]
fn word_memory_carry_currency_uses_only_consumed_code_pages() {
    let bytes = encoding(0, 4, &[0x84, 0xf4, 0x66, 0x67, 0xf0, 0xf3]);
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
    let decoded = decode_one(&memory, GuestAddress(CODE)).unwrap();
    memory
        .write(GuestAddress(CODE + bytes.len() as u32), &[0x66])
        .unwrap();
    assert!(!memory.is_code_current(decoded.code_snapshot()));
    let mut memory = code_space(0x2000 - bytes.len() as u32, &bytes);
    let decoded = decode_one(&memory, GuestAddress(0x2000 - bytes.len() as u32)).unwrap();
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

fn checked_word_read_interface(wasm: &[u8], resident: bool) {
    let mut types = section(wasm, 1);
    assert_eq!(types.unsigned(), 3);
    for arity in [4, if resident { 7 } else { 6 }, 1] {
        assert_eq!(types.byte(), 0x60);
        assert_eq!(types.unsigned(), arity);
        assert!(types.take(arity as usize).iter().all(|byte| *byte == 0x7f));
        assert_eq!((types.unsigned(), types.byte()), (1, 0x7f));
    }
    types.end();
    let mut imports = section(wasm, 2);
    assert_eq!(imports.unsigned(), 3);
    assert_eq!(
        (imports.name(), imports.name(), imports.byte()),
        ("env", "memory", 2)
    );
    assert_eq!((imports.unsigned(), imports.unsigned()), (0, 1));
    for (name, index) in [
        (if resident { "guard_resident" } else { "guard" }, 1),
        ("read16", 2),
    ] {
        assert_eq!(
            (
                imports.name(),
                imports.name(),
                imports.byte(),
                imports.unsigned()
            ),
            ("ring3", name, 0, index)
        );
    }
    imports.end();
    let mut functions = section(wasm, 3);
    assert_eq!((functions.unsigned(), functions.unsigned()), (1, 0));
    functions.end();
    let mut exports = section(wasm, 7);
    assert_eq!(
        (
            exports.unsigned(),
            exports.name(),
            exports.byte(),
            exports.unsigned()
        ),
        (1, "run", 0, 2)
    );
    exports.end();
    let mut bodies = section(wasm, 10);
    assert_eq!(bodies.unsigned(), 1);
    let length = bodies.unsigned() as usize;
    let mut body = Reader(bodies.take(length));
    bodies.end();
    assert_eq!(body.unsigned(), 3);
    for expected in [(16, 0x7f), (1, 0x7e), (6, 0x7f)] {
        assert_eq!((body.unsigned(), body.byte()), expected);
    }
}

#[test]
fn word_memory_carry_representative_bank_has_six_precise_profile_outcomes() {
    let mut bytes = Vec::new();
    let mut pcs = Vec::new();
    for (index, (tail, _)) in addresses().into_iter().enumerate() {
        pcs.push(CODE + bytes.len() as u32);
        bytes.extend(encoding((index % 2) as u8, (index % 8) as u8, tail));
    }
    bytes.extend([0xeb, 0]);
    let memory = code_space(CODE, &bytes);
    let expected = CompileError::Instruction {
        pc: GuestAddress(CODE),
        cause: InstructionError::BackendUnsupported,
    };
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
        Some(expected)
    );
    assert_eq!(
        compile_entry_region(&memory, &[GuestAddress(CODE)], CompileLimits::default()).err(),
        Some(expected)
    );
    let mut profiles = 2;
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
            checked_word_read_interface(artifact(&engine, resident, id), resident);
            if resident {
                engine.guard_resident(KEY, id).unwrap();
                for pc in &pcs {
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
fn late_word_memory_carry_failures_preserve_prior_publication_and_installed_keepers() {
    let opcode = DecodeError::Unsupported(UnsupportedFeature::Opcode);
    let segment = DecodeError::Unsupported(UnsupportedFeature::Segment);
    let mut cases = vec![
        (
            CODE,
            vec![0x66, 0x13, 0x03, 0x0f, 0x0b],
            5,
            instruction_error(CODE + 3, opcode),
        ),
        (
            CODE,
            vec![0x66, 0x1b, 0x03, 0x66, 0x66, 0x1b, 0x03],
            7,
            instruction_error(CODE + 3, opcode),
        ),
        (
            CODE,
            vec![0x66, 0x13, 0x03, 0x64, 0x13, 0x03],
            6,
            instruction_error(CODE + 3, segment),
        ),
        (
            0x1ffb,
            vec![0x66, 0x1b, 0x03, 0x66, 0x1b],
            6,
            instruction_error(
                0x1ffe,
                fetch_error(0x1ffe, 0x2000, FaultReason::Unmapped, 3),
            ),
        ),
    ];
    let mut capped = encoding(0, 0, &[0x03]).repeat(64);
    capped.extend([0x0f, 0x0b]);
    cases.push((
        CODE,
        capped.clone(),
        capped.len() as u32,
        CompileError::InstructionLimit,
    ));
    let mut failures = 0;
    for (pc, bytes, declared_length, cause) in cases {
        for resident in [false, true] {
            for entries in [false, true] {
                let (mut engine, keep) = previous_owners();
                upload(&mut engine, pc, &bytes);
                describe(&mut engine, pc, declared_length as usize, entries);
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
}

#[test]
fn word_memory_carry_exact_instruction_cap_compiles_all_bound_profiles() {
    let mut bytes = Vec::new();
    for index in 0..63 {
        bytes.extend(encoding(index % 2, index % 8, &[0x03]));
    }
    bytes.extend([0xeb, 0]);
    for resident in [false, true] {
        for entries in [false, true] {
            let mut engine = fixture(&bytes);
            describe(&mut engine, CODE, bytes.len(), entries);
            let before = engine.arena().to_vec();
            let id = bound(&mut engine, resident, entries).unwrap();
            assert_eq!(engine.arena(), before);
            checked_word_read_interface(artifact(&engine, resident, id), resident);
        }
    }
}
