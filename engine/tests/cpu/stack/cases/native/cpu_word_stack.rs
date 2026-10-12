use super::support_word_prefix as word_prefix;
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
            ir::{EffectiveAddress, Location32, Operation, UnaryKind, Value32, WordValue},
        },
    },
    memory::{
        Access, AddressSpace, FaultReason, GuestAddress, MemoryFault, PageRange, Permissions,
    },
    process::{EngineInstance, HostError},
};

const CODE: u32 = 0x1000;
const KEY: u64 = 0x5753_5441_434b_3136;

fn code(bytes: &[u8]) -> EngineInstance {
    let mut engine = EngineInstance::new(1, KEY).unwrap();
    engine.map(CODE, 1, 7).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(bytes);
    engine.upload(CODE, bytes.len() as u32).unwrap();
    engine.protect(CODE, 1, 4).unwrap();
    engine
}

fn admitted_forms() -> Vec<Vec<u8>> {
    let mut forms = Vec::new();
    for register in 0..8 {
        forms.extend([
            vec![0x66, 0x50 + register],
            vec![0x66, 0xff, 0xf0 | register],
            vec![0x66, 0x58 + register],
            vec![0x66, 0x8f, 0xc0 | register],
        ]);
    }
    forms.extend([
        vec![0x66, 0x68, 0x66, 0xf3],
        vec![0x66, 0x6a, 0x80],
        vec![0x66, 0xff, 0x33],
        vec![0x66, 0x8f, 0x03],
    ]);
    forms
}

#[test]
fn word_stack_data_forms_decode_without_stack_or_operand_access() {
    for bytes in admitted_forms() {
        let engine = code(&bytes);
        let before = engine.arena().to_vec();
        let decoded = decode_one(engine.memory().unwrap(), GuestAddress(CODE))
            .expect("WORD data-stack forms must decode without stack/operand access");
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
}

fn bound_admission(resident: bool, entries: bool) {
    let bytes = [
        0x66, 0x50, 0x66, 0xff, 0xf4, 0x66, 0x68, 0x66, 0xf3, 0x66, 0x6a, 0x80, 0x66, 0x58, 0x66,
        0x8f, 0xc4, 0x66, 0xff, 0x33, 0x66, 0x8f, 0x03, 0xeb, 0,
    ];
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
    .expect("WORD data-stack forms must compile bound without guest memory access");
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
fn word_stack_compiles_replacement_explicit() {
    bound_admission(false, false);
}
#[test]
fn word_stack_compiles_replacement_entry() {
    bound_admission(false, true);
}
#[test]
fn word_stack_compiles_resident_explicit() {
    bound_admission(true, false);
}
#[test]
fn word_stack_compiles_resident_entry() {
    bound_admission(true, true);
}

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

fn code_space(pc: u32, bytes: &[u8]) -> AddressSpace {
    let pages = (u64::from(pc & 0xfff) + bytes.len() as u64).div_ceil(4096) as u32;
    let mut memory = AddressSpace::new(pages).unwrap();
    let range = PageRange::new(GuestAddress(pc & !0xfff), pages).unwrap();
    memory.map_zeroed(range, Permissions::ALL).unwrap();
    memory.write(GuestAddress(pc), bytes).unwrap();
    memory.protect(range, Permissions::EXECUTE).unwrap();
    memory
}

fn addresses() -> [(&'static [u8], EffectiveAddress); 13] {
    use Register32::{Ebp, Ebx, Ecx, Esi, Esp};
    let address = |base, index, scale, displacement| EffectiveAddress {
        base,
        index,
        scale,
        displacement,
        ..Default::default()
    };
    [
        (&[0x03], address(Some(Ebx), None, 1, 0)),
        (&[0x04, 0x24], address(Some(Esp), None, 1, 0)),
        (&[0x45, 0], address(Some(Ebp), None, 1, 0)),
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
            &[0x05, 0xff, 0xff, 0xff, 0xff],
            address(None, None, 1, u32::MAX),
        ),
        (
            &[0x84, 0xf4, 0x66, 0x67, 0xf0, 0xf3],
            address(Some(Esp), Some(Esi), 8, 0xf3f0_6766),
        ),
    ]
}

fn memory_encoding(push: bool, tail: &[u8]) -> Vec<u8> {
    let mut bytes = vec![
        0x66,
        if push { 0xff } else { 0x8f },
        tail[0] | if push { 0x30 } else { 0 },
    ];
    bytes.extend_from_slice(&tail[1..]);
    bytes
}

fn memory_operation(push: bool, address: EffectiveAddress) -> Operation {
    if push {
        Operation::PushMemoryWord { source: address }
    } else {
        Operation::PopMemoryWord {
            destination: address,
        }
    }
}

fn assert_operation(bytes: &[u8], expected: Operation) {
    let memory = code_space(CODE, bytes);
    let decoded = decode_one(&memory, GuestAddress(CODE)).unwrap();
    assert_eq!(decoded.operation(), &expected, "{bytes:02x?}");
    assert_eq!(
        (decoded.length() as usize, decoded.next_pc()),
        (bytes.len(), GuestAddress(CODE + bytes.len() as u32))
    );
    assert!(memory.is_code_current(decoded.code_snapshot()));
    assert_eq!(memory.mapped_pages(), 1);
}

#[test]
fn word_stack_all_register_selectors_short_and_modrm_aliases_have_exact_ir() {
    let mut rows = 0;
    for (selector, register) in REGISTERS.into_iter().enumerate() {
        for bytes in [
            vec![0x66, 0x50 + selector as u8],
            vec![0x66, 0xff, 0xf0 | selector as u8],
        ] {
            assert_operation(
                &bytes,
                Operation::PushWord {
                    source: WordValue::Register(register),
                },
            );
            rows += 1;
        }
        for bytes in [
            vec![0x66, 0x58 + selector as u8],
            vec![0x66, 0x8f, 0xc0 | selector as u8],
        ] {
            assert_operation(
                &bytes,
                Operation::PopWord {
                    destination: register,
                },
            );
            rows += 1;
        }
    }
    assert_eq!(rows, 32);
}

#[test]
fn word_stack_immediate_width_sign_and_prefix_looking_payloads_are_literal() {
    for immediate in [0_u16, 1, 0x7fff, 0x8000, 0xffff, 0x6766, 0xf3f0, 0x9280] {
        let [low, high] = immediate.to_le_bytes();
        assert_operation(
            &[0x66, 0x68, low, high],
            Operation::PushWord {
                source: WordValue::Immediate(immediate),
            },
        );
    }
    for raw in [0_u8, 1, 0x7f, 0x80, 0xff, 0x66, 0x67, 0xf0, 0xf2, 0xf3] {
        assert_operation(
            &[0x66, 0x6a, raw],
            Operation::PushWord {
                source: WordValue::Immediate(i16::from(raw as i8) as u16),
            },
        );
    }
}

#[test]
fn word_stack_memory_forms_and_original_esp_aliases_have_exact_ir() {
    let mut rows = 0;
    for push in [false, true] {
        for (tail, address) in addresses() {
            assert_operation(
                &memory_encoding(push, tail),
                memory_operation(push, address),
            );
            rows += 1;
        }
        for sib in [0x66, 0x67, 0xf0, 0xf2, 0xf3] {
            let index = (sib >> 3) & 7;
            assert_operation(
                &memory_encoding(push, &[0x04, sib]),
                memory_operation(
                    push,
                    EffectiveAddress {
                        base: Some(REGISTERS[usize::from(sib & 7)]),
                        index: if index == 4 {
                            None
                        } else {
                            Some(REGISTERS[usize::from(index)])
                        },
                        scale: 1 << (sib >> 6),
                        displacement: 0,
                        ..Default::default()
                    },
                ),
            );
        }
        for (tail, address) in [
            (
                &[0x44, 0x24, 0xfe][..],
                EffectiveAddress {
                    base: Some(Register32::Esp),
                    index: None,
                    scale: 1,
                    displacement: 0xffff_fffe,
                    ..Default::default()
                },
            ),
            (
                &[0x04, 0x84][..],
                EffectiveAddress {
                    base: Some(Register32::Esp),
                    index: Some(Register32::Eax),
                    scale: 4,
                    displacement: 0,
                    ..Default::default()
                },
            ),
            (
                &[0x00][..],
                EffectiveAddress {
                    base: Some(Register32::Eax),
                    index: None,
                    scale: 1,
                    displacement: 0,
                    ..Default::default()
                },
            ),
        ] {
            assert_operation(
                &memory_encoding(push, tail),
                memory_operation(push, address),
            );
        }
    }
    assert_eq!(rows, 26);
}

#[test]
fn word_stack_prefix_lock_and_closed_stack_neighbors_remain_precise() {
    let unsupported = DecodeError::Unsupported(UnsupportedFeature::Opcode);
    for bytes in [
        vec![0x66, 0x54],
        vec![0x66, 0xff, 0xf4],
        vec![0x66, 0x5c],
        vec![0x66, 0x8f, 0xc4],
        vec![0x66, 0x68, 0x66, 0xf3],
        vec![0x66, 0x6a, 0x80],
        vec![0x66, 0xff, 0x34, 0x24],
        vec![0x66, 0x8f, 0x04, 0x24],
    ] {
        for prefix in [
            0x66, 0x67, 0xf2, 0xf3, 0xf0, 0x26, 0x2e, 0x36, 0x3e, 0x64, 0x65,
        ] {
            let expected = match prefix {
                0xf0 => DecodeError::InvalidEncoding,
                0x26 | 0x2e | 0x36 | 0x3e | 0x64 | 0x65 => {
                    DecodeError::Unsupported(UnsupportedFeature::Segment)
                }
                _ => unsupported,
            };
            for prefixed in [
                [vec![prefix], bytes.clone()].concat(),
                [vec![0x66, prefix], bytes[1..].to_vec()].concat(),
            ] {
                if matches!(prefix, 0x67 | 0x26 | 0x2e | 0x36 | 0x3e | 0x64 | 0x65) {
                    word_prefix::admitted(&prefixed);
                    continue;
                }
                assert_eq!(
                    decode_one(&code_space(CODE, &prefixed), GuestAddress(CODE)).err(),
                    Some(expected),
                    "{prefixed:02x?}"
                );
            }
        }
    }
    for bytes in [
        &[0x66, 0x66, 0xc9][..],
        &[0x66, 0x66, 0x9c],
        &[0x66, 0x9d],
        &[0x66, 0x66, 0xff, 0xd0],
        &[0x66, 0x66, 0xff, 0xe0],
        &[0x66, 0xff, 0x1b],
        &[0x66, 0xff, 0x2b],
    ] {
        if matches!(bytes, [0x9d] | [0x66, 0x9d]) {
            assert_eq!(
                decode_one(&code_space(CODE, bytes), GuestAddress(CODE))
                    .unwrap()
                    .operation(),
                &Operation::Control(ring3_engine::cpu::x86::ControlOperation::PopFlags {
                    word: bytes.contains(&0x66),
                })
            );
            continue;
        }

        if !word_prefix::admit_if_new_prefix(&bytes) {
            assert_eq!(
                decode_one(&code_space(CODE, bytes), GuestAddress(CODE)).err(),
                Some(unsupported),
                "{bytes:02x?}"
            );
        }
    }
    let (push_all, length) = word_prefix::admitted(&[0x66, 0x60]);
    assert_eq!(length, 2);
    assert!(matches!(
        push_all,
        Operation::StackFrame {
            kind: ring3_engine::cpu::x86::StackFrameKind::PushAll16,
            allocation: 0,
            nesting: 0
        }
    ));
    for bytes in [
        &[0x66, 0x06][..],
        &[0x66, 0x07],
        &[0x66, 0x0f, 0xa0],
        &[0x66, 0x0f, 0xa1],
    ] {
        let (operation, length) = word_prefix::admitted(bytes);
        assert_eq!(length, bytes.len());
        assert!(matches!(
            operation,
            Operation::PushSegment { word: true, .. } | Operation::PopSegment { word: true, .. }
        ));
    }
    for bytes in [
        &[0x66, 0xff, 0xfb][..],
        &[0x66, 0x8f, 0xcb],
        &[0x66, 0x8f, 0x0b],
    ] {
        assert_eq!(
            decode_one(&code_space(CODE, bytes), GuestAddress(CODE)).err(),
            Some(DecodeError::InvalidEncoding),
            "{bytes:02x?}"
        );
    }
}

#[test]
fn word_stack_all_selector_and_memory_banks_compile_in_four_bound_profiles() {
    let mut banks = vec![(
        admitted_forms().into_iter().flatten().collect::<Vec<_>>(),
        36,
    )];
    for push in [false, true] {
        banks.push((
            addresses()
                .into_iter()
                .flat_map(|(tail, _)| memory_encoding(push, tail))
                .collect(),
            13,
        ));
    }
    for (mut bytes, instructions) in banks {
        assert!(instructions < 64);
        bytes.extend([0xeb, 0]);
        for resident in [false, true] {
            for entries in [false, true] {
                assert_bound_admission(&bytes, resident, entries);
            }
        }
    }
}

#[test]
fn decoded_word_stack_forms_stay_closed_in_all_standalone_apis() {
    for instruction in [
        &[0x66, 0x54][..],
        &[0x66, 0xff, 0xf4],
        &[0x66, 0x5c],
        &[0x66, 0x8f, 0xc4],
        &[0x66, 0x68, 0x80, 0x92],
        &[0x66, 0x6a, 0x80],
        &[0x66, 0xff, 0x34, 0x24],
        &[0x66, 0x8f, 0x04, 0x24],
    ] {
        let bytes = [vec![0x90], instruction.to_vec(), vec![0xeb, 0]].concat();
        let memory = code_space(CODE, &bytes);
        let snapshot = memory
            .snapshot_code(GuestAddress(CODE), bytes.len())
            .unwrap();
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
        assert!(memory.is_code_current(&snapshot));
    }
}

#[test]
fn word_stack_dword_neighbors_fixed32_and_prefix_payload_adjacency_are_literal() {
    for (target, expected) in [
        (
            vec![0x66, 0x54],
            Operation::PushWord {
                source: WordValue::Register(Register32::Esp),
            },
        ),
        (
            vec![0x66, 0x8f, 0xc4],
            Operation::PopWord {
                destination: Register32::Esp,
            },
        ),
        (
            memory_encoding(false, addresses()[12].0),
            memory_operation(false, addresses()[12].1),
        ),
    ] {
        let bytes = [vec![0x48], target.clone()].concat();
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
        let next = decode_one(&memory, first.next_pc()).unwrap();
        assert_eq!(next.operation(), &expected);
        assert_eq!(next.length() as usize, target.len());
        for payload in [[0x66, 0x67, 0xf0, 0xf3], [0xf2, 0x68, 0x66, 0x8f]] {
            let bytes = [vec![0xb8], payload.to_vec(), target.clone()].concat();
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
            let next = decode_one(&memory, first.next_pc()).unwrap();
            assert_eq!(next.operation(), &expected);
            assert_eq!(
                (next.length() as usize, next.next_pc()),
                (target.len(), GuestAddress(CODE + 5 + target.len() as u32))
            );
        }
    }
    for (bytes, expected) in [
        (
            &[0x54][..],
            Operation::Push {
                source: Value32::Register(Register32::Esp),
            },
        ),
        (
            &[0xff, 0xf4][..],
            Operation::Push {
                source: Value32::Register(Register32::Esp),
            },
        ),
        (
            &[0x5c][..],
            Operation::Pop {
                destination: Location32::Register(Register32::Esp),
            },
        ),
        (
            &[0x8f, 0xc4][..],
            Operation::Pop {
                destination: Location32::Register(Register32::Esp),
            },
        ),
        (
            &[0x68, 0x66, 0x67, 0xf0, 0xf3][..],
            Operation::Push {
                source: Value32::Immediate(0xf3f0_6766),
            },
        ),
        (
            &[0x6a, 0x80][..],
            Operation::Push {
                source: Value32::Immediate(0xffff_ff80),
            },
        ),
        (
            &[0xff, 0x33][..],
            Operation::Push {
                source: Value32::Memory(addresses()[0].1),
            },
        ),
        (
            &[0x8f, 0x03][..],
            Operation::Pop {
                destination: Location32::Memory(addresses()[0].1),
            },
        ),
    ] {
        assert_operation(bytes, expected);
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
fn word_stack_fetch_checks_two_to_eight_byte_page_top_and_execute_boundaries() {
    for bytes in [
        vec![0x66, 0x54],
        vec![0x66, 0xff, 0xf4],
        vec![0x66, 0x5c],
        vec![0x66, 0x8f, 0xc4],
        vec![0x66, 0x68, 0x66, 0xf3],
        vec![0x66, 0x6a, 0x80],
        memory_encoding(true, addresses()[12].0),
        memory_encoding(false, addresses()[12].0),
    ] {
        assert!((2..=8).contains(&bytes.len()));
        for pc in [0x1fff, u32::MAX - bytes.len() as u32 + 1] {
            let memory = code_space(pc, &bytes);
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
        for cut in 1..bytes.len() {
            let pc = 0x2000 - cut as u32;
            assert_eq!(
                decode_one(&code_space(pc, &bytes[..cut]), GuestAddress(pc)).err(),
                Some(fetch_error(pc, 0x2000, FaultReason::Unmapped, cut + 1))
            );
            let top = u32::MAX - cut as u32 + 1;
            assert_eq!(
                decode_one(&code_space(top, &bytes[..cut]), GuestAddress(top)).err(),
                Some(fetch_error(top, top, FaultReason::AddressOverflow, cut + 1))
            );
        }
        let pc = 0x2000 - bytes.len() as u32 + 1;
        let mut memory = code_space(pc, &bytes);
        memory
            .protect(
                PageRange::new(GuestAddress(0x2000), 1).unwrap(),
                Permissions::READ,
            )
            .unwrap();
        assert_eq!(
            decode_one(&memory, GuestAddress(pc)).err(),
            Some(fetch_error(
                pc,
                0x2000,
                FaultReason::Permission,
                bytes.len()
            ))
        );
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
    }
}

#[test]
fn word_stack_mixed_64_instruction_block_fits_four_bound_profiles() {
    let mut bytes = Vec::new();
    for selector in 0..8 {
        bytes.extend([0x66, 0x50 + selector, 0x66, 0x58 + selector]);
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
fn word_stack_late_refusal_preserves_full_arena_and_both_owners() {
    let mut bytes = vec![0x66, 0x54];
    bytes.extend(memory_encoding(false, &[0x44, 0x24, 0xfe]));
    let refused_pc = CODE + bytes.len() as u32;
    bytes.extend([0xf0, 0x90]);
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
        cause: InstructionError::Decode(DecodeError::InvalidEncoding),
    };
    for is_resident in [false, true] {
        for entries in [false, true] {
            let descriptor = &mut engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8];
            descriptor[..4].copy_from_slice(&CODE.to_le_bytes());
            descriptor[4..]
                .copy_from_slice(&if entries { 0 } else { refused_length }.to_le_bytes());
            let before = engine.arena().to_vec();
            assert_eq!(before.len(), 4612);
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
