use ring3_engine::{abi::arena::TRANSFER_OFFSET, process::EngineInstance};

fn admits_x87_control(opcode: [u8; 2]) {
    let bytes = [opcode[0], opcode[1], 0xeb, 0x00];
    let mut engine = EngineInstance::new(1, 0x1234_5678_9abc_def0).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(&bytes);
    engine.upload(0x1000, bytes.len() as u32).unwrap();
    engine.protect(0x1000, 1, 4).unwrap();
    let request = &mut engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8];
    request[..4].copy_from_slice(&0x1000_u32.to_le_bytes());
    request[4..].copy_from_slice(&(bytes.len() as u32).to_le_bytes());
    engine
        .compile(1)
        .expect("no-wait x87 control must admit via public API");
}

#[test]
fn fninit_admits_public_api() {
    admits_x87_control([0xdb, 0xe3]);
}

#[test]
fn fnstsw_ax_admits_public_api() {
    admits_x87_control([0xdf, 0xe0]);
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
fn exact_no_wait_pair_decodes_and_compiles_all_six_profiles() {
    let bytes = [0xdf, 0xe0, 0xdb, 0xe3, 0xdf, 0xe0, 0xeb, 0];
    let engine = code(0x1000, &bytes);
    let memory = engine.memory().unwrap();
    for (offset, operation) in [
        (0, Operation::X87StatusToAx),
        (2, Operation::InitializeX87),
        (4, Operation::X87StatusToAx),
    ] {
        let pc = GuestAddress(0x1000 + offset);
        let decoded = decode_one(memory, pc).unwrap();
        assert_eq!(decoded.operation(), &operation);
        assert_eq!(
            (decoded.length(), decoded.next_pc()),
            (2, GuestAddress(pc.0 + 2))
        );
        assert!(memory.is_code_current(decoded.code_snapshot()));
    }
    for unit in [
        compile_region(
            memory,
            &[BlockSpec {
                entry: GuestAddress(0x1000),
                byte_length: 8,
            }],
            CompileLimits::default(),
        )
        .unwrap(),
        compile_entry_region(memory, &[GuestAddress(0x1000)], CompileLimits::default()).unwrap(),
    ] {
        assert_eq!(unit.metadata().instructions, 4);
        assert_eq!(&unit.wasm_bytes(memory).unwrap()[..8], b"\0asm\x01\0\0\0");
    }
    for resident in [false, true] {
        for entries in [false, true] {
            let mut engine = code(0x1000, &bytes);
            request(&mut engine, 0x1000, if entries { 0 } else { 8 });
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
                for offset in [0, 2, 4, 6] {
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
fn waiting_stack_memory_arithmetic_and_ignored_prefixes_admit_with_exact_operations() {
    use ring3_engine::cpu::x86::{Register32, X87Binary, X87Operation, ir::EffectiveAddress};
    for (bytes, operation, address, opcode) in [
        (vec![0x9b], X87Operation::Wait, None, 0),
        (
            vec![0xd9, 0xe8],
            X87Operation::LoadConstant { index: 0 },
            None,
            0x1e8,
        ),
        (
            vec![0xd9, 0xee],
            X87Operation::LoadConstant { index: 6 },
            None,
            0x1ee,
        ),
        (
            vec![0xd9, 0xc9],
            X87Operation::Exchange { index: 1 },
            None,
            0x1c9,
        ),
        (
            vec![0xdd, 0xd8],
            X87Operation::StoreRegister {
                index: 0,
                pop: true,
            },
            None,
            0x5d8,
        ),
        (
            vec![0xd8, 0xc1],
            X87Operation::Binary {
                kind: X87Binary::Add,
                destination: 0,
                source: 1,
                pop: false,
            },
            None,
            0x0c1,
        ),
    ] {
        let engine = code(0x1000, &bytes);
        let decoded = decode_one(engine.memory().unwrap(), GuestAddress(0x1000)).unwrap();
        assert_eq!(decoded.length() as usize, bytes.len(), "{bytes:02x?}");
        assert_eq!(
            decoded.operation(),
            &Operation::X87 {
                operation,
                address,
                waiting: true,
                opcode
            },
            "{bytes:02x?}"
        );
    }
    for (bytes, second) in [
        (vec![0x9b, 0xdb, 0xe3], Operation::InitializeX87),
        (vec![0x9b, 0xdf, 0xe0], Operation::X87StatusToAx),
        (
            vec![0x9b, 0xdd, 0x38],
            Operation::X87StatusToMemory {
                address: EffectiveAddress {
                    base: Some(Register32::Eax),
                    ..EffectiveAddress::default()
                },
            },
        ),
    ] {
        let mut complete = bytes.clone();
        complete.extend_from_slice(&[0xeb, 0]);
        let engine = code(0x1000, &complete);
        let memory = engine.memory().unwrap();
        let wait = decode_one(memory, GuestAddress(0x1000)).unwrap();
        assert_eq!(wait.length(), 1);
        assert_eq!(wait.next_pc(), GuestAddress(0x1001));
        assert_eq!(
            wait.operation(),
            &Operation::X87 {
                operation: X87Operation::Wait,
                address: None,
                waiting: true,
                opcode: 0,
            }
        );
        let decoded = decode_one(memory, GuestAddress(0x1001)).unwrap();
        assert_eq!(decoded.length() as usize, bytes.len() - 1);
        assert_eq!(decoded.operation(), &second, "{bytes:02x?}");
        for resident in [false, true] {
            for entries in [false, true] {
                let mut engine = code(0x1000, &complete);
                request(&mut engine, 0x1000, complete.len() as u32);
                let before = engine.arena().to_vec();
                match (resident, entries) {
                    (false, false) => {
                        engine.compile(1).unwrap();
                    }
                    (false, true) => {
                        engine.compile_entries(1, 0).unwrap();
                    }
                    (true, false) => {
                        engine.compile_resident(1).unwrap();
                    }
                    (true, true) => {
                        engine.compile_resident_entries(1, 0).unwrap();
                    }
                }
                assert_eq!(engine.arena(), before);
            }
        }
    }
    for (opcode, operation) in [
        ([0xdb, 0xe3], Operation::InitializeX87),
        ([0xdf, 0xe0], Operation::X87StatusToAx),
    ] {
        for prefix in [0x66, 0x67, 0xf2, 0xf3, 0x64] {
            let engine = code(0x1000, &[prefix, opcode[0], opcode[1]]);
            let decoded = decode_one(engine.memory().unwrap(), GuestAddress(0x1000)).unwrap();
            assert_eq!(
                decoded.operation(),
                &operation,
                "{prefix:02x} {opcode:02x?}"
            );
            assert_eq!(decoded.length(), 3);
        }
        let engine = code(0x1000, &[0xf0, opcode[0], opcode[1]]);
        assert!(decode_one(engine.memory().unwrap(), GuestAddress(0x1000)).is_err());
        let engine = code(0x1fff, &opcode[..1]);
        assert!(
            matches!(decode_one(engine.memory().unwrap(), GuestAddress(0x1fff)), Err(DecodeError::MemoryFault { fault, length: 2, .. }) if fault.reason == FaultReason::Unmapped && fault.address == GuestAddress(0x2000))
        );
        let engine = code(0x1fff, &opcode);
        assert_eq!(
            decode_one(engine.memory().unwrap(), GuestAddress(0x1fff))
                .unwrap()
                .next_pc(),
            GuestAddress(0x2001)
        );
    }
    for bytes in [
        vec![0xdb, 0xe0],
        vec![0xdb, 0xe1],
        vec![0xdb, 0xe4],
        vec![0xd9, 0xd1],
        vec![0x0f, 0x20, 0xc0],
        vec![0x0f, 0x0b],
    ] {
        let engine = code(0x1000, &bytes);
        assert!(
            decode_one(engine.memory().unwrap(), GuestAddress(0x1000)).is_err(),
            "{bytes:02x?}"
        );
    }
}

#[test]
fn late_refusal_preserves_both_existing_publications_and_fpu_state() {
    let mut engine = code(0x1000, &[0x90, 0xeb, 0]);
    request(&mut engine, 0x1000, 3);
    let generation = engine.compile(1).unwrap();
    let id = engine.compile_resident(1).unwrap().get();
    engine.map(0x3000, 1, 7).unwrap();
    let bytes = [0xdb, 0xe3, 0xdf, 0xe0, 0x0f, 0x0b];
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
