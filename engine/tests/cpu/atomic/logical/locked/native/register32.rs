use super::support_prefix_admission as prefix_admission;

use ring3_engine::{
    abi::arena::{TRANSFER_OFFSET, X87_OFFSET},
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

const CODE: u32 = 0x2000;
const KEY: u64 = 0x4c4f_434b_5247_4e41;
const OPCODES: [u8; 3] = [0x09, 0x21, 0x31];
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

fn instruction(opcode: u8, source: u8) -> Vec<u8> {
    vec![0xf0, opcode, 0x03 | source << 3]
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

fn operation(opcode: u8, address: EffectiveAddress, source: Register32) -> Operation {
    match opcode {
        0x09 => Operation::LockedMemoryOrRegister32 { address, source },
        0x21 => Operation::LockedMemoryAndRegister32 { address, source },
        0x31 => Operation::LockedMemoryXorRegister32 { address, source },
        _ => panic!("unexpected logical opcode"),
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
fn all_eight_full_register_sources_have_distinct_locked_ir() {
    let address = ea(Some(Register32::Ebx), None, 1, 0);
    for opcode in OPCODES {
        for (field, source) in REGISTERS.into_iter().enumerate() {
            assert_operation(
                &instruction(opcode, field as u8),
                operation(opcode, address, source),
            );
        }
    }
}

#[test]
fn source_base_index_and_stack_aliases_retain_the_full_original_address() {
    for (bytes, expected) in [
        (
            vec![0xf0, 0x09, 0x00],
            operation(0x09, ea(Some(Register32::Eax), None, 1, 0), Register32::Eax),
        ),
        (
            vec![0xf0, 0x21, 0x4c, 0x8e, 0xf0],
            operation(
                0x21,
                ea(Some(Register32::Esi), Some(Register32::Ecx), 4, 0xffff_fff0),
                Register32::Ecx,
            ),
        ),
        (
            vec![0xf0, 0x31, 0x64, 0x24, 1],
            operation(0x31, ea(Some(Register32::Esp), None, 1, 1), Register32::Esp),
        ),
        (
            vec![0xf0, 0x09, 0x34, 0xb5, 0x78, 0x56, 0x34, 0x92],
            operation(
                0x09,
                ea(None, Some(Register32::Esi), 4, 0x9234_5678),
                Register32::Esi,
            ),
        ),
        (
            vec![0xf0, 0x31, 0x1d, 0xfc, 0xff, 0xff, 0xff],
            operation(0x31, ea(None, None, 1, 0xffff_fffc), Register32::Ebx),
        ),
    ] {
        assert_operation(&bytes, expected);
    }
}

#[test]
fn mixed_register_and_immediate_operations_use_only_the_existing_atomic_helpers() {
    let mut bytes = Vec::new();
    for opcode in OPCODES {
        for source in 0..8 {
            bytes.extend(instruction(opcode, source));
        }
    }
    bytes.extend([0xf0, 0x83, 0x0b, 0x80]);
    bytes.extend([0xf0, 0x81, 0x23, 0xbf, 0xff, 0xfd, 0xff]);
    bytes.extend([0xf0, 0x81, 0x33, 0, 8, 0x40, 0]);
    bytes.extend([0xeb, 0]);
    for resident in [false, true] {
        for entries in [false, true] {
            let mut engine = code_at(CODE, &bytes);
            describe(&mut engine, CODE, bytes.len(), entries);
            let before = engine.arena().to_vec();
            let snapshot = engine
                .memory()
                .unwrap()
                .snapshot_code(GuestAddress(CODE), bytes.len())
                .unwrap();
            let id = compile(&mut engine, resident, entries).unwrap();
            let module = if resident {
                engine.resident_bytes(id).unwrap()
            } else {
                engine.artifact_bytes().unwrap()
            };
            assert_eq!(&module[..8], b"\0asm\x01\0\0\0");
            for base in ["atomic_or", "atomic_and", "atomic_xor"] {
                let required = if resident {
                    format!("{base}_resident_u32")
                } else {
                    format!("{base}_u32")
                };
                assert!(
                    module
                        .windows(required.len())
                        .any(|part| part == required.as_bytes()),
                    "{required}"
                );
            }
            for excluded in [b"read32".as_slice(), b"store32", b"compare_exchange32"] {
                assert!(!module.windows(excluded.len()).any(|part| part == excluded));
            }
            assert!(module.len() <= 65_536);
            assert_eq!(engine.arena(), before);
            assert!(engine.memory().unwrap().is_code_current(&snapshot));
            assert_eq!(engine.memory().unwrap().mapped_pages(), 1);
            assert!(
                engine
                    .memory()
                    .unwrap()
                    .resolve(GuestAddress(0), Access::Read)
                    .is_err()
            );
            if resident {
                assert_eq!(engine.guard_resident(KEY, id), Ok(()));
            } else {
                assert_eq!(engine.guard(KEY, id as u32), Ok(()));
            }
        }
    }
}

#[test]
fn final_three_bytes_and_progressive_sib_fetches_preserve_top_and_page_boundaries() {
    for bytes in [
        instruction(0x09, 0),
        instruction(0x21, 1),
        instruction(0x31, 4),
        vec![0xf0, 0x31, 0x84, 0x8e, 0x78, 0x56, 0x34, 0x92],
    ] {
        for end in [0x3000_u64, 1_u64 << 32] {
            let pc = (end - bytes.len() as u64) as u32;
            let engine = code_at(pc, &bytes);
            let before = engine.arena().to_vec();
            let decoded = decode_one(engine.memory().unwrap(), GuestAddress(pc)).unwrap();
            assert_eq!(usize::from(decoded.length()), bytes.len());
            assert_eq!(
                decoded.next_pc(),
                GuestAddress(pc.wrapping_add(bytes.len() as u32))
            );
            assert_eq!(engine.arena(), before);
            for cut in 1..bytes.len() {
                let pc = (end - cut as u64) as u32;
                let mut engine = code_at(pc, &bytes[..cut]);
                let top = end == 1_u64 << 32;
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
                    "{bytes:02x?} cut={cut} top={top}"
                );
                assert_eq!(engine.arena(), before);
            }
        }
    }
    let bytes = [0xf0, 0x31, 0x03];
    let mut engine = code_at(0x2ffe, &bytes);
    engine.protect(0x3000, 1, 3).unwrap();
    let before = engine.arena().to_vec();
    assert_eq!(
        decode_one(engine.memory().unwrap(), GuestAddress(0x2ffe)).err(),
        Some(fetch_error(0x2ffe, 0x3000, 3, FaultReason::Permission))
    );
    assert_eq!(engine.arena(), before);
}

#[test]
fn extra_prefixes_reverse_destinations_and_register_destinations_remain_closed() {
    let opcode = DecodeError::Unsupported(UnsupportedFeature::Opcode);
    let segment = DecodeError::Unsupported(UnsupportedFeature::Segment);
    for (bytes, expected) in [
        (vec![0xf0, 0xf0, 0x09, 0x08], opcode),
        (vec![0x66, 0xf0, 0x21, 0x08], opcode),
        (vec![0xf0, 0x66, 0x31, 0x08], opcode),
        (vec![0x67, 0xf0, 0x09, 0x08], opcode),
        (vec![0xf2, 0xf0, 0x21, 0x08], opcode),
        (vec![0xf3, 0xf0, 0x31, 0x08], opcode),
        (vec![0x64, 0xf0, 0x09, 0x08], segment),
        (vec![0xf0, 0x08, 0x08], opcode),
        (vec![0xf0, 0x20, 0x08], opcode),
        (vec![0xf0, 0x30, 0x08], opcode),
        (vec![0xf0, 0x0b, 0x03], DecodeError::InvalidEncoding),
        (vec![0xf0, 0x23, 0x03], DecodeError::InvalidEncoding),
        (vec![0xf0, 0x33, 0x03], DecodeError::InvalidEncoding),
        (vec![0xf0, 0x09, 0xc8], DecodeError::InvalidEncoding),
        (vec![0xf0, 0x21, 0xe0], DecodeError::InvalidEncoding),
        (vec![0xf0, 0x31, 0xf0], DecodeError::InvalidEncoding),
        (vec![0xf0, 0x01, 0x08], opcode),
    ] {
        if prefix_admission::admit_if_new_prefix(&bytes) {
            continue;
        }
        let engine = code_at(CODE, &bytes);
        let before = engine.arena().to_vec();
        assert_eq!(
            decode_one(engine.memory().unwrap(), GuestAddress(CODE)).err(),
            Some(expected),
            "{bytes:02x?}"
        );
        assert_eq!(engine.arena(), before);
    }
}

#[test]
fn standalone_paths_remain_closed_before_later_poison() {
    for opcode in OPCODES {
        let bytes = [vec![0x90], instruction(opcode, 4), vec![0xf0, 0x90]].concat();
        let engine = code_at(CODE, &bytes);
        let before = engine.arena().to_vec();
        let memory = engine.memory().unwrap();
        let specs = [BlockSpec {
            entry: GuestAddress(CODE),
            byte_length: bytes.len() as u32,
        }];
        let entries = [GuestAddress(CODE)];
        let expected = Some(CompileError::Instruction {
            pc: GuestAddress(CODE + 1),
            cause: InstructionError::BackendUnsupported,
        });
        for actual in [
            prepare_region(memory, &specs, CompileLimits::default()).err(),
            compile_region(memory, &specs, CompileLimits::default()).err(),
            prepare_entry_region(memory, &entries, CompileLimits::default()).err(),
            compile_entry_region(memory, &entries, CompileLimits::default()).err(),
        ] {
            assert_eq!(actual, expected);
        }
        assert_eq!(engine.arena(), before);
    }
}

#[test]
fn mixed_register_forms_keep_the_existing_instruction_limit() {
    let limits = CompileLimits::default();
    assert_eq!(
        (limits.blocks, limits.instructions, limits.wasm_bytes),
        (8, 64, 65_536)
    );
    let mut mixed = Vec::new();
    for _ in 0..21 {
        for opcode in OPCODES {
            mixed.extend(instruction(opcode, 4));
        }
    }
    mixed.extend([0xeb, 0]);
    let mut excessive = instruction(0x09, 4).repeat(64);
    excessive.extend([0xeb, 0]);
    for resident in [false, true] {
        for entries in [false, true] {
            let mut engine = code_at(CODE, &mixed);
            describe(&mut engine, CODE, mixed.len(), entries);
            let before = engine.arena().to_vec();
            compile(&mut engine, resident, entries).unwrap();
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
fn late_poison_after_register_mask_preserves_published_owners_and_full_arena() {
    let mut bytes = vec![0x90; 130];
    bytes[..6].copy_from_slice(&[vec![0x90], instruction(0x31, 4), vec![0xf0, 0x90]].concat());
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
        pc: GuestAddress(CODE + 4),
        cause: InstructionError::Decode(DecodeError::InvalidEncoding),
    };
    for is_resident in [false, true] {
        for entries in [false, true] {
            describe(&mut engine, CODE, 6, entries);
            let before = engine.arena().to_vec();
            let arena_pointer = engine.arena_address();
            assert_eq!(before.len(), ring3_engine::abi::arena::ARENA_SIZE);
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
