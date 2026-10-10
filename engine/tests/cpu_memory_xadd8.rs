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
            ir::{ByteRegister, EffectiveAddress, Operation},
        },
    },
    memory::{Access, FaultReason, GuestAddress, MemoryFault},
    process::{EngineInstance, HostError},
};

#[test]
fn memory_xadd8_admits_before_jump_without_reading_unmapped_data() {
    let mut engine = EngineInstance::new(1, 0x1234_5678_9abc_def0).unwrap();
    let bytes = [0x0f, 0xc0, 0x03, 0xeb, 0];
    engine.map(0x1000, 1, 7).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(&bytes);
    engine.upload(0x1000, bytes.len() as u32).unwrap();
    let request = &mut engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8];
    request[..4].copy_from_slice(&0x1000_u32.to_le_bytes());
    request[4..].copy_from_slice(&(bytes.len() as u32).to_le_bytes());
    engine
        .compile(1)
        .expect("memory XADD8 must admit without reading guest data");
}

const CODE: u32 = 0x1000;
const DATA: u32 = 0x5000;
const KEY: u64 = 0x1234_5678_9abc_def0;
const SOURCES: [ByteRegister; 8] = [
    ByteRegister::Al,
    ByteRegister::Cl,
    ByteRegister::Dl,
    ByteRegister::Bl,
    ByteRegister::Ah,
    ByteRegister::Ch,
    ByteRegister::Dh,
    ByteRegister::Bh,
];
const PARENTS: [Register32; 4] = [
    Register32::Eax,
    Register32::Ecx,
    Register32::Edx,
    Register32::Ebx,
];

fn upload(engine: &mut EngineInstance, pc: u32, bytes: &[u8]) {
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(bytes);
    engine.upload(pc, bytes.len() as u32).unwrap();
}

fn code(pc: u32, bytes: &[u8]) -> EngineInstance {
    let base = pc & !0xfff;
    let pages = (u64::from(pc - base) + bytes.len() as u64).div_ceil(4096) as u32;
    let mut engine = EngineInstance::new(pages + 1, KEY).unwrap();
    engine.map(base, pages, 7).unwrap();
    upload(&mut engine, pc, bytes);
    engine
}

fn describe(engine: &mut EngineInstance, pc: u32, length: u32, entries: bool) {
    let request = &mut engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8];
    request[..4].copy_from_slice(&pc.to_le_bytes());
    request[4..].copy_from_slice(&if entries { 0 } else { length }.to_le_bytes());
}

fn compile(engine: &mut EngineInstance, resident: bool, entries: bool) -> Result<u64, HostError> {
    match (resident, entries) {
        (false, false) => engine.compile(1).map(u64::from),
        (false, true) => engine.compile_entries(1, 0).map(u64::from),
        (true, false) => engine.compile_resident(1).map(|id| id.get()),
        (true, true) => engine.compile_resident_entries(1, 0).map(|id| id.get()),
    }
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
    }
}

fn forms() -> Vec<(Vec<u8>, ByteRegister, EffectiveAddress)> {
    let mut forms = Vec::new();
    for (ordinal, source) in SOURCES.into_iter().enumerate() {
        let parent_ordinal = ordinal % 4;
        let parent = PARENTS[parent_ordinal];
        let field = (ordinal as u8) << 3;
        forms.push((
            vec![0x0f, 0xc0, field | parent_ordinal as u8],
            source,
            address(Some(parent), None, 1, 0),
        ));
        if ordinal >= 4 {
            forms.push((
                vec![0x0f, 0xc0, field | 4, 0x87 | (parent_ordinal as u8) << 3],
                source,
                address(Some(Register32::Edi), Some(parent), 4, 0),
            ));
            forms.push((
                vec![
                    0x0f,
                    0xc0,
                    field | 4,
                    0xc0 | (parent_ordinal as u8) << 3 | parent_ordinal as u8,
                ],
                source,
                address(Some(parent), Some(parent), 8, 0),
            ));
        }
    }
    for (bytes, source, expected) in [
        (
            vec![0x0f, 0xc0, 0x24, 0x24],
            ByteRegister::Ah,
            address(Some(Register32::Esp), None, 1, 0),
        ),
        (
            vec![0x0f, 0xc0, 0x72, 0x80],
            ByteRegister::Dh,
            address(Some(Register32::Edx), None, 1, 0xffff_ff80),
        ),
        (
            vec![0x0f, 0xc0, 0x89, 0x78, 0x56, 0x34, 0x12],
            ByteRegister::Cl,
            address(Some(Register32::Ecx), None, 1, 0x1234_5678),
        ),
        (
            vec![0x0f, 0xc0, 0x04, 0x85, 0x10, 0x50, 0, 0],
            ByteRegister::Al,
            address(None, Some(Register32::Eax), 4, DATA + 0x10),
        ),
        (
            vec![0x0f, 0xc0, 0x3d, 0x10, 0x50, 0, 0],
            ByteRegister::Bh,
            address(None, None, 1, DATA + 0x10),
        ),
    ] {
        forms.push((bytes, source, expected));
    }
    forms
}

fn unsigned(bytes: &mut &[u8]) -> u32 {
    let mut value = 0;
    for shift in (0..35).step_by(7) {
        let (&byte, remaining) = bytes.split_first().unwrap();
        *bytes = remaining;
        value |= u32::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return value;
        }
    }
    panic!("unterminated unsigned LEB");
}

fn take<'a>(bytes: &mut &'a [u8], length: usize) -> &'a [u8] {
    let (value, remaining) = bytes.split_at(length);
    *bytes = remaining;
    value
}

fn name<'a>(bytes: &mut &'a [u8]) -> &'a str {
    let length = unsigned(bytes) as usize;
    std::str::from_utf8(take(bytes, length)).unwrap()
}

fn assert_imports(wasm: &[u8], resident: bool) {
    assert_eq!(&wasm[..8], b"\0asm\x01\0\0\0");
    let mut sections = &wasm[8..];
    loop {
        let kind = take(&mut sections, 1)[0];
        let length = unsigned(&mut sections) as usize;
        let mut section = take(&mut sections, length);
        if kind != 2 {
            continue;
        }
        assert_eq!(unsigned(&mut section), 4);
        assert_eq!((name(&mut section), name(&mut section)), ("env", "memory"));
        assert_eq!(take(&mut section, 1), &[2]);
        assert_eq!((unsigned(&mut section), unsigned(&mut section)), (0, 1));
        for (index, expected) in [
            if resident { "guard_resident" } else { "guard" },
            "read8",
            if resident {
                "store_resident8"
            } else {
                "store8"
            },
        ]
        .into_iter()
        .enumerate()
        {
            assert_eq!(
                (name(&mut section), name(&mut section)),
                ("ring3", expected)
            );
            assert_eq!(take(&mut section, 1), &[0]);
            assert_eq!(unsigned(&mut section), index as u32 + 1);
        }
        assert!(section.is_empty());
        return;
    }
}

#[test]
fn low_high_sources_and_parent_addresses_decode_exactly_and_admit_only_bound() {
    let mut bytes = Vec::new();
    let mut expected = Vec::new();
    for (form, source, address) in forms() {
        expected.push((CODE + bytes.len() as u32, form.len(), source, address));
        bytes.extend(form);
    }
    assert_eq!(expected.len(), 21);
    bytes.extend([0xeb, 0]);
    let engine = code(CODE, &bytes);
    let memory = engine.memory().unwrap();
    for &(pc, length, source, address) in &expected {
        let decoded = decode_one(memory, GuestAddress(pc)).unwrap();
        assert_eq!(
            decoded.operation(),
            &Operation::MemoryExchangeAddByte { address, source }
        );
        assert_eq!(
            (decoded.pc(), decoded.length() as usize, decoded.next_pc()),
            (GuestAddress(pc), length, GuestAddress(pc + length as u32))
        );
        assert!(memory.is_code_current(decoded.code_snapshot()));
    }
    let unsupported = CompileError::Instruction {
        pc: GuestAddress(CODE),
        cause: InstructionError::BackendUnsupported,
    };
    assert_eq!(
        compile_region(
            memory,
            &[BlockSpec {
                entry: GuestAddress(CODE),
                byte_length: bytes.len() as u32,
            }],
            CompileLimits::default()
        )
        .err(),
        Some(unsupported)
    );
    assert_eq!(
        compile_entry_region(memory, &[GuestAddress(CODE)], CompileLimits::default()).err(),
        Some(unsupported)
    );
    for resident in [false, true] {
        for entries in [false, true] {
            let mut engine = code(CODE, &bytes);
            engine.protect(CODE, 1, 4).unwrap();
            describe(&mut engine, CODE, bytes.len() as u32, entries);
            let before = engine.arena().to_vec();
            let id = compile(&mut engine, resident, entries).unwrap();
            if resident {
                engine.guard_resident(KEY, id).unwrap();
                assert_imports(engine.resident_bytes(id).unwrap(), true);
                for &(pc, length, _, _) in &expected {
                    assert_eq!(engine.lookup_resident(pc).unwrap().get(), id);
                    for interior in 1..length as u32 {
                        assert!(engine.lookup_resident(pc + interior).is_err());
                    }
                }
            } else {
                engine.guard(KEY, id as u32).unwrap();
                assert_imports(engine.artifact_bytes().unwrap(), false);
            }
            assert_eq!(engine.arena(), before);
            assert_eq!(engine.memory().unwrap().mapped_pages(), 1);
            for data in [0, DATA] {
                assert!(
                    engine
                        .memory()
                        .unwrap()
                        .resolve(GuestAddress(data), Access::Read)
                        .is_err()
                );
            }
        }
    }
}

fn fetch_error(pc: u32, address: u32, length: u32, reason: FaultReason) -> DecodeError {
    DecodeError::MemoryFault {
        pc: GuestAddress(pc),
        length,
        fault: MemoryFault {
            address: GuestAddress(address),
            access: Access::Execute,
            reason,
        },
    }
}

#[test]
fn consumed_byte_instruction_keeps_exact_fetch_and_page_currency() {
    let bytes = [0x0f, 0xc0, 0xa4, 0xc0, 0xe0, 0xff, 0xff, 0xff];
    for pc in [
        0x2000 - bytes.len() as u32,
        u32::MAX - (bytes.len() as u32 - 1),
    ] {
        let engine = code(pc, &bytes);
        let decoded = decode_one(engine.memory().unwrap(), GuestAddress(pc)).unwrap();
        assert_eq!(
            (decoded.length() as usize, decoded.next_pc()),
            (
                bytes.len(),
                GuestAddress(pc.wrapping_add(bytes.len() as u32))
            )
        );
        assert!(
            engine
                .memory()
                .unwrap()
                .is_code_current(decoded.code_snapshot())
        );
        assert!(
            engine
                .memory()
                .unwrap()
                .resolve(decoded.next_pc(), Access::Execute)
                .is_err()
        );
    }
    for top in [false, true] {
        let missing = &bytes[..bytes.len() - 1];
        let pc = if top {
            u32::MAX - (missing.len() as u32 - 1)
        } else {
            0x2000 - missing.len() as u32
        };
        let engine = code(pc, missing);
        assert_eq!(
            decode_one(engine.memory().unwrap(), GuestAddress(pc)).err(),
            Some(fetch_error(
                pc,
                if top { pc } else { 0x2000 },
                bytes.len() as u32,
                if top {
                    FaultReason::AddressOverflow
                } else {
                    FaultReason::Unmapped
                },
            ))
        );
    }
    for changed in [0x1fff, 0x2000] {
        let mut engine = code(0x1ffc, &bytes);
        let decoded = decode_one(engine.memory().unwrap(), GuestAddress(0x1ffc)).unwrap();
        describe(&mut engine, 0x1ffc, bytes.len() as u32, false);
        let generation = engine.compile(1).unwrap();
        let id = engine.compile_resident(1).unwrap().get();
        engine.map(DATA, 1, 3).unwrap();
        engine.write8(DATA, 0x80).unwrap();
        engine.protect(DATA, 1, 1).unwrap();
        engine.unmap(DATA, 1).unwrap();
        assert!(
            engine
                .memory()
                .unwrap()
                .is_code_current(decoded.code_snapshot())
        );
        engine.guard(KEY, generation).unwrap();
        engine.guard_resident(KEY, id).unwrap();
        upload(
            &mut engine,
            changed,
            &bytes[(changed - 0x1ffc) as usize..][..1],
        );
        assert!(
            !engine
                .memory()
                .unwrap()
                .is_code_current(decoded.code_snapshot())
        );
        assert_eq!(
            engine.guard(KEY, generation),
            Err(HostError::CodeInvalidated)
        );
        assert_eq!(
            engine.guard_resident(KEY, id),
            Err(HostError::Resident(RegistryError::CodeInvalidated))
        );
    }
    let mut engine = code(0x1fff, &bytes);
    engine.protect(0x2000, 1, 3).unwrap();
    assert_eq!(
        decode_one(engine.memory().unwrap(), GuestAddress(0x1fff)).err(),
        Some(fetch_error(0x1fff, 0x2000, 2, FaultReason::Permission))
    );
}

#[test]
fn word_prefix_and_other_memory_exchange_refusals_keep_inputs_unchanged() {
    let opcode = DecodeError::Unsupported(UnsupportedFeature::Opcode);
    let mut cases = vec![
        (vec![0x66, 0x66, 0x0f, 0xc1, 0x03], opcode),
        (vec![0x86, 0x03], opcode),
        (vec![0x66, 0x0f, 0xb0, 0x03], opcode),
    ];
    for prefix in [
        0x66, 0x67, 0xf2, 0xf3, 0x26, 0x2e, 0x36, 0x3e, 0x64, 0x65, 0xf0,
    ] {
        let expected = if matches!(prefix, 0x26 | 0x2e | 0x36 | 0x3e | 0x64 | 0x65) {
            DecodeError::Unsupported(UnsupportedFeature::Segment)
        } else {
            opcode
        };
        cases.push((vec![prefix, 0x0f, 0xc0, 0x03], expected));
    }
    for (bytes, expected) in cases {
        let mut engine = code(CODE, &bytes);
        assert_eq!(
            decode_one(engine.memory().unwrap(), GuestAddress(CODE)).err(),
            Some(expected),
            "{bytes:02x?}"
        );
        let error = CompileError::Instruction {
            pc: GuestAddress(CODE),
            cause: InstructionError::Decode(expected),
        };
        for resident in [false, true] {
            for entries in [false, true] {
                describe(&mut engine, CODE, bytes.len() as u32, entries);
                let before = engine.arena().to_vec();
                let expected = if resident {
                    HostError::Resident(RegistryError::Compile(error))
                } else {
                    HostError::Compile(error)
                };
                assert_eq!(
                    compile(&mut engine, resident, entries),
                    Err(expected),
                    "{bytes:02x?}"
                );
                assert_eq!(engine.arena(), before);
                assert_eq!(engine.generation(), 0);
            }
        }
    }
}

#[test]
fn memory_xadd8_keeps_existing_default_and_instruction_limits() {
    let limits = CompileLimits::default();
    assert_eq!(
        (limits.blocks, limits.instructions, limits.wasm_bytes),
        (8, 64, 65_536)
    );
    let mut bytes = [0x0f, 0xc0, 0x03].repeat(64);
    bytes.extend([0xeb, 0]);
    for resident in [false, true] {
        for entries in [false, true] {
            let mut engine = code(CODE, &bytes);
            describe(&mut engine, CODE, bytes.len() as u32, entries);
            let before = engine.arena().to_vec();
            let expected = if resident {
                HostError::Resident(RegistryError::Compile(CompileError::InstructionLimit))
            } else {
                HostError::Compile(CompileError::InstructionLimit)
            };
            assert_eq!(compile(&mut engine, resident, entries), Err(expected));
            assert_eq!(engine.arena(), before);
            assert_eq!(engine.generation(), 0);
        }
    }
}
