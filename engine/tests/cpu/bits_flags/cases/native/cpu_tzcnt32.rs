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
            ir::{BitScanKind, EffectiveAddress, Operation},
        },
    },
    memory::{Access, FaultReason, GuestAddress, MemoryFault},
    process::{EngineInstance, HostError},
};

const CODE: u32 = 0x2000;
const DATA: u32 = 0x5000;
const KEY: u64 = 0x545a_434e_5433_324e;
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

fn code_at(pc: u32, bytes: &[u8]) -> EngineInstance {
    let base = pc & !0xfff;
    let pages = (u64::from(pc - base) + bytes.len() as u64).div_ceil(4096) as u32;
    let mut engine = EngineInstance::new(pages + 1, KEY).unwrap();
    engine.map(base, pages, 7).unwrap();
    upload(&mut engine, pc, bytes);
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
fn register_self_and_rotated_sources_have_separate_count_ir() {
    for (field, destination) in REGISTERS.into_iter().enumerate() {
        for source in [field, (field + 1) & 7] {
            assert_operation(
                &[0xf3, 0x0f, 0xbc, 0xc0 | ((field as u8) << 3) | source as u8],
                Operation::TrailingZeroCount32 {
                    destination,
                    source: REGISTERS[source],
                },
            );
        }
    }
    for (opcode, kind) in [(0xbc, BitScanKind::Forward), (0xbd, BitScanKind::Reverse)] {
        assert_operation(
            &[0x0f, opcode, 0xc6],
            Operation::BitScan {
                kind,
                destination: Register32::Eax,
                source: Register32::Esi,
            },
        );
    }
}

#[test]
fn memory_destination_aliases_preserve_full_effective_addresses() {
    for (bytes, destination, address) in [
        (
            vec![0xf3, 0x0f, 0xbc, 0x00],
            Register32::Eax,
            ea(Some(Register32::Eax), None, 1, 0),
        ),
        (
            vec![0xf3, 0x0f, 0xbc, 0x44, 0x88, 0xf0],
            Register32::Eax,
            ea(Some(Register32::Eax), Some(Register32::Ecx), 4, 0xffff_fff0),
        ),
        (
            vec![0xf3, 0x0f, 0xbc, 0x64, 0x24, 1],
            Register32::Esp,
            ea(Some(Register32::Esp), None, 1, 1),
        ),
        (
            vec![0xf3, 0x0f, 0xbc, 0x6d, 0x80],
            Register32::Ebp,
            ea(Some(Register32::Ebp), None, 1, 0xffff_ff80),
        ),
        (
            vec![0xf3, 0x0f, 0xbc, 0x34, 0xb5, 0x78, 0x56, 0x34, 0x92],
            Register32::Esi,
            ea(None, Some(Register32::Esi), 4, 0x9234_5678),
        ),
        (
            vec![0xf3, 0x0f, 0xbc, 0x3d, 0xfc, 0xff, 0xff, 0xff],
            Register32::Edi,
            ea(None, None, 1, 0xffff_fffc),
        ),
    ] {
        assert_operation(
            &bytes,
            Operation::ReadTrailingZeroCount32 {
                destination,
                address,
            },
        );
    }
}

#[test]
fn standalone_register_bank_and_four_bound_read_profiles_are_neutral() {
    let mut registers = Vec::new();
    for field in 0..8 {
        registers.extend([0xf3, 0x0f, 0xbc, 0xc0 | field << 3 | field]);
    }
    registers.extend([0xeb, 0]);
    let engine = code_at(CODE, &registers);
    let before = engine.arena().to_vec();
    let memory = engine.memory().unwrap();
    let specs = [BlockSpec {
        entry: GuestAddress(CODE),
        byte_length: registers.len() as u32,
    }];
    for prepared in [
        prepare_region(memory, &specs, CompileLimits::default()).unwrap(),
        prepare_entry_region(memory, &[GuestAddress(CODE)], CompileLimits::default()).unwrap(),
    ] {
        assert_eq!(
            (prepared.block_count(), prepared.instruction_count()),
            (1, 9)
        );
    }
    for unit in [
        compile_region(memory, &specs, CompileLimits::default()).unwrap(),
        compile_entry_region(memory, &[GuestAddress(CODE)], CompileLimits::default()).unwrap(),
    ] {
        assert_eq!(
            (unit.metadata().blocks, unit.metadata().instructions),
            (1, 9)
        );
        let bytes = unit.wasm_bytes(memory).unwrap();
        assert!(!bytes.windows(6).any(|part| part == b"read32"));
    }
    assert_eq!(engine.arena(), before);
    let mixed = [
        0xf3, 0x0f, 0xbc, 0xc0, 0xf3, 0x0f, 0xbc, 0x05, 0x10, 0x50, 0, 0, 0xeb, 0,
    ];
    for resident in [false, true] {
        for entries in [false, true] {
            let mut engine = code_at(CODE, &mixed);
            describe(&mut engine, CODE, mixed.len(), entries);
            let before = engine.arena().to_vec();
            assert_eq!(before.len(), 4612);
            let id = compile(&mut engine, resident, entries).unwrap();
            let module = if resident {
                engine.resident_bytes(id).unwrap()
            } else {
                engine.artifact_bytes().unwrap()
            };
            assert_eq!(&module[..8], b"\0asm\x01\0\0\0");
            assert!(module.windows(6).any(|part| part == b"read32"));
            for excluded in [
                b"read8".as_slice(),
                b"read16",
                b"store32",
                b"exchange",
                b"atomic_",
            ] {
                assert!(!module.windows(excluded.len()).any(|part| part == excluded));
            }
            assert!(module.len() <= 65_536);
            assert_eq!(engine.arena(), before);
            assert_eq!(engine.memory().unwrap().mapped_pages(), 1);
            assert!(
                engine
                    .memory()
                    .unwrap()
                    .resolve(GuestAddress(DATA), Access::Read)
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
fn standalone_memory_rejection_precedes_later_poison() {
    let bytes = [0x90, 0xf3, 0x0f, 0xbc, 0x00, 0xf0, 0x90];
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

#[test]
fn only_single_f3_dword_count_is_open_and_segment_priority_is_preserved() {
    let opcode = DecodeError::Unsupported(UnsupportedFeature::Opcode);
    let segment = DecodeError::Unsupported(UnsupportedFeature::Segment);
    for (bytes, expected) in [
        (vec![0xf3, 0xf3, 0x0f, 0xbc, 0xc0], opcode),
        (vec![0xf2, 0xf3, 0x0f, 0xbc, 0xc0], opcode),
        (vec![0xf3, 0xf2, 0x0f, 0xbc, 0xc0], opcode),
        (vec![0x66, 0xf3, 0x0f, 0xbc, 0xc0], opcode),
        (vec![0xf3, 0x66, 0x0f, 0xbc, 0x00], opcode),
        (vec![0x67, 0xf3, 0x0f, 0xbc, 0x00], opcode),
        (vec![0xf3, 0x0f, 0xbd, 0xc0], opcode),
        (vec![0xf3, 0x0f, 0xbd, 0x00], opcode),
        (vec![0xf3, 0x0f, 0xb8, 0xc0], opcode),
        (
            vec![0xf0, 0xf3, 0x0f, 0xbc, 0x00],
            DecodeError::InvalidEncoding,
        ),
        (vec![0x64, 0xf3, 0x0f, 0xbc, 0x00], segment),
        (vec![0xf3, 0x65, 0x0f, 0xbc, 0xc0], segment),
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
fn progressive_register_and_sib_fetches_keep_page_and_top_fault_priority() {
    for bytes in [
        vec![0xf3, 0x0f, 0xbc, 0xc0],
        vec![0xf3, 0x0f, 0xbc, 0x84, 0x88, 0x78, 0x56, 0x34, 0x92],
    ] {
        for end in [0x3000_u64, 1_u64 << 32] {
            let pc = (end - bytes.len() as u64) as u32;
            let engine = code_at(pc, &bytes);
            let decoded = decode_one(engine.memory().unwrap(), GuestAddress(pc)).unwrap();
            assert_eq!(usize::from(decoded.length()), bytes.len());
            assert_eq!(
                decoded.next_pc(),
                GuestAddress(pc.wrapping_add(bytes.len() as u32))
            );
            for cut in 1..bytes.len() {
                let pc = (end - cut as u64) as u32;
                let top = end == 1_u64 << 32;
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
                    "{bytes:02x?} cut={cut} top={top}"
                );
                assert_eq!(engine.arena(), before);
            }
        }
    }
    let mut engine = code_at(0x2ffe, &[0xf3, 0x0f, 0xbc, 0xc0]);
    engine.protect(0x3000, 1, 3).unwrap();
    assert_eq!(
        decode_one(engine.memory().unwrap(), GuestAddress(0x2ffe)).err(),
        Some(fetch_error(0x2ffe, 0x3000, 3, FaultReason::Permission))
    );
}

#[test]
fn operand_only_changes_preserve_code_and_both_owners() {
    let bytes = [0xf3, 0x0f, 0xbc, 0x05, 0x10, 0x50, 0, 0, 0xeb, 0];
    let mut engine = code_at(CODE, &bytes);
    let decoded = decode_one(engine.memory().unwrap(), GuestAddress(CODE)).unwrap();
    describe(&mut engine, CODE, bytes.len(), false);
    let generation = compile(&mut engine, false, false).unwrap() as u32;
    let resident = compile(&mut engine, true, false).unwrap();
    let replacement_bytes = engine.artifact_bytes().unwrap().to_vec();
    let resident_bytes = engine.resident_bytes(resident).unwrap().to_vec();
    let replacement_pointer = engine.artifact_bytes().unwrap().as_ptr();
    let resident_pointer = engine.resident_bytes(resident).unwrap().as_ptr();
    engine.map(DATA, 1, 3).unwrap();
    engine.write32(DATA + 0x10, 0).unwrap();
    engine.protect(DATA, 1, 1).unwrap();
    for phase in 0..3 {
        if phase == 1 {
            engine.unmap(DATA, 1).unwrap();
        }
        if phase == 2 {
            engine.map(DATA, 1, 3).unwrap();
            engine.write32(DATA + 0x10, 0x8000_0000).unwrap();
            engine.protect(DATA, 1, 1).unwrap();
        }
        assert!(
            engine
                .memory()
                .unwrap()
                .is_code_current(decoded.code_snapshot())
        );
        assert_eq!(engine.guard(KEY, generation), Ok(()));
        assert_eq!(engine.guard_resident(KEY, resident), Ok(()));
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
    }
    engine.protect(CODE, 1, 7).unwrap();
    upload(&mut engine, CODE + 2, &[0xbc]);
    assert!(
        !engine
            .memory()
            .unwrap()
            .is_code_current(decoded.code_snapshot())
    );
    assert!(engine.guard(KEY, generation).is_err());
    assert!(engine.guard_resident(KEY, resident).is_err());
}

#[test]
fn count_forms_keep_the_default_instruction_limit() {
    let limits = CompileLimits::default();
    assert_eq!(
        (limits.blocks, limits.instructions, limits.wasm_bytes),
        (8, 64, 65_536)
    );
    for resident in [false, true] {
        for entries in [false, true] {
            for count in [63, 64] {
                let mut bytes = [0xf3, 0x0f, 0xbc, 0xc0].repeat(count);
                bytes.extend([0xeb, 0]);
                let mut engine = code_at(CODE, &bytes);
                describe(&mut engine, CODE, bytes.len(), entries);
                let before = engine.arena().to_vec();
                let result = compile(&mut engine, resident, entries);
                if count == 63 {
                    result.unwrap();
                } else {
                    assert_eq!(
                        result,
                        Err(if resident {
                            HostError::Resident(RegistryError::Compile(
                                CompileError::InstructionLimit,
                            ))
                        } else {
                            HostError::Compile(CompileError::InstructionLimit)
                        })
                    );
                    assert_eq!(engine.generation(), 0);
                }
                assert_eq!(engine.arena(), before);
            }
        }
    }
}

#[test]
fn late_count_poison_keeps_published_owners_and_full_arena() {
    let mut bytes = vec![0x90; 130];
    bytes[..6].copy_from_slice(&[0xf3, 0x0f, 0xbc, 0x00, 0xf0, 0x90]);
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
            assert_eq!(before.len(), 4612);
            assert_eq!(
                compile(&mut engine, is_resident, entries),
                Err(if is_resident {
                    HostError::Resident(RegistryError::Compile(cause))
                } else {
                    HostError::Compile(cause)
                }),
                "resident={is_resident} entries={entries}"
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
