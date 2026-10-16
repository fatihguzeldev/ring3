use super::support_word_prefix as word_prefix;
use ring3_engine::{abi::arena::TRANSFER_OFFSET, process::EngineInstance};
use std::{fs::OpenOptions, io::Write, path::PathBuf};

fn compile_bound(bytes: &[u8]) -> Vec<u8> {
    let mut engine = EngineInstance::new(1, 0x574f_5244_554e_4152).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(bytes);
    engine.upload(0x1000, bytes.len() as u32).unwrap();
    engine.protect(0x1000, 1, 4).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8]
        .copy_from_slice(&[0x1000_u32.to_le_bytes(), (bytes.len() as u32).to_le_bytes()].concat());
    engine
        .compile(1)
        .expect("register unary must compile without data pages");
    engine.artifact_bytes().unwrap().to_vec()
}

#[test]
fn word_neg_admits_in_bound_engine() {
    compile_bound(&[0x66, 0xf7, 0xd8, 0xeb, 0]);
}

#[test]
fn word_not_admits_in_bound_engine() {
    compile_bound(&[0x66, 0xf7, 0xd0, 0xeb, 0]);
}

#[test]
fn existing_neg_not_modules_validate_and_optionally_capture() {
    let directory = std::env::var_os("RING3_WORD_NEG_NOT_BASELINE_DIR").map(PathBuf::from);
    let mut input = Vec::new();
    for (name, bytes) in [
        ("byte-neg", &[0xf6, 0xd8, 0xeb, 0][..]),
        ("dword-neg", &[0xf7, 0xd8, 0xeb, 0][..]),
        ("byte-not", &[0xf6, 0xd0, 0xeb, 0][..]),
        ("dword-not", &[0xf7, 0xd0, 0xeb, 0][..]),
    ] {
        let module = compile_bound(bytes);
        if let Some(directory) = &directory {
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(directory.join(format!("{name}.wasm")))
                .unwrap()
                .write_all(&module)
                .unwrap();
        }
        input.extend((module.len() as u32).to_le_bytes());
        input.extend(module);
    }
    crate::support_wasm_interface::assert_packed(&input, 4, false);
}

use ring3_engine::{
    cpu::{
        UnsupportedFeature,
        dbt::{
            BlockSpec, CompileError, CompileLimits, InstructionError, RegistryError,
            compile_entry_region, compile_region,
        },
        x86::{
            Register32,
            decode::{DecodeError, decode_one},
            ir::{Operation, UnaryKind},
        },
    },
    memory::{
        Access, AddressSpace, FaultReason, GuestAddress, MemoryFault, PageRange, Permissions,
    },
    process::HostError,
};

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
const KEY: u64 = 0x574f_5244_554e_4152;

fn memory(pc: u32, bytes: &[u8]) -> AddressSpace {
    let base = pc & !0xfff;
    let pages = (u64::from(pc - base) + bytes.len() as u64).div_ceil(4096) as u32;
    let mut memory = AddressSpace::new(pages + 1).unwrap();
    let range = PageRange::new(GuestAddress(base), pages).unwrap();
    memory.map_zeroed(range, Permissions::ALL).unwrap();
    memory.write(GuestAddress(pc), bytes).unwrap();
    memory.protect(range, Permissions::EXECUTE).unwrap();
    memory
}

fn all_word_unary() -> Vec<u8> {
    let mut bytes = Vec::new();
    for base in [0xd8, 0xd0] {
        for alias in 0..8 {
            bytes.extend([0x66, 0xf7, base | alias]);
        }
    }
    bytes.extend([0xeb, 0]);
    bytes
}

fn upload(engine: &mut EngineInstance, pc: u32, bytes: &[u8]) {
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(bytes);
    engine.upload(pc, bytes.len() as u32).unwrap();
}

fn prepare_engine(pc: u32, bytes: &[u8]) -> EngineInstance {
    let mut engine = EngineInstance::new(3, KEY).unwrap();
    engine.map(pc, 1, 7).unwrap();
    upload(&mut engine, pc, bytes);
    engine.protect(pc, 1, 4).unwrap();
    engine
}

fn describe(engine: &mut EngineInstance, pc: u32, length: u32, entries: bool) {
    let arena = engine.arena_mut().unwrap();
    arena[TRANSFER_OFFSET..TRANSFER_OFFSET + 4].copy_from_slice(&pc.to_le_bytes());
    if !entries {
        arena[TRANSFER_OFFSET + 4..TRANSFER_OFFSET + 8].copy_from_slice(&length.to_le_bytes());
    }
}

fn bound(
    engine: &mut EngineInstance,
    resident: bool,
    entries: bool,
    count: u32,
) -> Result<u64, HostError> {
    match (resident, entries) {
        (false, false) => engine.compile(count).map(u64::from),
        (false, true) => engine.compile_entries(count, 0).map(u64::from),
        (true, false) => engine.compile_resident(count).map(|id| id.get()),
        (true, true) => engine.compile_resident_entries(count, 0).map(|id| id.get()),
    }
}

fn artifact(engine: &EngineInstance, resident: bool, id: u64) -> Vec<u8> {
    if resident {
        engine.resident_bytes(id).unwrap().to_vec()
    } else {
        engine.artifact_bytes().unwrap().to_vec()
    }
}

#[test]
fn typed_word_neg_not_capture_all_eight_current_parent_names() {
    let mut count = 0;
    for (kind, base) in [(UnaryKind::Neg, 0xd8), (UnaryKind::Not, 0xd0)] {
        for (alias, destination) in PARENTS.into_iter().enumerate() {
            let bytes = [0x66, 0xf7, base | alias as u8];
            let decoded = decode_one(&memory(0x1000, &bytes), GuestAddress(0x1000)).unwrap();
            assert_eq!(
                decoded.operation(),
                &Operation::UnaryWord { kind, destination }
            );
            assert_eq!(
                (decoded.length(), decoded.next_pc()),
                (3, GuestAddress(0x1003))
            );
            count += 1;
        }
    }
    assert_eq!(count, 16);
}

#[test]
fn six_profiles_validate_pure_word_neg_not_without_native_cpu_execution() {
    let bytes = all_word_unary();
    let code = memory(0x1000, &bytes);
    let mut input = Vec::new();
    for entries in [false, true] {
        let compiled = if entries {
            compile_entry_region(&code, &[GuestAddress(0x1000)], CompileLimits::default()).unwrap()
        } else {
            compile_region(
                &code,
                &[BlockSpec {
                    entry: GuestAddress(0x1000),
                    byte_length: bytes.len() as u32,
                }],
                CompileLimits::default(),
            )
            .unwrap()
        };
        assert_eq!(
            (compiled.metadata().blocks, compiled.metadata().instructions),
            (1, 17)
        );
        let module = compiled.wasm_bytes(&code).unwrap();
        input.push(0);
        input.extend((module.len() as u32).to_le_bytes());
        input.extend(module);
    }
    for resident in [false, true] {
        for entries in [false, true] {
            let mut engine = prepare_engine(0x1000, &bytes);
            describe(&mut engine, 0x1000, bytes.len() as u32, entries);
            let before = engine.arena().to_vec();
            let id = bound(&mut engine, resident, entries, 1).unwrap();
            assert_eq!(engine.arena(), before);
            assert_eq!(engine.memory().unwrap().mapped_pages(), 1);
            assert!(
                engine
                    .memory()
                    .unwrap()
                    .resolve(GuestAddress(0), Access::Read)
                    .is_err()
            );
            let module = artifact(&engine, resident, id);
            input.push(if resident { 2 } else { 1 });
            input.extend((module.len() as u32).to_le_bytes());
            input.extend(module);
        }
    }
    crate::support_wasm_interface::assert_packed(&input, 6, true);
}

#[test]
fn exact_word_neg_not_prefix_fetch_top_and_consumed_source_currency() {
    for bytes in [[0x66, 0xf7, 0xdc], [0x66, 0xf7, 0xd7]] {
        for pc in [0x1000, 0x1fff, u32::MAX - 2] {
            let mut code = memory(pc, &bytes);
            let decoded = decode_one(&code, GuestAddress(pc)).unwrap();
            assert_eq!(
                (decoded.length(), decoded.next_pc()),
                (3, GuestAddress(pc.wrapping_add(3)))
            );
            let range = PageRange::new(
                GuestAddress(pc & !0xfff),
                (u64::from(pc & 0xfff) + 3).div_ceil(4096) as u32,
            )
            .unwrap();
            for index in 0..3 {
                code.protect(range, Permissions::ALL).unwrap();
                let old = decode_one(&code, GuestAddress(pc)).unwrap();
                code.write(GuestAddress(pc + index as u32), &bytes[index..index + 1])
                    .unwrap();
                assert!(!code.is_code_current(old.code_snapshot()));
                code.protect(range, Permissions::EXECUTE).unwrap();
            }
        }
        for cut in 1..3 {
            let pc = 0x2000 - cut;
            let mut code = memory(pc, &bytes[..cut as usize]);
            crate::support_fetch::assert_error(
                decode_one(&code, GuestAddress(pc)).unwrap_err(),
                pc,
                0x2000,
                FaultReason::Unmapped,
                cut + 1,
            );
            code.map_zeroed(
                PageRange::new(GuestAddress(0x2000), 1).unwrap(),
                Permissions::READ,
            )
            .unwrap();
            crate::support_fetch::assert_error(
                decode_one(&code, GuestAddress(pc)).unwrap_err(),
                pc,
                0x2000,
                FaultReason::Permission,
                cut + 1,
            );
        }
        let top = memory(u32::MAX - 2, &bytes);
        let limits = CompileLimits {
            instructions: 1,
            ..CompileLimits::default()
        };
        let explicit = compile_region(
            &top,
            &[BlockSpec {
                entry: GuestAddress(u32::MAX - 2),
                byte_length: 3,
            }],
            limits,
        )
        .unwrap();
        let entries = compile_entry_region(&top, &[GuestAddress(u32::MAX - 2)], limits).unwrap();
        assert_eq!(
            (explicit.metadata().blocks, explicit.metadata().instructions),
            (1, 1)
        );
        assert_eq!(
            explicit.wasm_bytes(&top).unwrap(),
            entries.wasm_bytes(&top).unwrap()
        );
        for prefix in [0x66, 0x67, 0xf2, 0xf3] {
            if matches!(prefix, 0x67) {
                word_prefix::admitted(&[vec![prefix], bytes.to_vec()].concat());
                continue;
            }
            crate::support_fetch::assert_decode_contract(
                decode_one(
                    &memory(0x1000, &[vec![prefix], bytes.to_vec()].concat()),
                    GuestAddress(0x1000),
                )
                .unwrap_err(),
                DecodeError::Unsupported(UnsupportedFeature::Opcode),
            );
        }
        crate::support_fetch::assert_decode_contract(
            decode_one(
                &memory(0x1000, &[vec![0xf0], bytes.to_vec()].concat()),
                GuestAddress(0x1000),
            )
            .unwrap_err(),
            DecodeError::InvalidEncoding,
        );
        for prefix in [0x26, 0x2e, 0x36, 0x3e, 0x64, 0x65] {
            word_prefix::admitted(&[vec![prefix], bytes.to_vec()].concat());
        }
    }
    for bytes in [
        &[0x66, 0x66, 0xf7, 0x18][..],
        &[0x66, 0x66, 0xf7, 0x10][..],
        &[0x66, 0xf6, 0xd8][..],
        &[0x66, 0xf6, 0xd0][..],
        &[0x66, 0x66, 0xd1, 0xe0][..],
    ] {
        if !word_prefix::admit_if_new_prefix(bytes) {
            crate::support_fetch::assert_decode_contract(
                decode_one(&memory(0x1000, bytes), GuestAddress(0x1000)).unwrap_err(),
                DecodeError::Unsupported(UnsupportedFeature::Opcode),
            );
        }
    }
    for bytes in [&[0x66][..], &[0x66, 0xf7][..]] {
        let pc = u32::MAX - bytes.len() as u32 + 1;
        assert!(matches!(
            decode_one(&memory(pc, bytes), GuestAddress(pc)),
            Err(DecodeError::MemoryFault {
                fault: MemoryFault {
                    reason: FaultReason::AddressOverflow,
                    ..
                },
                ..
            })
        ));
    }
}

#[test]
fn caps_and_late_word_neg_not_refusals_preserve_complete_arena_and_both_owners() {
    for nops in [62, 63] {
        let bytes = [vec![0x66, 0xf7, 0xdc], vec![0x90; nops], vec![0xeb, 0]].concat();
        let result = compile_region(
            &memory(0x1000, &bytes),
            &[BlockSpec {
                entry: GuestAddress(0x1000),
                byte_length: bytes.len() as u32,
            }],
            CompileLimits::default(),
        );
        if nops == 62 {
            assert_eq!(result.unwrap().metadata().instructions, 64);
        } else {
            assert!(matches!(result, Err(CompileError::InstructionLimit)));
        }
    }
    let row = [0x66, 0xf7, 0xd8, 0xeb, 0];
    let code = memory(0x1000, &row.repeat(9));
    let specs: Vec<_> = (0..9)
        .map(|index| BlockSpec {
            entry: GuestAddress(0x1000 + index * row.len() as u32),
            byte_length: row.len() as u32,
        })
        .collect();
    assert_eq!(
        compile_region(&code, &specs[..8], CompileLimits::default())
            .unwrap()
            .metadata()
            .blocks,
        8
    );
    assert!(matches!(
        compile_region(&code, &specs, CompileLimits::default()),
        Err(CompileError::InvalidBlocks)
    ));
    assert!(matches!(
        compile_region(
            &code,
            &specs[..1],
            CompileLimits {
                wasm_bytes: 1,
                ..CompileLimits::default()
            }
        ),
        Err(CompileError::WasmLimit)
    ));
    for resident in [false, true] {
        for entries in [false, true] {
            let mut engine = prepare_engine(0x1000, &row);
            for (pc, bytes) in [
                (0x3000, row.to_vec()),
                (0x5000, vec![0x66, 0xf7, 0xdc, 0x90, 0xf0, 0x90, 0xeb, 0]),
            ] {
                engine.map(pc, 1, 7).unwrap();
                upload(&mut engine, pc, &bytes);
                engine.protect(pc, 1, 4).unwrap();
            }
            describe(&mut engine, 0x3000, row.len() as u32, entries);
            let resident_id = bound(&mut engine, true, entries, 1).unwrap();
            describe(&mut engine, 0x1000, row.len() as u32, entries);
            let generation = bound(&mut engine, false, entries, 1).unwrap();
            let old_replacement = artifact(&engine, false, generation);
            let old_resident = artifact(&engine, true, resident_id);
            describe(&mut engine, 0x5000, 8, entries);
            let before = engine.arena().to_vec();
            let error = CompileError::Instruction {
                pc: GuestAddress(0x5004),
                cause: InstructionError::Decode(DecodeError::InvalidEncoding),
            };
            let expected = if resident {
                HostError::Resident(RegistryError::Compile(error))
            } else {
                HostError::Compile(error)
            };
            assert_eq!(
                bound(&mut engine, resident, entries, 1).unwrap_err(),
                expected
            );
            assert_eq!(engine.arena(), before);
            assert_eq!(artifact(&engine, false, generation), old_replacement);
            assert_eq!(artifact(&engine, true, resident_id), old_resident);
            assert_eq!(engine.generation() as u64, generation);
            engine.guard(KEY, generation as u32).unwrap();
            engine.guard_resident(KEY, resident_id).unwrap();
        }
    }
}
