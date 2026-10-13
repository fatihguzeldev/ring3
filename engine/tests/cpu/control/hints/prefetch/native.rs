use ring3_engine::{
    abi::{
        arena::{EXIT_OFFSET, TRANSFER_OFFSET},
        x86::{EXIT_SIZE, STATE_SIZE, encode_exit, encode_state_v2},
    },
    cpu::{
        ExecutionExit, ExitReason,
        dbt::{BlockSpec, CompileLimits, compile_entry_region, compile_region},
        x86::{
            State32,
            decode::{DecodeError, decode_one},
            ir::Operation,
        },
    },
    memory::{Access, FaultReason, GuestAddress},
    process::EngineInstance,
};

const KEY: u64 = 0x4f50_5449_4f4e_414c;

fn upload(engine: &mut EngineInstance, address: u32, bytes: &[u8]) {
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(bytes);
    engine.upload(address, bytes.len() as u32).unwrap();
}

fn stopped(bytes: &[u8], entry: u32) -> EngineInstance {
    let mut engine = EngineInstance::new(2, KEY).unwrap();
    for page in [0x1000, 0x3000] {
        engine.map(page, 1, 7).unwrap();
    }
    let mut jump = vec![0xe9];
    jump.extend((entry - 0x1005).to_le_bytes());
    upload(&mut engine, 0x1000, &jump);
    upload(&mut engine, entry, bytes);
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8]
        .copy_from_slice(&[0x1000_u32.to_le_bytes(), 5_u32.to_le_bytes()].concat());
    engine.compile(1).unwrap();
    // this public capability probe constructs a canonical stop; it does not execute the bootstrap.
    let arena = engine.arena_mut().unwrap();
    encode_state_v2(
        &State32 {
            registers: [u32::MAX; 8],
            eip: entry,
            eflags: 0x244fd7,
        },
        &mut arena[..STATE_SIZE],
    )
    .unwrap();
    arena[12..16].copy_from_slice(&1_u32.to_le_bytes());
    encode_exit(
        &ExecutionExit {
            retired: 1,
            reason: ExitReason::NeedCode,
        },
        &mut arena[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
    )
    .unwrap();
    engine
}

fn ram(engine: &EngineInstance) -> Vec<u8> {
    let mut bytes = vec![0; 8192];
    for (index, page) in [0x1000, 0x3000].into_iter().enumerate() {
        engine
            .memory()
            .unwrap()
            .read(
                GuestAddress(page),
                &mut bytes[index * 4096..(index + 1) * 4096],
            )
            .unwrap();
    }
    bytes
}

fn admitted(selector: u8) {
    for prefix in [
        vec![],
        vec![0x66],
        vec![0x67],
        vec![0x64],
        vec![0x36],
        vec![0x2e],
        vec![0x66, 0x67],
    ] {
        let bytes = [prefix, vec![0x0f, 0x0d, selector]].concat();
        let engine = stopped(&bytes, 0x3000);
        let before = engine.arena().to_vec();
        let before_ram = ram(&engine);
        let instruction = decode_one(engine.memory().unwrap(), GuestAddress(0x3000))
            .expect("explicit supported data-prefetch hint");
        assert_eq!(instruction.operation(), &Operation::Nop);
        assert_eq!(instruction.length() as usize, bytes.len());
        assert_eq!(
            instruction.next_pc(),
            GuestAddress(0x3000 + bytes.len() as u32)
        );
        assert!(
            engine
                .memory()
                .unwrap()
                .is_code_current(instruction.code_snapshot())
        );
        assert_eq!(engine.arena(), before);
        assert_eq!(ram(&engine), before_ram);
    }
}

#[test]
fn supported_prefetch_memory_hint_is_ignored_without_losing_prefix_length() {
    admitted(0x00);
}

#[test]
fn supported_prefetchw_memory_hint_is_ignored_without_losing_prefix_length() {
    admitted(0x08);
}

#[test]
fn both_supported_hints_compile_in_all_public_bound_and_standalone_profiles() {
    let bytes = [0x0f, 0x0d, 0x00, 0x66, 0x67, 0x0f, 0x0d, 0x08, 0xeb, 0xfe];
    let engine = stopped(&bytes, 0x3000);
    let memory = engine.memory().unwrap();
    for entries in [false, true] {
        let compiled = if entries {
            compile_entry_region(memory, &[GuestAddress(0x3000)], CompileLimits::default())
        } else {
            compile_region(
                memory,
                &[BlockSpec {
                    entry: GuestAddress(0x3000),
                    byte_length: bytes.len() as u32,
                }],
                CompileLimits::default(),
            )
        }
        .unwrap();
        assert_eq!(compiled.metadata().instructions, 3);
    }
    for resident in [false, true] {
        for entries in [false, true] {
            let mut engine = stopped(&bytes, 0x3000);
            let descriptor = if entries {
                vec![0x3000_u32]
            } else {
                vec![0x3000, bytes.len() as u32]
            };
            let encoded: Vec<u8> = descriptor
                .iter()
                .flat_map(|value| value.to_le_bytes())
                .collect();
            engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + encoded.len()]
                .copy_from_slice(&encoded);
            let before = engine.arena().to_vec();
            let before_ram = ram(&engine);
            match (resident, entries) {
                (false, false) => engine.compile(1).map(|_| ()),
                (false, true) => engine.compile_entries(1, 0).map(|_| ()),
                (true, false) => engine.compile_resident(1).map(|_| ()),
                (true, true) => engine.compile_resident_entries(1, 0).map(|_| ()),
            }
            .expect("exact supported hints are ordinary bound instructions");
            assert_eq!(engine.arena(), before);
            assert_eq!(ram(&engine), before_ram);
        }
    }
}

#[test]
fn unsupported_neighbors_invalid_lock_prefix_refusals_and_full_fetch_stay_separate() {
    for bytes in [
        &[0x0f, 0x0d, 0x10][..],
        &[0x0f, 0x0d, 0xc0],
        &[0x0f, 0x0d, 0xc8],
        &[0xf2, 0x0f, 0x0d, 0x08],
        &[0xf3, 0x0f, 0x0d, 0x00],
        &[0x66, 0x66, 0x0f, 0x0d, 0x08],
    ] {
        let mut engine = stopped(bytes, 0x3000);
        let before = engine.arena().to_vec();
        let before_ram = ram(&engine);
        assert!(
            decode_one(engine.memory().unwrap(), GuestAddress(0x3000)).is_err(),
            "{bytes:02x?}"
        );
        assert!(
            engine.compile_stopped_entry(KEY, 1).is_err(),
            "{bytes:02x?}"
        );
        assert_eq!(engine.arena(), before);
        assert_eq!(ram(&engine), before_ram);
    }
    for selector in [0x00, 0x08] {
        let mut engine = stopped(&[0xf0, 0x0f, 0x0d, selector], 0x3000);
        assert_eq!(
            decode_one(engine.memory().unwrap(), GuestAddress(0x3000)).err(),
            Some(DecodeError::InvalidEncoding)
        );
        assert_eq!(engine.compile_stopped_entry(KEY, 1), Ok(2));
        let bytes = [0x0f, 0x0d, selector | 5, 0, 0, 0];
        let pc = 0x4000 - bytes.len() as u32;
        let mut engine = stopped(&bytes, pc);
        let before = engine.arena().to_vec();
        let before_ram = ram(&engine);
        assert!(
            matches!(decode_one(engine.memory().unwrap(),GuestAddress(pc)),Err(DecodeError::MemoryFault{fault,..}) if fault.address==GuestAddress(0x4000)&&fault.access==Access::Execute&&fault.reason==FaultReason::Unmapped)
        );
        assert!(engine.compile_stopped_entry(KEY, 1).is_err());
        assert_eq!(engine.arena(), before);
        assert_eq!(ram(&engine), before_ram);
    }
}
