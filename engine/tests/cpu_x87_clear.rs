#[path = "support/prefix_admission.rs"]
mod prefix_admission;
use ring3_engine::{abi::arena::TRANSFER_OFFSET, process::EngineInstance};

#[test]
fn no_wait_fnclex_admits_before_jump() {
    let bytes = [0xdb, 0xe2, 0xeb, 0];
    let mut engine = EngineInstance::new(1, 0x1234_5678_9abc_def0).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(&bytes);
    engine.upload(0x1000, bytes.len() as u32).unwrap();
    let request = &mut engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8];
    request[..4].copy_from_slice(&0x1000_u32.to_le_bytes());
    request[4..].copy_from_slice(&(bytes.len() as u32).to_le_bytes());
    engine.compile(1).expect("bare no-wait FNCLEX must admit");
}

use ring3_engine::{
    cpu::{
        dbt::{BlockSpec, CompileLimits, compile_entry_region, compile_region},
        x86::{
            decode::{DecodeError, decode_one},
            ir::Operation,
        },
    },
    memory::{FaultReason, GuestAddress},
};

const KEY: u64 = 0x1234_5678_9abc_def0;

fn code(pc: u32, bytes: &[u8]) -> EngineInstance {
    let mut engine = EngineInstance::new(3, KEY).unwrap();
    let base = pc & !0xfff;
    let pages = (u64::from(pc - base) + bytes.len() as u64).div_ceil(4096) as u32;
    engine.map(base, pages, 7).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(bytes);
    engine.upload(pc, bytes.len() as u32).unwrap();
    engine.protect(base, pages, 4).unwrap();
    engine
}

fn request(engine: &mut EngineInstance, pc: u32, length: u32) {
    let bytes = &mut engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8];
    bytes[..4].copy_from_slice(&pc.to_le_bytes());
    bytes[4..].copy_from_slice(&length.to_le_bytes());
}

#[test]
fn bare_fnclex_decodes_and_compiles_all_six_profiles() {
    let bytes = [0xdb, 0xe2, 0xeb, 0];
    let engine = code(0x1000, &bytes);
    let memory = engine.memory().unwrap();
    let decoded = decode_one(memory, GuestAddress(0x1000)).unwrap();
    assert_eq!(decoded.operation(), &Operation::ClearX87Exceptions);
    assert_eq!(
        (decoded.length(), decoded.next_pc()),
        (2, GuestAddress(0x1002))
    );
    assert!(memory.is_code_current(decoded.code_snapshot()));
    for unit in [
        compile_region(
            memory,
            &[BlockSpec {
                entry: GuestAddress(0x1000),
                byte_length: bytes.len() as u32,
            }],
            CompileLimits::default(),
        )
        .unwrap(),
        compile_entry_region(memory, &[GuestAddress(0x1000)], CompileLimits::default()).unwrap(),
    ] {
        assert_eq!(unit.metadata().instructions, 2);
        assert_eq!(&unit.wasm_bytes(memory).unwrap()[..8], b"\0asm\x01\0\0\0");
    }
    for resident in [false, true] {
        for entries in [false, true] {
            let mut engine = code(0x1000, &bytes);
            request(
                &mut engine,
                0x1000,
                if entries { 0 } else { bytes.len() as u32 },
            );
            let before = engine.arena().to_vec();
            if resident {
                let id = if entries {
                    engine.compile_resident_entries(1, 0)
                } else {
                    engine.compile_resident(1)
                }
                .unwrap()
                .get();
                engine.guard_resident(KEY, id).unwrap();
                assert_eq!(&engine.resident_bytes(id).unwrap()[..8], b"\0asm\x01\0\0\0");
                for offset in [0, 2] {
                    assert_eq!(engine.lookup_resident(0x1000 + offset).unwrap().get(), id);
                }
            } else {
                let generation = if entries {
                    engine.compile_entries(1, 0)
                } else {
                    engine.compile(1)
                }
                .unwrap();
                engine.guard(KEY, generation).unwrap();
                assert_eq!(&engine.artifact_bytes().unwrap()[..8], b"\0asm\x01\0\0\0");
            }
            assert_eq!(engine.arena(), before);
        }
    }
}

#[test]
fn waiting_stack_arithmetic_and_ignored_prefixes_admit() {
    for bytes in [
        vec![0x9b],
        vec![0x9b, 0xdb, 0xe2],
        vec![0x9b, 0xdb, 0xe3],
        vec![0x9b, 0xdf, 0xe0],
        vec![0x9b, 0xdd, 0x38],
        vec![0xd9, 0xe8],
        vec![0xd9, 0xee],
        vec![0xd9, 0xc9],
        vec![0xdd, 0xd8],
        vec![0xd8, 0xc1],
    ] {
        use ring3_engine::cpu::x86::{X87Binary, X87Operation};
        let x87 = |operation, opcode| Operation::X87 {
            operation,
            address: None,
            waiting: true,
            opcode,
        };
        let expected = match bytes.as_slice() {
            [0x9b, ..] => x87(X87Operation::Wait, 0),
            [0xd9, 0xe8] => x87(X87Operation::LoadConstant { index: 0 }, 0x1e8),
            [0xd9, 0xee] => x87(X87Operation::LoadConstant { index: 6 }, 0x1ee),
            [0xd9, 0xc9] => x87(X87Operation::Exchange { index: 1 }, 0x1c9),
            [0xdd, 0xd8] => x87(
                X87Operation::StoreRegister {
                    index: 0,
                    pop: true,
                },
                0x5d8,
            ),
            [0xd8, 0xc1] => x87(
                X87Operation::Binary {
                    kind: X87Binary::Add,
                    destination: 0,
                    source: 1,
                    pop: false,
                },
                0xc1,
            ),
            _ => unreachable!(),
        };
        let (operation, length) = prefix_admission::admitted(&bytes);
        assert_eq!(operation, expected, "{bytes:02x?}");
        if bytes[0] == 0x9b && bytes.len() > 1 {
            let tail = &bytes[length..];
            let (operation, consumed) = prefix_admission::admitted(tail);
            let expected = match tail {
                [0xdb, 0xe2] => Operation::ClearX87Exceptions,
                [0xdb, 0xe3] => Operation::InitializeX87,
                [0xdf, 0xe0] => Operation::X87StatusToAx,
                [0xd9, 0x38] => Operation::X87ControlToMemory {
                    address: ring3_engine::cpu::x86::ir::EffectiveAddress {
                        base: Some(ring3_engine::cpu::x86::Register32::Eax),
                        ..Default::default()
                    },
                },
                [0xdd, 0x38] => Operation::X87StatusToMemory {
                    address: ring3_engine::cpu::x86::ir::EffectiveAddress {
                        base: Some(ring3_engine::cpu::x86::Register32::Eax),
                        ..Default::default()
                    },
                },
                _ => unreachable!(),
            };
            assert_eq!(operation, expected, "{tail:02x?}");
            assert_eq!(length + consumed, bytes.len());
        } else {
            assert_eq!(length, bytes.len());
        }
    }
    for prefix in [
        0x66, 0x67, 0xf0, 0xf2, 0xf3, 0x26, 0x2e, 0x36, 0x3e, 0x64, 0x65,
    ] {
        let bytes = [prefix, 0xdb, 0xe2];
        if prefix == 0xf0 {
            let engine = code(0x1000, &[prefix, 0xdb, 0xe2]);
            assert!(decode_one(engine.memory().unwrap(), GuestAddress(0x1000)).is_err());
            continue;
        }
        let (operation, length) = prefix_admission::admitted(&bytes);
        assert_eq!(length, 3);
        assert_eq!(operation, Operation::ClearX87Exceptions);
    }
    let engine = code(0x1fff, &[0xdb]);
    assert!(
        matches!(decode_one(engine.memory().unwrap(), GuestAddress(0x1fff)), Err(DecodeError::MemoryFault {fault, length: 2, ..}) if fault.reason == FaultReason::Unmapped && fault.address == GuestAddress(0x2000))
    );
    let engine = code(0x1fff, &[0xdb, 0xe2]);
    assert_eq!(
        decode_one(engine.memory().unwrap(), GuestAddress(0x1fff))
            .unwrap()
            .next_pc(),
        GuestAddress(0x2001)
    );
}

#[test]
fn late_refusal_preserves_existing_publications_and_full_arena() {
    let mut engine = code(0x1000, &[0x90, 0xeb, 0]);
    request(&mut engine, 0x1000, 3);
    let generation = engine.compile(1).unwrap();
    let id = engine.compile_resident(1).unwrap().get();
    engine.map(0x3000, 1, 7).unwrap();
    let bytes = [0xdb, 0xe2, 0xdf, 0xe0, 0x0f, 0x0b];
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(&bytes);
    engine.upload(0x3000, bytes.len() as u32).unwrap();
    for resident in [false, true] {
        for entries in [false, true] {
            request(
                &mut engine,
                0x3000,
                if entries { 0 } else { bytes.len() as u32 },
            );
            let before = (
                engine.arena().to_vec(),
                engine.artifact_bytes().unwrap().to_vec(),
                engine.resident_bytes(id).unwrap().to_vec(),
            );
            let failed = match (resident, entries) {
                (false, false) => engine.compile(1).is_err(),
                (false, true) => engine.compile_entries(1, 0).is_err(),
                (true, false) => engine.compile_resident(1).is_err(),
                (true, true) => engine.compile_resident_entries(1, 0).is_err(),
            };
            assert!(failed);
            assert_eq!(
                (
                    engine.arena().to_vec(),
                    engine.artifact_bytes().unwrap().to_vec(),
                    engine.resident_bytes(id).unwrap().to_vec()
                ),
                before
            );
            assert_eq!(engine.generation(), generation);
            engine.guard(KEY, generation).unwrap();
            engine.guard_resident(KEY, id).unwrap();
        }
    }
}
