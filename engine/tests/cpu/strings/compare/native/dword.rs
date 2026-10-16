use super::support_string_prefix as string_prefix;
use ring3_engine::process::EngineInstance;
use string_prefix::check as check_string_prefix;

const PC: u32 = 0x1000;

fn compile(opcode: u8) {
    let mut engine = EngineInstance::new(1, 0x5152_5354_5556_5758).unwrap();
    engine.map(PC, 1, 7).unwrap();
    engine.arena_mut().unwrap()[140..143].copy_from_slice(&[opcode, 0xeb, 0]);
    engine.upload(PC, 3).unwrap();
    engine.protect(PC, 1, 4).unwrap();
    engine.arena_mut().unwrap()[140..148]
        .copy_from_slice(&[PC.to_le_bytes(), 3_u32.to_le_bytes()].concat());
    engine
        .compile(1)
        .expect("bare dword string comparison must compile without data mappings");
}

#[test]
fn cmpsd_admits_in_bound_engine() {
    compile(0xa7);
}

#[test]
fn scasd_admits_in_bound_engine() {
    compile(0xaf);
}

use ring3_engine::{
    cpu::{
        dbt::{
            BlockSpec, CompileError, CompileLimits, InstructionError, RegistryError,
            compile_entry_region, compile_region,
        },
        x86::{
            decode::{DecodeError, decode_one},
            ir::{AddressSize, Operation, StringKind, StringWidth},
        },
    },
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
    process::HostError,
};

fn memory(bytes: &[u8], pc: u32) -> AddressSpace {
    let mut memory = AddressSpace::new(1).unwrap();
    let page = PageRange::new(GuestAddress(pc & !0xfff), 1).unwrap();
    memory.map_zeroed(page, Permissions::ALL).unwrap();
    memory.write(GuestAddress(pc), bytes).unwrap();
    memory.protect(page, Permissions::EXECUTE).unwrap();
    memory
}

fn bound_compile(
    engine: &mut EngineInstance,
    resident: bool,
    entries: bool,
    count: u32,
) -> Result<Option<u64>, HostError> {
    if resident {
        let id = if entries {
            engine.compile_resident_entries(count, 0)?
        } else {
            engine.compile_resident(count)?
        };
        Ok(Some(id.get()))
    } else {
        if entries {
            engine.compile_entries(count, 0)?;
        } else {
            engine.compile(count)?;
        }
        Ok(None)
    }
}

fn module(engine: &EngineInstance, id: Option<u64>) -> Vec<u8> {
    match id {
        Some(id) => engine.resident_bytes(id).unwrap().to_vec(),
        None => engine.artifact_bytes().unwrap().to_vec(),
    }
}

fn code(bytes: &[u8]) -> EngineInstance {
    let mut engine = EngineInstance::new(2, 0x5152_5354_5556_5758).unwrap();
    engine.map(PC, 1, 7).unwrap();
    engine.arena_mut().unwrap()[140..140 + bytes.len()].copy_from_slice(bytes);
    engine.upload(PC, bytes.len() as u32).unwrap();
    engine.protect(PC, 1, 4).unwrap();
    engine
}

fn describe(engine: &mut EngineInstance, length: usize) {
    engine.arena_mut().unwrap()[140..148]
        .copy_from_slice(&[PC.to_le_bytes(), (length as u32).to_le_bytes()].concat());
}

#[test]
fn exact_dword_decode_fetch_prefix_and_standalone_boundaries() {
    for (opcode, operation) in [
        (
            0xa7,
            Operation::String {
                kind: StringKind::Compare,
                width: StringWidth::Dword,
                address_size: AddressSize::Bits32,
                source_segment: None,
            },
        ),
        (
            0xaf,
            Operation::String {
                kind: StringKind::Scan,
                width: StringWidth::Dword,
                address_size: AddressSize::Bits32,
                source_segment: None,
            },
        ),
    ] {
        for pc in [PC, 0x1fff, u32::MAX] {
            let memory = memory(&[opcode], pc);
            let decoded = decode_one(&memory, GuestAddress(pc)).unwrap();
            assert_eq!(decoded.operation(), &operation);
            assert_eq!(
                (decoded.length(), decoded.next_pc()),
                (1, GuestAddress(pc.wrapping_add(1)))
            );
            assert!(memory.is_code_current(decoded.code_snapshot()));
        }
        for prefix in [0xf3, 0xf2, 0x26, 0x64, 0x66, 0x67] {
            check_string_prefix(
                decode_one(
                    &memory(
                        &[prefix, if prefix == 0x66 { opcode - 1 } else { opcode }],
                        PC,
                    ),
                    GuestAddress(PC),
                ),
                &[prefix, if prefix == 0x66 { opcode - 1 } else { opcode }],
            );
        }
        crate::support_fetch::assert_decode_contract(
            decode_one(&memory(&[0xf0, opcode], PC), GuestAddress(PC)).unwrap_err(),
            DecodeError::InvalidEncoding,
        );
        let mut denied = memory(&[opcode], PC);
        denied
            .protect(
                PageRange::new(GuestAddress(PC), 1).unwrap(),
                Permissions::READ,
            )
            .unwrap();
        crate::support_fetch::assert_error(
            decode_one(&denied, GuestAddress(PC)).unwrap_err(),
            PC,
            PC,
            ring3_engine::memory::FaultReason::Permission,
            1,
        );
        crate::support_fetch::assert_error(
            decode_one(&AddressSpace::new(1).unwrap(), GuestAddress(PC)).unwrap_err(),
            PC,
            PC,
            ring3_engine::memory::FaultReason::Unmapped,
            1,
        );
        let bytes = [opcode, 0xeb, 0];
        let memory = memory(&bytes, PC);
        for result in [
            compile_region(
                &memory,
                &[BlockSpec {
                    entry: GuestAddress(PC),
                    byte_length: 3,
                }],
                CompileLimits::default(),
            ),
            compile_entry_region(&memory, &[GuestAddress(PC)], CompileLimits::default()),
        ] {
            assert!(matches!(
                result,
                Err(CompileError::Instruction {
                    pc: GuestAddress(PC),
                    cause: InstructionError::BackendUnsupported
                })
            ));
        }
    }
}

#[test]
fn all_four_profiles_have_only_guard_read32_and_unchanged_locals_and_arena() {
    let mut input = Vec::new();
    for opcode in [0xa7, 0xaf] {
        for resident in [false, true] {
            for entries in [false, true] {
                let mut engine = code(&[opcode, 0xeb, 0]);
                describe(&mut engine, 3);
                let before = engine.arena().to_vec();
                let id = bound_compile(&mut engine, resident, entries, 1).unwrap();
                assert_eq!(engine.arena(), before);
                let bytes = module(&engine, id);
                input.push(u8::from(resident));
                input.extend((bytes.len() as u32).to_le_bytes());
                input.extend(bytes);
            }
        }
    }
    let mut cursor = 0;
    let mut count = 0;
    while cursor < input.len() {
        let resident = match input[cursor] {
            0 => false,
            1 => true,
            other => panic!("invalid owner {other}"),
        };
        let size = u32::from_le_bytes(input[cursor + 1..cursor + 5].try_into().unwrap()) as usize;
        cursor += 5;
        crate::support_wasm_interface::assert_bound(
            &input[cursor..cursor + size],
            resident,
            &[("cpu_read32", 1)],
            &[4, if resident { 7 } else { 6 }, 1],
            true,
        );
        cursor += size;
        count += 1;
    }
    assert_eq!(cursor, input.len());
    assert_eq!(count, 8);
}

#[test]
fn late_refusal_retains_previous_owner_full_arena_and_source_currency() {
    for resident in [false, true] {
        let mut engine = code(&[0x90, 0xeb, 0]);
        describe(&mut engine, 3);
        let id = bound_compile(&mut engine, resident, false, 1).unwrap();
        let old = module(&engine, id);
        let bytes = [0x90, 0xa7, 0xf0, 0x90];
        engine.map(0x3000, 1, 7).unwrap();
        engine.arena_mut().unwrap()[140..144].copy_from_slice(&bytes);
        engine.upload(0x3000, 4).unwrap();
        engine.protect(0x3000, 1, 4).unwrap();
        engine.arena_mut().unwrap()[140..148]
            .copy_from_slice(&[0x3000_u32.to_le_bytes(), 4_u32.to_le_bytes()].concat());
        let before = engine.arena().to_vec();
        let error = bound_compile(&mut engine, resident, false, 1).unwrap_err();
        let expected = CompileError::Instruction {
            pc: GuestAddress(0x3002),
            cause: InstructionError::Decode(DecodeError::InvalidEncoding),
        };
        assert_eq!(
            error,
            if resident {
                HostError::Resident(RegistryError::Compile(expected))
            } else {
                HostError::Compile(expected)
            }
        );
        assert_eq!(engine.arena(), before);
        assert_eq!(module(&engine, id), old);
        if let Some(id) = id {
            engine.guard_resident(0x5152_5354_5556_5758, id).unwrap();
        } else {
            engine
                .guard(0x5152_5354_5556_5758, engine.generation())
                .unwrap();
        }
        engine.protect(PC, 1, 7).unwrap();
        engine.arena_mut().unwrap()[140..141].copy_from_slice(&[0x90]);
        engine.upload(PC, 1).unwrap();
        if let Some(id) = id {
            assert_eq!(
                engine.guard_resident(0x5152_5354_5556_5758, id),
                Err(HostError::Resident(RegistryError::CodeInvalidated))
            );
        } else {
            assert_eq!(
                engine.guard(0x5152_5354_5556_5758, engine.generation()),
                Err(HostError::CodeInvalidated)
            );
        }
    }
}

#[test]
fn dword_comparison_instruction_and_block_caps_remain_exact() {
    for resident in [false, true] {
        for nops in [62, 63] {
            let bytes = [vec![0x90; nops], vec![0xa7, 0xeb, 0]].concat();
            let mut engine = code(&bytes);
            describe(&mut engine, bytes.len());
            assert_eq!(
                bound_compile(&mut engine, resident, false, 1).is_ok(),
                nops == 62
            );
        }
        let mut engine = code(&[0xaf, 0xeb, 0].repeat(8));
        let descriptors: Vec<u8> = (0..8_u32)
            .flat_map(|index| [PC + index * 3, 3].into_iter().flat_map(u32::to_le_bytes))
            .collect();
        engine.arena_mut().unwrap()[140..204].copy_from_slice(&descriptors);
        let id = bound_compile(&mut engine, resident, false, 8).unwrap();
        let old = module(&engine, id);
        let before = engine.arena().to_vec();
        assert!(bound_compile(&mut engine, resident, false, 9).is_err());
        assert_eq!(engine.arena(), before);
        assert_eq!(module(&engine, id), old);
    }
}
