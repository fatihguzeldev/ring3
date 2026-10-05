use ring3_engine::{abi::arena::TRANSFER_OFFSET, process::EngineInstance};
use ring3_engine::{
    cpu::{
        UnsupportedFeature,
        dbt::{BlockSpec, CompileLimits, compile_entry_region, compile_region},
        x86::{
            decode::{DecodeError, decode_one},
            ir::{CarryKind, Operation},
        },
    },
    memory::{FaultReason, GuestAddress},
};

const KEY: u64 = 0x1234_5678_9abc_def0;

fn upload(engine: &mut EngineInstance, pc: u32, bytes: &[u8]) {
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(bytes);
    engine.upload(pc, bytes.len() as u32).unwrap();
}

fn code(pc: u32, bytes: &[u8]) -> EngineInstance {
    let mut engine = EngineInstance::new(2, KEY).unwrap();
    let base = pc & !0xfff;
    let pages = (u64::from(pc - base) + bytes.len() as u64).div_ceil(4096) as u32;
    engine.map(base, pages, 7).unwrap();
    upload(&mut engine, pc, bytes);
    engine.protect(base, pages, 4).unwrap();
    engine
}

fn describe(engine: &mut EngineInstance, pc: u32, length: u32, entries: bool) {
    let request = &mut engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8];
    request[..4].copy_from_slice(&pc.to_le_bytes());
    request[4..].copy_from_slice(&if entries { 0 } else { length }.to_le_bytes());
}

#[test]
fn exact_controls_decode_and_compile_through_all_six_profiles() {
    let bytes = [0xf8, 0xf9, 0xf5, 0xeb, 0];
    let engine = code(0x1000, &bytes);
    let memory = engine.memory().unwrap();
    for (offset, kind) in [CarryKind::Clear, CarryKind::Set, CarryKind::Complement]
        .into_iter()
        .enumerate()
    {
        let pc = GuestAddress(0x1000 + offset as u32);
        let decoded = decode_one(memory, pc).unwrap();
        assert_eq!(decoded.operation(), &Operation::Carry { kind });
        assert_eq!(
            (decoded.length(), decoded.next_pc()),
            (1, GuestAddress(pc.0 + 1))
        );
        assert!(memory.is_code_current(decoded.code_snapshot()));
    }
    for unit in [
        compile_region(
            memory,
            &[BlockSpec {
                entry: GuestAddress(0x1000),
                byte_length: 5,
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
            describe(&mut engine, 0x1000, 5, entries);
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
                for offset in 0..4 {
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
fn prefixes_refuse_and_single_byte_fetch_preserves_boundaries() {
    for opcode in [0xf8, 0xf9, 0xf5] {
        for pc in [0x1fff, u32::MAX] {
            let engine = code(pc, &[opcode]);
            let decoded = decode_one(engine.memory().unwrap(), GuestAddress(pc)).unwrap();
            assert_eq!(
                (decoded.length(), decoded.next_pc()),
                (1, GuestAddress(pc.wrapping_add(1)))
            );
        }
        for prefix in [0x66, 0x67, 0xf2, 0xf3, 0x64, 0xf0] {
            let engine = code(0x1000, &[prefix, opcode]);
            let result = decode_one(engine.memory().unwrap(), GuestAddress(0x1000));
            assert!(
                matches!(
                    result,
                    Err(DecodeError::InvalidEncoding
                        | DecodeError::Unsupported(
                            UnsupportedFeature::Opcode | UnsupportedFeature::Segment
                        ))
                ),
                "prefix {prefix:02x} opcode {opcode:02x}"
            );
        }
    }
    let engine = code(0x1fff, &[0x66]);
    assert!(
        matches!(decode_one(engine.memory().unwrap(), GuestAddress(0x1fff)), Err(DecodeError::MemoryFault { fault, length: 2, .. }) if fault.reason == FaultReason::Unmapped && fault.address == GuestAddress(0x2000))
    );
}

#[test]
fn late_refusal_preserves_replacement_and_resident_publications() {
    let mut engine = EngineInstance::new(2, KEY).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    upload(&mut engine, 0x1000, &[0x90, 0xeb, 0]);
    describe(&mut engine, 0x1000, 3, false);
    engine.compile(1).unwrap();
    let keep = engine.compile_resident(1).unwrap().get();
    engine.map(0x3000, 1, 7).unwrap();
    upload(&mut engine, 0x3000, &[0xf8, 0xf9, 0xf5, 0x0f, 0x0b]);
    for resident in [false, true] {
        for entries in [false, true] {
            describe(&mut engine, 0x3000, 5, entries);
            let before = (
                engine.arena().to_vec(),
                engine.generation(),
                engine.artifact_bytes().unwrap().to_vec(),
                engine.resident_bytes(keep).unwrap().to_vec(),
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
                    engine.generation(),
                    engine.artifact_bytes().unwrap().to_vec(),
                    engine.resident_bytes(keep).unwrap().to_vec()
                ),
                before
            );
            engine.guard(KEY, before.1).unwrap();
            engine.guard_resident(KEY, keep).unwrap();
        }
    }
}

#[test]
fn carry_controls_admit_before_jump() {
    let mut engine = EngineInstance::new(1, 0x1234_5678_9abc_def0).unwrap();
    let code = [0xf8, 0xf9, 0xf5, 0xeb, 0];
    engine.map(0x1000, 1, 7).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + code.len()]
        .copy_from_slice(&code);
    engine.upload(0x1000, code.len() as u32).unwrap();
    engine.protect(0x1000, 1, 4).unwrap();
    let request = &mut engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8];
    request[..4].copy_from_slice(&0x1000_u32.to_le_bytes());
    request[4..].copy_from_slice(&(code.len() as u32).to_le_bytes());
    engine
        .compile(1)
        .expect("carry controls must admit before a jump");
}
