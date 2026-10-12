use super::support_prefix_admission as prefix_admission;
use ring3_engine::{
    abi::arena::{TRANSFER_OFFSET, X87_OFFSET},
    cpu::{
        UnsupportedFeature,
        dbt::{
            BlockSpec, CompileError, CompileLimits, InstructionError, RegistryError,
            prepare_entry_region, prepare_region,
        },
        x86::{
            Register32,
            atomic::AtomicKind,
            decode::{DecodeError, decode_one},
            ir::{AddressSize, AtomicSource, EffectiveAddress, Operation},
        },
    },
    memory::{Access, FaultReason, GuestAddress, MemoryFault},
    process::{EngineInstance, HostError},
};

const CODE: u32 = 0x2000;
const KEY: u64 = 0x4c4f_434b_3831_4e41;
const GROUPS: [u8; 3] = [0x08, 0x20, 0x30];

fn instruction(group: u8, mask: u32) -> Vec<u8> {
    let mut bytes = vec![0xf0, 0x81, group];
    bytes.extend(mask.to_le_bytes());
    bytes
}

fn ea(
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

fn operation(group: u8, address: EffectiveAddress, mask: u32) -> Operation {
    match group {
        0x08 => Operation::LockedMemoryOr32 { address, mask },
        0x20 => Operation::LockedMemoryAnd32 { address, mask },
        0x30 => Operation::LockedMemoryXor32 { address, mask },
        _ => panic!("unexpected logical group"),
    }
}

fn code_at(pc: u32, bytes: &[u8]) -> EngineInstance {
    let base = pc & !0xfff;
    let pages = (u64::from(pc - base) + bytes.len() as u64).div_ceil(4096) as u32;
    let mut engine = EngineInstance::new(pages + 1, KEY).unwrap();
    engine.map(base, pages, 7).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(bytes);
    engine.upload(pc, bytes.len() as u32).unwrap();
    engine.protect(base, pages, 4).unwrap();
    let arena = engine.arena_mut().unwrap();
    for index in 0..8 {
        let start = 16 + index * 4;
        arena[start..start + 4]
            .copy_from_slice(&(0xa100_1234_u32.wrapping_add(index as u32 * 0x10001)).to_le_bytes());
    }
    arena[48..52].copy_from_slice(&0xdead_beef_u32.to_le_bytes());
    arena[52..56].copy_from_slice(&0x0cd7_u32.to_le_bytes());
    arena[X87_OFFSET + 16..X87_OFFSET + 18].copy_from_slice(&0x027f_u16.to_le_bytes());
    arena[X87_OFFSET + 18..X87_OFFSET + 20].copy_from_slice(&0x81a5_u16.to_le_bytes());
    for (index, byte) in arena[X87_OFFSET + 40..X87_OFFSET + 120]
        .iter_mut()
        .enumerate()
    {
        *byte = (index as u8).wrapping_mul(29).wrapping_add(17);
    }
    engine
}

fn assert_operation(bytes: &[u8], expected: Operation) {
    let engine = code_at(CODE, bytes);
    let before = engine.arena().to_vec();
    let decoded = decode_one(engine.memory().unwrap(), GuestAddress(CODE)).unwrap();
    assert_eq!(decoded.operation(), &expected);
    assert_eq!(usize::from(decoded.length()), bytes.len());
    assert_eq!(decoded.next_pc(), GuestAddress(CODE + bytes.len() as u32));
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
            .resolve(GuestAddress(CODE), Access::Read)
            .is_err()
    );
    assert_eq!(engine.arena(), before);
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
fn literal_dword_masks_reuse_locked_ir_without_signextending_the_low_byte() {
    let address = ea(Some(Register32::Eax), None, 1, 0);
    for group in GROUPS {
        for mask in [
            0,
            0x80,
            0xff,
            0x8001_0000,
            0xfffd_ffbf,
            0x0040_0800,
            0x810f_66f0,
        ] {
            assert_operation(&instruction(group, mask), operation(group, address, mask));
        }
        for (immediate, signed_mask) in [(0x80, 0xffff_ff80), (0xff, u32::MAX)] {
            assert_operation(
                &[0xf0, 0x83, group, immediate],
                operation(group, address, signed_mask),
            );
            assert_operation(
                &instruction(group, u32::from(immediate)),
                operation(group, address, u32::from(immediate)),
            );
        }
    }
}

#[test]
fn sib_displacements_and_prefix_looking_immediates_remain_separate_payloads() {
    assert_operation(
        &[0xf0, 0x81, 0x0e, 0x80, 0, 0, 0],
        operation(0x08, ea(Some(Register32::Esi), None, 1, 0), 0x80),
    );
    assert_operation(
        &[0xf0, 0x81, 0x64, 0x24, 1, 0xff, 0, 0, 0],
        operation(0x20, ea(Some(Register32::Esp), None, 1, 1), 0xff),
    );
    assert_operation(
        &[
            0xf0, 0x81, 0xb4, 0x8e, 0xf0, 0xff, 0xff, 0xff, 0xf0, 0x66, 0x0f, 0x81,
        ],
        operation(
            0x30,
            ea(Some(Register32::Esi), Some(Register32::Ecx), 4, 0xffff_fff0),
            0x810f_66f0,
        ),
    );
    assert_operation(
        &[
            0xf0, 0x81, 0x8c, 0x66, 0x66, 0x67, 0xf0, 0xf3, 0x67, 0xf2, 0xf3, 0xf0,
        ],
        operation(
            0x08,
            ea(Some(Register32::Esi), None, 2, 0xf3f0_6766),
            0xf0f3_f267,
        ),
    );
    assert_operation(
        &[0xf0, 0x81, 0x25, 0xfc, 0xff, 0xff, 0xff, 0xff, 0, 0, 0],
        operation(0x20, ea(None, None, 1, 0xffff_fffc), 0xff),
    );
}

#[test]
fn final_seven_execute_only_bytes_decode_and_truncated_immediates_never_wrap() {
    for group in GROUPS {
        let bytes = instruction(group, 0x810f_66f0);
        for pc in [0x2ff9, 0xffff_fff9] {
            let engine = code_at(pc, &bytes);
            let before = engine.arena().to_vec();
            let decoded = decode_one(engine.memory().unwrap(), GuestAddress(pc)).unwrap();
            assert_eq!(decoded.length(), 7);
            assert_eq!(decoded.next_pc(), GuestAddress(pc.wrapping_add(7)));
            assert!(
                engine
                    .memory()
                    .unwrap()
                    .is_code_current(decoded.code_snapshot())
            );
            assert_eq!(engine.arena(), before);
        }
        for cut in 1..bytes.len() {
            for top in [false, true] {
                let pc = if top {
                    u32::MAX - cut as u32 + 1
                } else {
                    0x3000 - cut as u32
                };
                let mut engine = code_at(pc, &bytes[..cut]);
                if top {
                    engine.map(0, 1, 4).unwrap();
                }
                let before = engine.arena().to_vec();
                assert_eq!(
                    decode_one(engine.memory().unwrap(), GuestAddress(pc)).err(),
                    Some(fetch_error(
                        pc,
                        if top { pc } else { 0x3000 },
                        cut + 1,
                        if top {
                            FaultReason::AddressOverflow
                        } else {
                            FaultReason::Unmapped
                        },
                    )),
                    "group={group:x} cut={cut} top={top}"
                );
                assert_eq!(engine.arena(), before);
            }
        }
        let mut engine = code_at(0x2ffc, &bytes);
        engine.protect(0x3000, 1, 3).unwrap();
        let before = engine.arena().to_vec();
        assert_eq!(
            decode_one(engine.memory().unwrap(), GuestAddress(0x2ffc)).err(),
            Some(fetch_error(0x2ffc, 0x3000, 5, FaultReason::Permission))
        );
        assert_eq!(engine.arena(), before);
    }
}

#[test]
fn longer_immediates_keep_exact_prefix_and_standalone_boundaries() {
    let opcode = DecodeError::Unsupported(UnsupportedFeature::Opcode);
    for bytes in [
        vec![0xf0, 0xf0, 0x81, 0x08, 0, 0, 1, 0x80],
        vec![0x66, 0xf0, 0x81, 0x08, 0, 0x80],
        vec![0xf0, 0x66, 0x81, 0x08, 0, 0x80],
        vec![0x67, 0xf0, 0x81, 0x08, 0, 0, 1, 0x80],
        vec![0xf0, 0x81, 0x00, 0, 0, 1, 0x80],
        vec![0xf0, 0x81, 0x10, 0, 0, 1, 0x80],
        vec![0xf0, 0x81, 0x28, 0, 0, 1, 0x80],
    ] {
        if prefix_admission::admit_if_new_prefix(&bytes) {
            let (actual, length) = prefix_admission::admitted(&bytes);
            let expected = if bytes.contains(&0x66) {
                Operation::AtomicMemory {
                    kind: AtomicKind::Or,
                    width: 2,
                    address: ea(Some(Register32::Eax), None, 1, 0),
                    source: AtomicSource::Immediate(0x8000),
                }
            } else if bytes[0] == 0x67 {
                Operation::LockedMemoryOr32 {
                    address: EffectiveAddress {
                        base: Some(Register32::Ebx),
                        index: Some(Register32::Esi),
                        address_size: AddressSize::Bits16,
                        ..Default::default()
                    },
                    mask: 0x8001_0000,
                }
            } else {
                Operation::AtomicMemory {
                    kind: match bytes[2] {
                        0x00 => AtomicKind::Add,
                        0x10 => AtomicKind::AddCarry,
                        0x28 => AtomicKind::Subtract,
                        _ => unreachable!(),
                    },
                    width: 4,
                    address: ea(Some(Register32::Eax), None, 1, 0),
                    source: AtomicSource::Immediate(0x8001_0000),
                }
            };
            assert_eq!(actual, expected, "{bytes:02x?}");
            assert_eq!(length, bytes.len());
            continue;
        }
        let engine = code_at(CODE, &bytes);
        let before = engine.arena().to_vec();
        assert_eq!(
            decode_one(engine.memory().unwrap(), GuestAddress(CODE)).err(),
            Some(opcode)
        );
        assert_eq!(engine.arena(), before);
    }
    for group in GROUPS {
        let bytes = [
            vec![0x90],
            instruction(group, 0x810f_66f0),
            vec![0xf0, 0x90],
        ]
        .concat();
        let engine = code_at(CODE, &bytes);
        let before = engine.arena().to_vec();
        let memory = engine.memory().unwrap();
        let expected = Some(CompileError::Instruction {
            pc: GuestAddress(CODE + 1),
            cause: InstructionError::BackendUnsupported,
        });
        assert_eq!(
            prepare_region(
                memory,
                &[BlockSpec {
                    entry: GuestAddress(CODE),
                    byte_length: bytes.len() as u32
                }],
                CompileLimits::default()
            )
            .err(),
            expected
        );
        assert_eq!(
            prepare_entry_region(memory, &[GuestAddress(CODE)], CompileLimits::default()).err(),
            expected
        );
        assert_eq!(engine.arena(), before);
    }
}

#[test]
fn full_immediates_share_existing_sixty_four_instruction_bound_profile_limit() {
    let limits = CompileLimits::default();
    assert_eq!(
        (limits.blocks, limits.instructions, limits.wasm_bytes),
        (8, 64, 65_536)
    );
    let mut mixed = Vec::new();
    for _ in 0..21 {
        for (group, mask) in [
            (0x08, 0x8001_0000),
            (0x20, 0xfffd_ffbf),
            (0x30, 0x0040_0800),
        ] {
            mixed.extend(instruction(group, mask));
        }
    }
    mixed.extend([0xeb, 0]);
    let mut excessive = instruction(0x08, 0x8001_0000).repeat(64);
    excessive.extend([0xeb, 0]);
    for resident in [false, true] {
        for entries in [false, true] {
            let mut engine = code_at(CODE, &mixed);
            describe(&mut engine, CODE, mixed.len(), entries);
            let before = engine.arena().to_vec();
            let id = compile(&mut engine, resident, entries).unwrap();
            let module = if resident {
                engine.resident_bytes(id).unwrap()
            } else {
                engine.artifact_bytes().unwrap()
            };
            assert_eq!(&module[..8], b"\0asm\x01\0\0\0");
            assert!(module.len() <= limits.wasm_bytes);
            assert_eq!(engine.arena(), before);
            let mut engine = code_at(CODE, &excessive);
            describe(&mut engine, CODE, excessive.len(), entries);
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
fn late_poison_after_full_immediate_preserves_both_published_owners_and_dirty_arena() {
    let mut bytes = vec![0x90; 130];
    bytes[..10]
        .copy_from_slice(&[vec![0x90], instruction(0x30, 0x810f_66f0), vec![0xf0, 0x90]].concat());
    bytes[128..].copy_from_slice(&[0xeb, 0]);
    let mut engine = code_at(CODE, &bytes);
    describe(&mut engine, CODE + 128, 2, false);
    let generation = compile(&mut engine, false, false).unwrap() as u32;
    let resident = compile(&mut engine, true, false).unwrap();
    let replacement_bytes = engine.artifact_bytes().unwrap().to_vec();
    let resident_bytes = engine.resident_bytes(resident).unwrap().to_vec();
    let replacement_pointer = engine.artifact_bytes().unwrap().as_ptr();
    let resident_pointer = engine.resident_bytes(resident).unwrap().as_ptr();
    let cause = CompileError::Instruction {
        pc: GuestAddress(CODE + 8),
        cause: InstructionError::Decode(DecodeError::InvalidEncoding),
    };
    for is_resident in [false, true] {
        for entries in [false, true] {
            describe(&mut engine, CODE, 10, entries);
            let before = engine.arena().to_vec();
            let arena_pointer = engine.arena_address();
            assert_eq!(before.len(), 4612);
            assert_eq!(
                compile(&mut engine, is_resident, entries),
                Err(if is_resident {
                    HostError::Resident(RegistryError::Compile(cause))
                } else {
                    HostError::Compile(cause)
                })
            );
            assert_eq!(engine.arena(), before);
            assert_eq!(engine.arena_address(), arena_pointer);
            assert_eq!(engine.generation(), generation);
            assert_eq!(engine.artifact_bytes().unwrap(), replacement_bytes);
            assert_eq!(engine.resident_bytes(resident).unwrap(), resident_bytes);
            assert_eq!(
                engine.artifact_bytes().unwrap().as_ptr(),
                replacement_pointer
            );
            assert_eq!(
                engine.resident_bytes(resident).unwrap().as_ptr(),
                resident_pointer
            );
            assert_eq!(engine.guard(KEY, generation), Ok(()));
            assert_eq!(engine.guard_resident(KEY, resident), Ok(()));
        }
    }
}
