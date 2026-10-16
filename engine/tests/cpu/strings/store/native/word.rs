use super::support_string_prefix as string_prefix;
use ring3_engine::{abi::arena::TRANSFER_OFFSET, process::EngineInstance};
use std::io::Write;
use string_prefix::check as check_string_prefix;

#[test]
fn stosw_admits_in_bound_engine() {
    let output = std::env::var_os("RING3_STOS_BASELINE_DIR").map(std::path::PathBuf::from);
    for (name, bytes) in [
        ("stosb", &[0xaa, 0xeb, 0][..]),
        ("stosd", &[0xab, 0xeb, 0][..]),
        ("stosw", &[0x66, 0xab, 0xeb, 0][..]),
    ] {
        let mut engine = EngineInstance::new(1, 0x5152_5354_5556_5758).unwrap();
        engine.map(0x1000, 1, 7).unwrap();
        engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
            .copy_from_slice(bytes);
        engine.upload(0x1000, bytes.len() as u32).unwrap();
        engine.protect(0x1000, 1, 4).unwrap();
        engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8].copy_from_slice(
            &[0x1000_u32.to_le_bytes(), (bytes.len() as u32).to_le_bytes()].concat(),
        );
        engine.compile(1).expect("accumulator store must compile");
        if name != "stosw"
            && let Some(directory) = &output
        {
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(directory.join(format!("{name}.wasm")))
                .unwrap();
            file.write_all(engine.artifact_bytes().unwrap()).unwrap();
        }
    }
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
    memory::{Access, AddressSpace, FaultReason, GuestAddress, PageRange, Permissions},
    process::HostError,
};

const PC: u32 = 0x1000;
const KEY: u64 = 0x5152_5354_5556_5758;

fn memory(pc: u32, bytes: &[u8]) -> AddressSpace {
    let pages = (u64::from(pc & 0xfff) + bytes.len() as u64).div_ceil(4096) as u32;
    let mut memory = AddressSpace::new(pages).unwrap();
    let range = PageRange::new(GuestAddress(pc & !0xfff), pages).unwrap();
    memory.map_zeroed(range, Permissions::ALL).unwrap();
    memory.write(GuestAddress(pc), bytes).unwrap();
    memory.protect(range, Permissions::EXECUTE).unwrap();
    memory
}

fn upload(engine: &mut EngineInstance, pc: u32, bytes: &[u8]) {
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(bytes);
    engine.upload(pc, bytes.len() as u32).unwrap();
}

fn describe(engine: &mut EngineInstance, pc: u32, length: u32) {
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8]
        .copy_from_slice(&[pc.to_le_bytes(), length.to_le_bytes()].concat());
}

fn compile(engine: &mut EngineInstance, resident: bool, entries: bool) -> Result<u64, HostError> {
    match (resident, entries) {
        (false, false) => engine.compile(1).map(u64::from),
        (false, true) => engine.compile_entries(1, 0).map(u64::from),
        (true, false) => engine.compile_resident(1).map(|id| id.get()),
        (true, true) => engine.compile_resident_entries(1, 0).map(|id| id.get()),
    }
}

#[test]
fn exact_word_decode_fetch_snapshot_and_instruction_wrap() {
    for pc in [PC, 0x1ffe, 0x1fff, 0xffff_fffe] {
        let mut memory = memory(pc, &[0x66, 0xab]);
        let decoded = decode_one(&memory, GuestAddress(pc)).unwrap();
        assert_eq!(
            decoded.operation(),
            &Operation::String {
                kind: StringKind::Store,
                width: StringWidth::Word,
                address_size: AddressSize::Bits32,
                source_segment: None
            }
        );
        assert_eq!(
            (decoded.length(), decoded.next_pc()),
            (2, GuestAddress(pc.wrapping_add(2)))
        );
        assert!(memory.is_code_current(decoded.code_snapshot()));
        let pages = (u64::from(pc & 0xfff) + 2).div_ceil(4096) as u32;
        memory
            .protect(
                PageRange::new(GuestAddress(pc & !0xfff), pages).unwrap(),
                Permissions::ALL,
            )
            .unwrap();
        memory.write(GuestAddress(pc), &[0x66, 0xab]).unwrap();
        assert!(!memory.is_code_current(decoded.code_snapshot()));
    }
    for (pc, address, reason) in [
        (0x1fff, 0x2000, FaultReason::Unmapped),
        (u32::MAX, u32::MAX, FaultReason::AddressOverflow),
    ] {
        crate::support_fetch::assert_error(
            decode_one(&memory(pc, &[0x66]), GuestAddress(pc)).unwrap_err(),
            pc,
            address,
            reason,
            2,
        );
    }
    let mut denied = memory(PC, &[0x66, 0xab]);
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
        FaultReason::Permission,
        1,
    );
    let mut second = memory(0x1fff, &[0x66, 0xab]);
    second
        .protect(
            PageRange::new(GuestAddress(0x2000), 1).unwrap(),
            Permissions::READ,
        )
        .unwrap();
    crate::support_fetch::assert_error(
        decode_one(&second, GuestAddress(0x1fff)).unwrap_err(),
        0x1fff,
        0x2000,
        FaultReason::Permission,
        2,
    );
}

#[test]
fn byte_exact_prefix_metadata_and_both_standalone_refusals() {
    for bytes in [
        &[0x66, 0xaa][..],
        &[0x67, 0xa5],
        &[0x66, 0xa6],
        &[0x66, 0xae],
        &[0x66, 0x66, 0x90],
        &[0x66, 0x66, 0xab],
        &[0x66, 0x67, 0xab],
        &[0x67, 0x66, 0xab],
    ] {
        check_string_prefix(decode_one(&memory(PC, bytes), GuestAddress(PC)), bytes);
    }
    for prefix in [0xf2, 0xf3, 0x26, 0x2e, 0x36, 0x3e, 0x64, 0x65] {
        check_string_prefix(
            decode_one(&memory(PC, &[prefix, 0x66, 0xab]), GuestAddress(PC)),
            &[prefix, 0x66, 0xab],
        );
    }
    crate::support_fetch::assert_decode_contract(
        decode_one(&memory(PC, &[0xf0, 0x66, 0xab]), GuestAddress(PC)).unwrap_err(),
        DecodeError::InvalidEncoding,
    );
    let memory = memory(PC, &[0x90, 0x66, 0xab, 0xeb, 0]);
    let expected = CompileError::Instruction {
        pc: GuestAddress(PC + 1),
        cause: InstructionError::BackendUnsupported,
    };
    assert_eq!(
        compile_region(
            &memory,
            &[BlockSpec {
                entry: GuestAddress(PC),
                byte_length: 5
            }],
            CompileLimits::default()
        )
        .unwrap_err(),
        expected
    );
    assert_eq!(
        compile_entry_region(&memory, &[GuestAddress(PC)], CompileLimits::default()).unwrap_err(),
        expected
    );
}

#[test]
fn four_bound_profiles_validate_store16_only_without_guest_execution() {
    let mut input = Vec::new();
    for resident in [false, true] {
        for entries in [false, true] {
            let mut engine = EngineInstance::new(1, KEY).unwrap();
            engine.map(PC, 1, 7).unwrap();
            upload(&mut engine, PC, &[0x66, 0xab, 0xeb, 0]);
            engine.protect(PC, 1, 4).unwrap();
            describe(&mut engine, PC, 4);
            let id = compile(&mut engine, resident, entries).unwrap();
            let module = if resident {
                engine.resident_bytes(id).unwrap()
            } else {
                engine.artifact_bytes().unwrap()
            };
            input.push(u8::from(resident));
            input.extend_from_slice(&(module.len() as u32).to_le_bytes());
            input.extend_from_slice(module);
            assert_eq!(engine.memory().unwrap().mapped_pages(), 1);
            assert!(
                engine
                    .memory()
                    .unwrap()
                    .resolve(GuestAddress(0x5000), Access::Write)
                    .is_err()
            );
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
            &[(
                if resident {
                    "cpu_store_resident16"
                } else {
                    "cpu_store16"
                },
                if resident { 6 } else { 2 },
            )],
            &[
                4,
                if resident { 7 } else { 6 },
                if resident { 6 } else { 2 },
            ],
            true,
        );
        cursor += size;
        count += 1;
    }
    assert_eq!(cursor, input.len());
    assert_eq!(count, 4);
}

#[test]
fn late_refusal_preserves_full_arena_and_both_prior_owners() {
    for resident in [false, true] {
        for entries in [false, true] {
            let mut engine = EngineInstance::new(3, KEY).unwrap();
            for (pc, bytes) in [
                (PC, &[0x66, 0xab, 0xeb, 0][..]),
                (0x3000, &[0x90, 0xf0, 0x90, 0xeb, 0][..]),
                (0x4000, &[0x90, 0xeb, 0][..]),
            ] {
                engine.map(pc, 1, 7).unwrap();
                upload(&mut engine, pc, bytes);
                engine.protect(pc, 1, 4).unwrap();
            }
            describe(&mut engine, 0x4000, 3);
            let generation = engine.compile(1).unwrap();
            let keep = engine.compile_resident(1).unwrap().get();
            let before_modules = (
                engine.artifact_bytes().unwrap().to_vec(),
                engine.resident_bytes(keep).unwrap().to_vec(),
            );
            describe(&mut engine, 0x3000, 5);
            let before = engine.arena().to_vec();
            let expected = CompileError::Instruction {
                pc: GuestAddress(0x3001),
                cause: InstructionError::Decode(DecodeError::InvalidEncoding),
            };
            assert_eq!(
                compile(&mut engine, resident, entries).unwrap_err(),
                if resident {
                    HostError::Resident(RegistryError::Compile(expected))
                } else {
                    HostError::Compile(expected)
                }
            );
            assert_eq!(engine.arena(), before);
            assert_eq!(engine.artifact_bytes().unwrap(), before_modules.0);
            assert_eq!(engine.resident_bytes(keep).unwrap(), before_modules.1);
            engine.guard(KEY, generation).unwrap();
            engine.guard_resident(KEY, keep).unwrap();
            assert!(engine.lookup_resident(0x3000).is_err());
        }
    }
}

#[test]
fn instruction_and_block_caps_are_distinct_from_data_currency() {
    assert_eq!(
        CompileLimits::default(),
        CompileLimits {
            blocks: 8,
            instructions: 64,
            wasm_bytes: 65536
        }
    );
    for resident in [false, true] {
        let mut engine = EngineInstance::new(2, KEY).unwrap();
        engine.map(PC, 1, 7).unwrap();
        engine.map(0x5000, 1, 3).unwrap();
        let mut exact = vec![0x66, 0xab];
        exact.extend([0x90; 62]);
        exact.extend([0xeb, 0]);
        let mut too_many = vec![0x66, 0xab];
        too_many.extend([0x90; 63]);
        too_many.extend([0xeb, 0]);
        upload(&mut engine, 0x1100, &too_many);
        upload(&mut engine, PC, &exact);
        describe(&mut engine, PC, exact.len() as u32);
        let id = compile(&mut engine, resident, false).unwrap();
        let saved = if resident {
            engine.resident_bytes(id).unwrap().to_vec()
        } else {
            engine.artifact_bytes().unwrap().to_vec()
        };
        engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 4]
            .copy_from_slice(&[0x81, 0x90, 0x27, 0x44]);
        engine.upload(0x5000, 4).unwrap();
        if resident {
            engine.guard_resident(KEY, id).unwrap();
        } else {
            engine.guard(KEY, id as u32).unwrap();
        }
        describe(&mut engine, 0x1100, too_many.len() as u32);
        let before = engine.arena().to_vec();
        assert_eq!(
            compile(&mut engine, resident, false).unwrap_err(),
            if resident {
                HostError::Resident(RegistryError::Compile(CompileError::InstructionLimit))
            } else {
                HostError::Compile(CompileError::InstructionLimit)
            }
        );
        assert_eq!(engine.arena(), before);
        assert_eq!(
            if resident {
                engine.resident_bytes(id).unwrap()
            } else {
                engine.artifact_bytes().unwrap()
            },
            saved
        );
        upload(&mut engine, PC, &exact);
        assert!(
            if resident {
                engine.guard_resident(KEY, id)
            } else {
                engine.guard(KEY, id as u32)
            }
            .is_err()
        );
    }
    for resident in [false, true] {
        let mut engine = EngineInstance::new(1, KEY).unwrap();
        engine.map(PC, 1, 7).unwrap();
        let mut bytes = Vec::new();
        let mut descriptors = Vec::new();
        for index in 0..9 {
            bytes.extend([0x66, 0xab, 0xeb, 0]);
            descriptors.extend([PC + index * 4, 4]);
        }
        upload(&mut engine, PC, &bytes);
        let encoded: Vec<u8> = descriptors
            .iter()
            .flat_map(|word| word.to_le_bytes())
            .collect();
        engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + encoded.len()]
            .copy_from_slice(&encoded);
        let id = if resident {
            engine.compile_resident(8).unwrap().get()
        } else {
            u64::from(engine.compile(8).unwrap())
        };
        let saved = if resident {
            engine.resident_bytes(id).unwrap().to_vec()
        } else {
            engine.artifact_bytes().unwrap().to_vec()
        };
        let before = engine.arena().to_vec();
        let error = if resident {
            engine.compile_resident(9).unwrap_err()
        } else {
            engine.compile(9).unwrap_err()
        };
        assert_eq!(error, HostError::InvalidRequest);
        assert_eq!(engine.arena(), before);
        assert_eq!(
            if resident {
                engine.resident_bytes(id).unwrap()
            } else {
                engine.artifact_bytes().unwrap()
            },
            saved
        );
    }
}
