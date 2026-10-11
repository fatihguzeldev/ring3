#[path = "support/prefix_admission.rs"]
mod prefix_admission;

use ring3_engine::{
    abi::arena::{TRANSFER_OFFSET, X87_OFFSET},
    cpu::x86::decode::decode_one,
    memory::{Access, GuestAddress},
    process::EngineInstance,
};

use ring3_engine::{
    cpu::{
        UnsupportedFeature,
        dbt::{
            BlockSpec, CompileError, CompileLimits, InstructionError, RegistryError,
            compile_entry_region, compile_region, prepare_entry_region, prepare_region,
        },
        x86::{
            Register32,
            decode::DecodeError,
            ir::{BitIndex, BitTestKind, EffectiveAddress, Operation},
        },
    },
    memory::{FaultReason, MemoryFault},
    process::HostError,
};

const CODE: u32 = 0x1000;
const DATA: u32 = 0x5000;
const KEY: u64 = 0x4c4f_434b_4254_5332;
const BASE: [u8; 4] = [0xf0, 0x0f, 0xab, 0x11];
const SIB: [u8; 6] = [0xf0, 0x0f, 0xab, 0x44, 0x87, 0xf0];
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

fn code(bytes: &[u8]) -> EngineInstance {
    code_at(CODE, bytes, true)
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
        ..Default::default()
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
    let mut result = Vec::new();
    for (tail, address) in &addresses {
        let mut bytes = vec![0xf0, 0x0f, 0xab];
        bytes.extend_from_slice(tail);
        result.push((bytes, Register32::Eax, *address));
    }
    for (ordinal, source) in REGISTERS.into_iter().enumerate().skip(1) {
        let base = ordinal as u8 & 3;
        result.push((
            vec![0xf0, 0x0f, 0xab, ((ordinal as u8) << 3) | base],
            source,
            address(Some(REGISTERS[usize::from(base)]), None, 1, 0),
        ));
    }
    result
}

fn assert_operation(bytes: &[u8], source: Register32, address: EffectiveAddress) {
    let engine = code(bytes);
    let before = engine.arena().to_vec();
    let decoded = decode_one(engine.memory().unwrap(), GuestAddress(CODE)).unwrap();
    assert_eq!(
        decoded.operation(),
        &Operation::LockedMemoryBitSet32 {
            address,
            index: source
        }
    );
    assert_eq!(usize::from(decoded.length()), bytes.len());
    assert_eq!(decoded.next_pc(), GuestAddress(CODE + bytes.len() as u32));
    assert!(
        engine
            .memory()
            .unwrap()
            .is_code_current(decoded.code_snapshot())
    );
    assert_eq!(engine.arena(), before);
}

#[test]
fn locked_bts32_all_eight_sources_and_thirteen_ea_rows_are_typed_and_bound() {
    let forms = forms();
    assert_eq!(forms.len(), 20);
    for (bytes, source, address) in &forms {
        assert_operation(bytes, *source, *address);
    }
    let mut bytes = Vec::new();
    for (form, _, _) in &forms {
        bytes.extend_from_slice(form);
    }
    bytes.extend([0xeb, 0]);
    for resident in [false, true] {
        for entries in [false, true] {
            let mut engine = code(&bytes);
            describe(&mut engine, CODE, bytes.len(), entries);
            let before = engine.arena().to_vec();
            compile(&mut engine, resident, entries).unwrap();
            assert_eq!(engine.arena(), before);
            assert_eq!(engine.memory().unwrap().mapped_pages(), 1);
        }
    }
}

#[test]
fn locked_bts32_prefix_bytes_inside_sib_and_displacement_are_payload() {
    for sib in [0x66, 0x67, 0xf0, 0xf2, 0xf3] {
        assert_operation(
            &[0xf0, 0x0f, 0xab, 0x84, sib, 0x66, 0x67, 0xf0, 0xf3],
            Register32::Eax,
            address(
                Some(REGISTERS[usize::from(sib & 7)]),
                if (sib >> 3) & 7 == 4 {
                    None
                } else {
                    Some(REGISTERS[usize::from((sib >> 3) & 7)])
                },
                1 << (sib >> 6),
                0xf3f0_6766,
            ),
        );
    }
    for bytes in [
        &[0xf0, 0x0f, 0xab, 0x45, 0xf0][..],
        &[0xf0, 0x0f, 0xab, 0x05, 0x66, 0x67, 0xf0, 0xf3][..],
    ] {
        let decoded = decode_one(code(bytes).memory().unwrap(), GuestAddress(CODE)).unwrap();
        assert_eq!(usize::from(decoded.length()), bytes.len());
    }
}

fn unsigned(bytes: &mut &[u8]) -> u32 {
    let mut value = 0;
    for shift in (0..35).step_by(7) {
        let (&byte, rest) = bytes.split_first().unwrap();
        *bytes = rest;
        value |= u32::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return value;
        }
    }
    panic!("unterminated unsigned LEB");
}

fn take<'a>(bytes: &mut &'a [u8], length: usize) -> &'a [u8] {
    let (value, rest) = bytes.split_at(length);
    *bytes = rest;
    value
}

fn name<'a>(bytes: &mut &'a [u8]) -> &'a str {
    let length = unsigned(bytes) as usize;
    std::str::from_utf8(take(bytes, length)).unwrap()
}

fn assert_imports(wasm: &[u8], expected: &[(&str, usize)], type_parameters: &[usize]) {
    assert_eq!(&wasm[..8], b"\0asm\x01\0\0\0");
    let mut sections = &wasm[8..];
    let mut types = Vec::new();
    let mut imported = Vec::new();
    let mut exported = false;
    while !sections.is_empty() {
        let kind = take(&mut sections, 1)[0];
        let length = unsigned(&mut sections) as usize;
        let mut section = take(&mut sections, length);
        match kind {
            1 => {
                let count = unsigned(&mut section);
                for _ in 0..count {
                    assert_eq!(take(&mut section, 1), &[0x60]);
                    let parameters = unsigned(&mut section) as usize;
                    assert!(
                        take(&mut section, parameters)
                            .iter()
                            .all(|&value| value == 0x7f)
                    );
                    assert_eq!(
                        (unsigned(&mut section), take(&mut section, 1)),
                        (1, &[0x7f][..])
                    );
                    types.push(parameters);
                }
            }
            2 => {
                assert_eq!(unsigned(&mut section) as usize, expected.len() + 1);
                assert_eq!((name(&mut section), name(&mut section)), ("env", "memory"));
                assert_eq!(take(&mut section, 1), &[2]);
                assert_eq!((unsigned(&mut section), unsigned(&mut section)), (0, 1));
                for &(expected_name, parameters) in expected {
                    assert_eq!(
                        (name(&mut section), name(&mut section)),
                        ("ring3", expected_name)
                    );
                    assert_eq!(take(&mut section, 1), &[0]);
                    let index = unsigned(&mut section) as usize;
                    assert_eq!(types[index], parameters);
                    imported.push(expected_name);
                }
            }
            7 => {
                assert_eq!(unsigned(&mut section), 1);
                assert_eq!(name(&mut section), "run");
                assert_eq!(take(&mut section, 1), &[0]);
                assert_eq!(unsigned(&mut section) as usize, expected.len());
                exported = true;
            }
            _ => continue,
        }
        assert!(section.is_empty());
    }
    assert_eq!(types, type_parameters);
    assert_eq!(imported.len(), expected.len());
    assert!(exported);
}

#[test]
fn locked_bts32_only_its_two_or_six_argument_helper_is_imported() {
    let bank = [0xf0, 0x0f, 0xab, 0x37, 0x43, 0xeb, 0];
    for resident in [false, true] {
        for entries in [false, true] {
            let mut engine = code(&bank);
            describe(&mut engine, CODE, bank.len(), entries);
            let id = compile(&mut engine, resident, entries).unwrap();
            let module = if resident {
                engine.resident_bytes(id).unwrap()
            } else {
                engine.artifact_bytes().unwrap()
            };
            let guard = if resident { 7 } else { 6 };
            let store = if resident { 6 } else { 2 };
            assert_imports(
                module,
                &[
                    (if resident { "guard_resident" } else { "guard" }, guard),
                    (
                        if resident {
                            "bit_test_set_resident32"
                        } else {
                            "bit_test_set32"
                        },
                        store,
                    ),
                ],
                &[4, guard, store],
            );
        }
    }
}

#[test]
fn locked_bts32_mixed_imports_keep_old_order_and_share_store_type() {
    let bank = [
        0x0f, 0xab, 0x0b, // ordinary BTS: read32 + store32
        0xf0, 0x0f, 0xb1, 0x0b, // CAS32: separate3/7 type
        0x66, 0xf0, 0x0f, 0xc1, 0x0b, // existing ADD16: shared2/6 type
        0xf0, 0x0f, 0xab, 0x0b, 0xeb, 0, // new BTS import appended last
    ];
    for resident in [false, true] {
        for entries in [false, true] {
            let mut engine = code(&bank);
            describe(&mut engine, CODE, bank.len(), entries);
            let id = compile(&mut engine, resident, entries).unwrap();
            let module = if resident {
                engine.resident_bytes(id).unwrap()
            } else {
                engine.artifact_bytes().unwrap()
            };
            let guard = if resident { 7 } else { 6 };
            let store = if resident { 6 } else { 2 };
            let cas = if resident { 7 } else { 3 };
            assert_imports(
                module,
                &[
                    (if resident { "guard_resident" } else { "guard" }, guard),
                    ("read32", 1),
                    (
                        if resident {
                            "store_resident32"
                        } else {
                            "store32"
                        },
                        store,
                    ),
                    (
                        if resident {
                            "compare_exchange_resident32"
                        } else {
                            "compare_exchange32"
                        },
                        cas,
                    ),
                    (
                        if resident {
                            "exchange_add_resident16"
                        } else {
                            "exchange_add16"
                        },
                        store,
                    ),
                    (
                        if resident {
                            "bit_test_set_resident32"
                        } else {
                            "bit_test_set32"
                        },
                        store,
                    ),
                ],
                &[4, guard, 1, store, cas],
            );
        }
    }
}

fn instruction_error(pc: u32, cause: InstructionError) -> CompileError {
    CompileError::Instruction {
        pc: GuestAddress(pc),
        cause,
    }
}

#[test]
fn locked_bts32_exact_prefix_scope_and_unselected_bit_forms_stay_closed() {
    let opcode = DecodeError::Unsupported(UnsupportedFeature::Opcode);
    let segment = DecodeError::Unsupported(UnsupportedFeature::Segment);
    let mut cases = std::collections::BTreeMap::new();
    for form in [BASE.as_slice(), SIB.as_slice()] {
        for prefix in [
            0xf0, 0x66, 0x67, 0xf2, 0xf3, 0x26, 0x2e, 0x36, 0x3e, 0x64, 0x65,
        ] {
            let error = if matches!(prefix, 0x26 | 0x2e | 0x36 | 0x3e | 0x64 | 0x65) {
                segment
            } else {
                opcode
            };
            cases.insert([vec![prefix], form.to_vec()].concat(), error);
            cases.insert([vec![0xf0, prefix], form[1..].to_vec()].concat(), error);
        }
    }
    for source in 0..8 {
        cases.insert(
            vec![0xf0, 0x0f, 0xab, 0xc0 | source << 3],
            DecodeError::InvalidEncoding,
        );
    }
    for bytes in [
        vec![0xf0, 0x0f, 0xba, 0x2b, 5],    // immediate BTS stays closed
        vec![0xf0, 0xf0, 0x0f, 0xb3, 0x0b], // duplicate LOCK BTR
        vec![0xf0, 0x0f, 0xbb, 0x0b],       // BTC
        vec![0xf0, 0xf0, 0x09, 0x0b],       // OR
        vec![0xf0, 0x66, 0x0f, 0xab, 0x0b],
    ] {
        cases.insert(bytes, opcode);
    }
    for (bytes, expected) in cases {
        if prefix_admission::admit_if_new_prefix(&bytes) {
            continue;
        }
        let engine = code(&bytes);
        let before = engine.arena().to_vec();
        assert_eq!(
            decode_one(engine.memory().unwrap(), GuestAddress(CODE)).err(),
            Some(expected),
            "{bytes:02x?}"
        );
        assert_eq!(engine.arena(), before);
        let mut bank = bytes.clone();
        bank.extend([0xeb, 0]);
        for resident in [false, true] {
            let mut engine = code(&bank);
            describe(&mut engine, CODE, bank.len(), false);
            let before = engine.arena().to_vec();
            let cause = instruction_error(CODE, InstructionError::Decode(expected));
            assert_eq!(
                compile(&mut engine, resident, false),
                Err(if resident {
                    HostError::Resident(RegistryError::Compile(cause))
                } else {
                    HostError::Compile(cause)
                }),
                "{bytes:02x?}"
            );
            assert_eq!(engine.arena(), before);
            assert_eq!(engine.generation(), 0);
        }
    }
}

#[test]
fn locked_bts32_ordinary_bit_and_existing_locked_operations_keep_their_ir() {
    let ea = address(Some(Register32::Ebx), None, 1, 0);
    for (bytes, expected) in [
        (
            &[0x0f, 0xab, 0x0b][..],
            Operation::MemoryBitMutation {
                kind: BitTestKind::Set,
                address: ea,
                index: BitIndex::Register(Register32::Ecx),
            },
        ),
        (
            &[0x0f, 0xba, 0x2b, 255][..],
            Operation::MemoryBitMutation {
                kind: BitTestKind::Set,
                address: ea,
                index: BitIndex::Immediate(255),
            },
        ),
        (
            &[0x0f, 0xab, 0xc8][..],
            Operation::BitTest {
                kind: BitTestKind::Set,
                destination: Register32::Eax,
                index: BitIndex::Register(Register32::Ecx),
            },
        ),
        (
            &[0x66, 0x0f, 0xab, 0x0b][..],
            Operation::MemoryBitMutationWord {
                kind: BitTestKind::Set,
                address: ea,
                index: BitIndex::Register(Register32::Ecx),
            },
        ),
        (
            &[0xf0, 0x0f, 0xc1, 0x0b][..],
            Operation::LockedMemoryExchangeAdd32 {
                address: ea,
                source: Register32::Ecx,
            },
        ),
        (
            &[0xf0, 0x0f, 0xb1, 0x0b][..],
            Operation::LockedMemoryCompareExchange32 {
                address: ea,
                source: Register32::Ecx,
            },
        ),
    ] {
        assert_eq!(
            decode_one(code(bytes).memory().unwrap(), GuestAddress(CODE))
                .unwrap()
                .operation(),
            &expected
        );
    }
}

#[test]
fn locked_bts32_index_aliases_preserve_full_base_and_scaled_index_types() {
    for (ordinal, source) in REGISTERS.into_iter().enumerate() {
        let tail = if source == Register32::Esp {
            vec![0x04, 0x24]
        } else if source == Register32::Ebp {
            vec![0x45, 0]
        } else {
            vec![ordinal as u8]
        };
        let mut bytes = vec![0xf0, 0x0f, 0xab];
        bytes.extend(tail);
        bytes[3] |= (ordinal as u8) << 3;
        assert_operation(&bytes, source, address(Some(source), None, 1, 0));
        assert_operation(
            &[0xf0, 0x0f, 0xab, ((ordinal as u8) << 3) | 4, 0x24],
            source,
            address(Some(Register32::Esp), None, 1, 0),
        );
        if source != Register32::Esp {
            assert_operation(
                &[
                    0xf0,
                    0x0f,
                    0xab,
                    ((ordinal as u8) << 3) | 0x44,
                    (ordinal as u8) << 3,
                    0x80,
                ],
                source,
                address(Some(Register32::Eax), Some(source), 1, 0xffff_ff80),
            );
        }
    }
}

#[test]
fn locked_bts32_negative_and_extreme_index_inputs_are_not_folded_from_the_arena() {
    // Translation admits runtime index values without reading operand RAM. Actual
    // execution proofs cover the signed selected-address arithmetic and CF output.
    for (source, value) in [
        (Register32::Eax, 31_u32),
        (Register32::Ecx, 32),
        (Register32::Edx, u32::MAX),
        (Register32::Ebx, 0xffff_ffe0),
        (Register32::Esp, 0xffff_ffdf),
        (Register32::Ebp, 0x8000_0000),
        (Register32::Esi, 0x7fff_ffff),
        (Register32::Edi, 0),
    ] {
        let raw = REGISTERS
            .iter()
            .position(|&register| register == source)
            .unwrap() as u8;
        let mut bytes = vec![0xb8 + raw];
        bytes.extend(value.to_le_bytes());
        bytes.extend([0xf0, 0x0f, 0xab, (raw << 3) | 0x03, 0xeb, 0]);
        for resident in [false, true] {
            for entries in [false, true] {
                let mut engine = code(&bytes);
                let start = 16 + usize::from(raw) * 4;
                engine.arena_mut().unwrap()[start..start + 4]
                    .copy_from_slice(&(!value).to_le_bytes());
                describe(&mut engine, CODE, bytes.len(), entries);
                let before = engine.arena().to_vec();
                let decoded = decode_one(engine.memory().unwrap(), GuestAddress(CODE + 5)).unwrap();
                assert_eq!(
                    decoded.operation(),
                    &Operation::LockedMemoryBitSet32 {
                        address: address(Some(Register32::Ebx), None, 1, 0),
                        index: source
                    }
                );
                compile(&mut engine, resident, entries).unwrap();
                assert_eq!(engine.arena(), before);
                assert_eq!(engine.memory().unwrap().mapped_pages(), 1);
            }
        }
    }
}

#[test]
fn locked_bts32_standalone_refusal_precedes_a_later_invalid_opcode() {
    for form in [BASE.as_slice(), SIB.as_slice()] {
        for end in [[0xeb, 0], [0x0f, 0x0b]] {
            let bytes = [vec![0x90], form.to_vec(), end.to_vec()].concat();
            let engine = code(&bytes);
            let before = engine.arena().to_vec();
            let memory = engine.memory().unwrap();
            let limits = CompileLimits::default();
            let specs = [BlockSpec {
                entry: GuestAddress(CODE),
                byte_length: bytes.len() as u32,
            }];
            let expected = Some(instruction_error(
                CODE + 1,
                InstructionError::BackendUnsupported,
            ));
            assert_eq!(prepare_region(memory, &specs, limits).err(), expected);
            assert_eq!(compile_region(memory, &specs, limits).err(), expected);
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
fn locked_bts32_progressive_fetch_uses_execute_only_and_never_wraps() {
    let longest = [0xf0, 0x0f, 0xab, 0x84, 0xc0, 0xe0, 0xff, 0xff, 0xff];
    let mut cuts = 0;
    for bytes in [BASE.as_slice(), longest.as_slice()] {
        for pc in [
            0x2000 - bytes.len() as u32,
            0x1fff,
            u32::MAX - bytes.len() as u32 + 1,
        ] {
            let engine = code_at(pc, bytes, true);
            let decoded = decode_one(engine.memory().unwrap(), GuestAddress(pc)).unwrap();
            assert_eq!(usize::from(decoded.length()), bytes.len());
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
                        }
                    ))
                );
                cuts += 1;
            }
        }
    }
    assert_eq!(cuts, 22);
    let mut first = code(&longest);
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
fn locked_bts32_source_pages_alone_control_both_owner_guards() {
    let bytes = [
        0xf0, 0x0f, 0xab, 0x84, 0xc0, 0xe0, 0xff, 0xff, 0xff, 0xeb, 0,
    ];
    for changed in [0x1ffc, 0x1fff, 0x2000, 0x2004] {
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
fn locked_bts32_mixed_sixty_four_instructions_and_existing_limits_are_exact() {
    let limits = CompileLimits::default();
    assert_eq!(
        (limits.blocks, limits.instructions, limits.wasm_bytes),
        (8, 64, 65_536)
    );
    let mut mixed = [0xf0, 0x0f, 0xab, 0x0b, 0x0f, 0xab, 0x0b, 0x0f, 0xab, 0xc8].repeat(21);
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
fn locked_bts32_late_fourth_block_failure_retains_dirty_full_arena_and_both_owners() {
    let mut bytes = vec![0x90; 514];
    for offset in [0, 32, 64] {
        bytes[offset..offset + 6].copy_from_slice(&[0xf0, 0x0f, 0xab, 0x37, 0xeb, 0]);
    }
    bytes[96..108].copy_from_slice(&[
        0xf0, 0x0f, 0xab, 0x37, 0xf0, 0x0f, 0xab, 0x44, 0x87, 0xf0, 0x0f, 0x0b,
    ]);
    bytes[512..].copy_from_slice(&[0xeb, 0]);
    let mut engine = code(&bytes);
    describe(&mut engine, CODE + 512, 2, false);
    let replacement = compile(&mut engine, false, false).unwrap() as u32;
    let resident = compile(&mut engine, true, false).unwrap();
    let replacement_bytes = engine.artifact_bytes().unwrap().to_vec();
    let replacement_pointer = engine.artifact_bytes().unwrap().as_ptr();
    let resident_bytes = engine.resident_bytes(resident).unwrap().to_vec();
    let resident_pointer = engine.resident_bytes(resident).unwrap().as_ptr();
    let arena_pointer = engine.arena_address();
    let fp_template = engine.arena()[X87_OFFSET..].to_vec();
    let mut page = [0; 4096];
    engine
        .memory()
        .unwrap()
        .fetch(GuestAddress(CODE), &mut page)
        .unwrap();
    let error = instruction_error(
        CODE + 106,
        InstructionError::Decode(DecodeError::Unsupported(UnsupportedFeature::Opcode)),
    );
    for is_resident in [false, true] {
        for entries in [false, true] {
            for (index, byte) in engine.arena_mut().unwrap().iter_mut().enumerate() {
                *byte = (index.wrapping_mul(37).wrapping_add(11) & 0xff) as u8;
            }
            let fp = &mut engine.arena_mut().unwrap()[X87_OFFSET..];
            fp.copy_from_slice(&fp_template);
            fp[16..18].copy_from_slice(&0x027f_u16.to_le_bytes());
            fp[18..20].copy_from_slice(&0x81a5_u16.to_le_bytes());
            for (index, byte) in fp[40..120].iter_mut().enumerate() {
                *byte = (index as u8).wrapping_mul(29).wrapping_add(17);
            }
            for (index, (offset, length)) in [(0_u32, 6_u32), (32, 6), (64, 6), (96, 12)]
                .into_iter()
                .enumerate()
            {
                let start = TRANSFER_OFFSET + index * if entries { 4 } else { 8 };
                engine.arena_mut().unwrap()[start..start + 4]
                    .copy_from_slice(&(CODE + offset).to_le_bytes());
                if !entries {
                    engine.arena_mut().unwrap()[start + 4..start + 8]
                        .copy_from_slice(&length.to_le_bytes());
                }
            }
            let before = engine.arena().to_vec();
            assert_eq!(before.len(), 4612);
            let result = match (is_resident, entries) {
                (false, false) => engine.compile(4).map(u64::from),
                (false, true) => engine.compile_entries(4, 0).map(u64::from),
                (true, false) => engine.compile_resident(4).map(|id| id.get()),
                (true, true) => engine.compile_resident_entries(4, 0).map(|id| id.get()),
            };
            assert_eq!(
                result,
                Err(if is_resident {
                    HostError::Resident(RegistryError::Compile(error))
                } else {
                    HostError::Compile(error)
                }),
                "resident={is_resident}, entries={entries}"
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
