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
    memory::{Access, FaultReason, GuestAddress, MemoryFault},
    process::{EngineInstance, HostError},
};

const CODE: u32 = 0x1000;
const KEY: u64 = 0x5843_4847_3332_4d45;
const BASE: [u8; 2] = [0x87, 0x0b];
const SIB: [u8; 4] = [0x87, 0x44, 0x8b, 0xf0];
const BANK: [u8; 8] = [0x87, 0x0b, 0x87, 0x44, 0x8b, 0xf0, 0xeb, 0];
const DATA: u32 = 0x5000;
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
        .expect("unprefixed DWORD memory XCHG must decode");
    assert_eq!(
        decoded.operation(),
        &if bytes == BASE {
            Operation::MemoryExchange {
                address: address(Some(Register32::Ebx), None, 1, 0),
                source: Register32::Ecx,
            }
        } else {
            Operation::MemoryExchange {
                address: address(Some(Register32::Ebx), Some(Register32::Ecx), 4, 0xffff_fff0),
                source: Register32::Eax,
            }
        }
    );
    assert_eq!(decoded.length() as usize, bytes.len());
    assert_eq!(decoded.next_pc(), GuestAddress(CODE + bytes.len() as u32));
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
fn memory_xchg32_decodes_base_form() {
    decode_admission(&BASE);
}

#[test]
fn memory_xchg32_decodes_sib_form() {
    decode_admission(&SIB);
}

fn bound_admission(resident: bool, entries: bool) {
    let mut engine = code(&BANK);
    describe(&mut engine, CODE, BANK.len(), entries);
    let before = engine.arena().to_vec();
    let id = compile(&mut engine, resident, entries)
        .expect("unprefixed DWORD memory XCHG must compile bound without reading guest data");
    let module = if resident {
        engine.resident_bytes(id).unwrap()
    } else {
        engine.artifact_bytes().unwrap()
    };
    assert_eq!(&module[..8], b"\0asm\x01\0\0\0");
    assert_exchange_imports(module, resident);
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
fn memory_xchg32_compiles_replacement_explicit() {
    bound_admission(false, false);
}

#[test]
fn memory_xchg32_compiles_replacement_entry() {
    bound_admission(false, true);
}

#[test]
fn memory_xchg32_compiles_resident_explicit() {
    bound_admission(true, false);
}

#[test]
fn memory_xchg32_compiles_resident_entry() {
    bound_admission(true, true);
}

fn upload(engine: &mut EngineInstance, pc: u32, bytes: &[u8]) {
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(bytes);
    engine.upload(pc, bytes.len() as u32).unwrap();
}

fn code_at(pc: u32, bytes: &[u8], execute_only: bool) -> EngineInstance {
    let base = pc & !0xfff;
    let pages = (u64::from(pc - base) + bytes.len() as u64).div_ceil(4096) as u32;
    let mut engine = EngineInstance::new(pages + 1, KEY).unwrap();
    engine.map(base, pages, 7).unwrap();
    upload(&mut engine, pc, bytes);
    if execute_only {
        engine.protect(base, pages, 4).unwrap();
    }
    engine
}

fn describe(engine: &mut EngineInstance, pc: u32, length: usize, entries: bool) {
    let request = &mut engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8];
    request[..4].copy_from_slice(&pc.to_le_bytes());
    request[4..].copy_from_slice(&if entries { 0 } else { length as u32 }.to_le_bytes());
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

fn forms() -> Vec<(Vec<u8>, Register32, EffectiveAddress)> {
    use Register32::*;
    let addresses = [
        (vec![0x00], address(Some(Eax), None, 1, 0)),
        (vec![0x01], address(Some(Ecx), None, 1, 0)),
        (vec![0x02], address(Some(Edx), None, 1, 0)),
        (vec![0x03], address(Some(Ebx), None, 1, 0)),
        (vec![0x04, 0x24], address(Some(Esp), None, 1, 0)),
        (vec![0x45, 0x00], address(Some(Ebp), None, 1, 0)),
        (vec![0x06], address(Some(Esi), None, 1, 0)),
        (vec![0x07], address(Some(Edi), None, 1, 0)),
        (vec![0x40, 0x80], address(Some(Eax), None, 1, 0xffff_ff80)),
        (
            vec![0x82, 0x78, 0x56, 0x34, 0x12],
            address(Some(Edx), None, 1, 0x1234_5678),
        ),
        (
            vec![0x44, 0x88, 0xf0],
            address(Some(Eax), Some(Ecx), 4, 0xffff_fff0),
        ),
        (
            vec![0x04, 0xf5, 0x10, 0x50, 0, 0],
            address(None, Some(Esi), 8, DATA + 16),
        ),
        (
            vec![0x05, 0x10, 0x50, 0, 0],
            address(None, None, 1, DATA + 16),
        ),
    ];
    let mut forms = Vec::new();
    for (ordinal, source) in REGISTERS.into_iter().enumerate() {
        for (tail, address) in &addresses {
            let mut bytes = vec![0x87];
            bytes.extend_from_slice(tail);
            bytes[1] |= (ordinal as u8) << 3;
            forms.push((bytes, source, *address));
        }
    }
    forms
}

fn assert_operation(bytes: &[u8], source: Register32, address: EffectiveAddress) {
    let engine = code(bytes);
    let before = engine.arena().to_vec();
    let decoded = decode_one(engine.memory().unwrap(), GuestAddress(CODE)).unwrap();
    assert_eq!(
        decoded.operation(),
        &Operation::MemoryExchange { address, source }
    );
    assert_eq!(decoded.length() as usize, bytes.len());
    assert_eq!(decoded.next_pc(), GuestAddress(CODE + bytes.len() as u32));
    assert!(
        engine
            .memory()
            .unwrap()
            .is_code_current(decoded.code_snapshot())
    );
    assert_eq!(engine.arena(), before);
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

fn assert_exchange_imports(wasm: &[u8], resident: bool) {
    assert_eq!(&wasm[..8], b"\0asm\x01\0\0\0");
    let mut sections = &wasm[8..];
    while !sections.is_empty() {
        let kind = take(&mut sections, 1)[0];
        let length = unsigned(&mut sections) as usize;
        let mut section = take(&mut sections, length);
        if kind != 2 {
            continue;
        }
        assert_eq!(unsigned(&mut section), 3);
        assert_eq!((name(&mut section), name(&mut section)), ("env", "memory"));
        assert_eq!(take(&mut section, 1), &[2]);
        assert_eq!((unsigned(&mut section), unsigned(&mut section)), (0, 1));
        for (index, expected) in [
            if resident { "guard_resident" } else { "guard" },
            if resident {
                "exchange_resident32"
            } else {
                "exchange32"
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
    panic!("missing import section");
}

#[test]
fn memory_xchg32_all_sources_and_thirteen_address_rows_are_typed_and_bound() {
    let forms = forms();
    assert_eq!(forms.len(), 104);
    for (bytes, source, address) in &forms {
        assert_operation(bytes, *source, *address);
    }
    let mut admissions = 0;
    for bank in forms.chunks(32) {
        let mut bytes = Vec::new();
        for (form, _, _) in bank {
            bytes.extend_from_slice(form);
        }
        bytes.extend([0xeb, 0]);
        for resident in [false, true] {
            for entries in [false, true] {
                let mut engine = code(&bytes);
                describe(&mut engine, CODE, bytes.len(), entries);
                let before = engine.arena().to_vec();
                let id = compile(&mut engine, resident, entries).unwrap();
                let module = if resident {
                    engine.resident_bytes(id).unwrap()
                } else {
                    engine.artifact_bytes().unwrap()
                };
                assert_exchange_imports(module, resident);
                assert_eq!(engine.arena(), before);
                assert_eq!(engine.memory().unwrap().mapped_pages(), 1);
                admissions += 1;
            }
        }
    }
    assert_eq!(admissions, 16);
}

#[test]
fn memory_xchg32_index_aliases_raw_scale_and_prefix_looking_displacements_are_data() {
    let mut rows = 0;
    for (ordinal, source) in REGISTERS.into_iter().enumerate() {
        if source == Register32::Esp {
            continue;
        }
        assert_operation(
            &[
                0x87,
                0x04 | (ordinal as u8) << 3,
                0x83 | (ordinal as u8) << 3,
            ],
            source,
            address(Some(Register32::Ebx), Some(source), 4, 0),
        );
        rows += 1;
    }
    assert_operation(
        &[0x87, 0x84, 0xc0, 0xe0, 0xff, 0xff, 0xff],
        Register32::Eax,
        address(Some(Register32::Eax), Some(Register32::Eax), 8, 0xffff_ffe0),
    );
    rows += 1;
    for (sib, scale) in [(0x64, 2), (0xa4, 4), (0xe4, 8)] {
        assert_operation(
            &[0x87, 0x04, sib],
            Register32::Eax,
            address(Some(Register32::Esp), None, scale, 0),
        );
        rows += 1;
    }
    for displacement in [
        0,
        0x8000_0000,
        u32::MAX,
        0x6666_6666,
        0x6767_6767,
        0xf0f0_f0f0,
        0xf2f2_f2f2,
        0xf3f3_f3f3,
    ] {
        let mut bytes = vec![0x87, 0x05];
        bytes.extend(displacement.to_le_bytes());
        assert_operation(
            &bytes,
            Register32::Eax,
            address(None, None, 1, displacement),
        );
        rows += 1;
    }
    assert_eq!(rows, 19);
}

fn instruction_error(pc: u32, cause: InstructionError) -> CompileError {
    CompileError::Instruction {
        pc: GuestAddress(pc),
        cause,
    }
}

#[test]
fn memory_xchg32_exact_prefix_width_and_lock_boundaries_preserve_categories() {
    let opcode = DecodeError::Unsupported(UnsupportedFeature::Opcode);
    let mut cases = vec![
        (vec![0x66, 0x87, 0x03], opcode),
        (vec![0x66, 0x66, 0x87, 0x0b], opcode),
        (vec![0xf0, 0x87, 0xc8], DecodeError::InvalidEncoding),
    ];
    for prefix in [
        0x66, 0x67, 0xf0, 0xf2, 0xf3, 0x26, 0x2e, 0x36, 0x3e, 0x64, 0x65,
    ] {
        let expected = if matches!(prefix, 0x26 | 0x2e | 0x36 | 0x3e | 0x64 | 0x65) {
            DecodeError::Unsupported(UnsupportedFeature::Segment)
        } else {
            opcode
        };
        cases.push(([vec![prefix], BASE.to_vec()].concat(), expected));
    }
    for (bytes, expected) in &cases {
        let mut program = bytes.clone();
        program.extend([0xeb, 0]);
        let mut engine = code(&program);
        assert_eq!(
            decode_one(engine.memory().unwrap(), GuestAddress(CODE)).err(),
            Some(*expected),
            "{bytes:02x?}"
        );
        for resident in [false, true] {
            for entries in [false, true] {
                describe(&mut engine, CODE, program.len(), entries);
                let before = engine.arena().to_vec();
                let error = instruction_error(CODE, InstructionError::Decode(*expected));
                assert_eq!(
                    compile(&mut engine, resident, entries),
                    Err(if resident {
                        HostError::Resident(RegistryError::Compile(error))
                    } else {
                        HostError::Compile(error)
                    })
                );
                assert_eq!(engine.arena(), before);
                assert_eq!(engine.generation(), 0);
            }
        }
    }
    assert_eq!(cases.len(), 14);
    for bytes in [
        &[0x87, 0xc8][..],
        &[0x91],
        &[0x66, 0x87, 0xc8],
        &[0x86, 0xc8],
        &[0x0f, 0xc1, 0x0b],
        &[0x0f, 0xb1, 0x0b],
    ] {
        assert!(decode_one(code(bytes).memory().unwrap(), GuestAddress(CODE)).is_ok());
    }
}

#[test]
fn memory_xchg32_standalone_compile_and_prepare_keep_backend_refusal_priority() {
    for form in [BASE.as_slice(), SIB.as_slice()] {
        for trailing in [false, true] {
            let mut bytes = vec![0x90];
            bytes.extend_from_slice(form);
            bytes.extend(if trailing { [0x0f, 0x0b] } else { [0xeb, 0] });
            let engine = code(&bytes);
            let before = engine.arena().to_vec();
            let memory = engine.memory().unwrap();
            let blocks = [BlockSpec {
                entry: GuestAddress(CODE),
                byte_length: bytes.len() as u32,
            }];
            let expected = Some(instruction_error(
                CODE + 1,
                InstructionError::BackendUnsupported,
            ));
            let limits = CompileLimits::default();
            assert_eq!(prepare_region(memory, &blocks, limits).err(), expected);
            assert_eq!(compile_region(memory, &blocks, limits).err(), expected);
            assert_eq!(
                prepare_entry_region(memory, &[GuestAddress(CODE)], limits).err(),
                expected
            );
            assert_eq!(
                compile_entry_region(memory, &[GuestAddress(CODE)], limits).err(),
                expected
            );
            assert_eq!(engine.arena(), before);
        }
    }
}

fn fetch_error(pc: u32, address: u32, length: usize, reason: FaultReason) -> DecodeError {
    DecodeError::MemoryFault {
        pc: GuestAddress(pc),
        length: length as u32,
        fault: MemoryFault {
            address: GuestAddress(address),
            access: Access::Execute,
            reason,
        },
    }
}

#[test]
fn memory_xchg32_progressive_fetch_and_final_address_never_read_guest_operands() {
    let longest = [0x87, 0x84, 0xc0, 0xe0, 0xff, 0xff, 0xff];
    let mut cuts = 0;
    for bytes in [BASE.as_slice(), longest.as_slice()] {
        for pc in [
            0x2000 - bytes.len() as u32,
            0x1fff,
            u32::MAX - bytes.len() as u32 + 1,
        ] {
            let engine = code_at(pc, bytes, true);
            let decoded = decode_one(engine.memory().unwrap(), GuestAddress(pc)).unwrap();
            assert_eq!(decoded.length() as usize, bytes.len());
            assert_eq!(
                decoded.next_pc(),
                GuestAddress(pc.wrapping_add(bytes.len() as u32))
            );
        }
        for cut in 1..bytes.len() {
            for top in [false, true] {
                let pc = if top {
                    u32::MAX - cut as u32 + 1
                } else {
                    0x2000 - cut as u32
                };
                let mut engine = code_at(pc, &bytes[..cut], true);
                if top {
                    engine.map(0, 1, 4).unwrap();
                }
                assert_eq!(
                    decode_one(engine.memory().unwrap(), GuestAddress(pc)).err(),
                    Some(fetch_error(
                        pc,
                        if top { pc } else { 0x2000 },
                        cut + 1,
                        if top {
                            FaultReason::AddressOverflow
                        } else {
                            FaultReason::Unmapped
                        },
                    ))
                );
                cuts += 1;
            }
        }
    }
    assert_eq!(cuts, 14);
    let mut first = code_at(CODE, &longest, true);
    first.protect(CODE, 1, 3).unwrap();
    assert_eq!(
        decode_one(first.memory().unwrap(), GuestAddress(CODE)).err(),
        Some(fetch_error(CODE, CODE, 1, FaultReason::Permission))
    );
    let mut second = code_at(0x1fff, &longest, true);
    second.protect(0x2000, 1, 3).unwrap();
    assert_eq!(
        decode_one(second.memory().unwrap(), GuestAddress(0x1fff)).err(),
        Some(fetch_error(0x1fff, 0x2000, 2, FaultReason::Permission))
    );
}

#[test]
fn memory_xchg32_only_consumed_source_pages_control_currency() {
    let bytes = [0x87, 0x84, 0xc0, 0xe0, 0xff, 0xff, 0xff, 0xeb, 0];
    for changed in [0x1fff, 0x2000] {
        let pc = 0x1ffc;
        let mut engine = code_at(pc, &bytes, false);
        let decoded = decode_one(engine.memory().unwrap(), GuestAddress(pc)).unwrap();
        describe(&mut engine, pc, bytes.len(), false);
        let replacement = compile(&mut engine, false, false).unwrap() as u32;
        let resident = compile(&mut engine, true, false).unwrap();
        engine.map(DATA, 1, 3).unwrap();
        engine.write32(DATA, 0x1234_5678).unwrap();
        engine.protect(DATA, 1, 1).unwrap();
        engine.unmap(DATA, 1).unwrap();
        assert!(
            engine
                .memory()
                .unwrap()
                .is_code_current(decoded.code_snapshot())
        );
        engine.guard(KEY, replacement).unwrap();
        engine.guard_resident(KEY, resident).unwrap();
        upload(&mut engine, changed, &bytes[(changed - pc) as usize..][..1]);
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
fn memory_xchg32_mixed_sixty_four_instructions_and_existing_limits_are_exact() {
    let limits = CompileLimits::default();
    assert_eq!(
        (limits.blocks, limits.instructions, limits.wasm_bytes),
        (8, 64, 65_536)
    );
    let mut mixed = [0x87, 0x0b, 0x87, 0xc8, 0x0f, 0xc1, 0xc8].repeat(21);
    mixed.extend([0xeb, 0]);
    let mut too_many = BASE.repeat(64);
    too_many.extend([0xeb, 0]);
    for resident in [false, true] {
        for entries in [false, true] {
            let mut engine = code(&mixed);
            describe(&mut engine, CODE, mixed.len(), entries);
            let before = engine.arena().to_vec();
            compile(&mut engine, resident, entries).unwrap();
            assert_eq!(engine.arena(), before);
            let mut engine = code(&too_many);
            describe(&mut engine, CODE, too_many.len(), entries);
            let before = engine.arena().to_vec();
            assert_eq!(
                compile(&mut engine, resident, entries),
                Err(if resident {
                    HostError::Resident(RegistryError::Compile(CompileError::InstructionLimit))
                } else {
                    HostError::Compile(CompileError::InstructionLimit)
                })
            );
            assert_eq!(engine.arena(), before);
            assert_eq!(engine.generation(), 0);
        }
    }
}

#[test]
fn memory_xchg32_late_refusal_keeps_dirty_full_arena_and_both_existing_owners() {
    let mut bytes = BANK[..6].to_vec();
    bytes.extend([0x0f, 0x0b]);
    bytes.resize(256, 0x90);
    bytes.extend([0xeb, 0]);
    let mut engine = code(&bytes);
    describe(&mut engine, CODE + 256, 2, false);
    let replacement = compile(&mut engine, false, false).unwrap() as u32;
    let resident = compile(&mut engine, true, false).unwrap();
    let replacement_bytes = engine.artifact_bytes().unwrap().to_vec();
    let replacement_pointer = engine.artifact_bytes().unwrap().as_ptr();
    let resident_bytes = engine.resident_bytes(resident).unwrap().to_vec();
    let resident_pointer = engine.resident_bytes(resident).unwrap().as_ptr();
    let arena_pointer = engine.arena_address();
    let mut page = [0; 4096];
    engine
        .memory()
        .unwrap()
        .fetch(GuestAddress(CODE), &mut page)
        .unwrap();
    let error = instruction_error(
        CODE + 6,
        InstructionError::Decode(DecodeError::Unsupported(UnsupportedFeature::Opcode)),
    );
    for is_resident in [false, true] {
        for entries in [false, true] {
            for (index, byte) in engine.arena_mut().unwrap().iter_mut().enumerate() {
                *byte = (index.wrapping_mul(37).wrapping_add(11) & 0xff) as u8;
            }
            describe(&mut engine, CODE, 8, entries);
            let before = engine.arena().to_vec();
            assert_eq!(before.len(), 4364);
            assert_eq!(
                compile(&mut engine, is_resident, entries),
                Err(if is_resident {
                    HostError::Resident(RegistryError::Compile(error))
                } else {
                    HostError::Compile(error)
                })
            );
            assert_eq!(engine.arena(), before);
            assert_eq!(engine.arena_address(), arena_pointer);
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
            let mut after = [0; 4096];
            engine
                .memory()
                .unwrap()
                .fetch(GuestAddress(CODE), &mut after)
                .unwrap();
            assert_eq!(after, page);
            engine.guard(KEY, replacement).unwrap();
            engine.guard_resident(KEY, resident).unwrap();
        }
    }
}
