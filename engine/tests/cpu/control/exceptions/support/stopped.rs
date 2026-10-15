use crate::support_fetch;

use ring3_engine::{
    abi::{
        arena::{EXIT_OFFSET, TRANSFER_OFFSET},
        x86::{EXIT_SIZE, STATE_SIZE, encode_exit, encode_state_v2},
    },
    cpu::{ExecutionExit, ExitReason, x86::State32},
    process::EngineInstance,
};

pub(super) const KEY: u64 = 0x4d53_5250_5245_5345;

fn upload(engine: &mut EngineInstance, address: u32, bytes: &[u8]) {
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(bytes);
    engine.upload(address, bytes.len() as u32).unwrap();
}

pub(super) fn stopped(bytes: &[u8], entry: u32) -> EngineInstance {
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

pub(super) fn expect_admitted(bytes: &[u8], entry: u32) {
    let mut engine = stopped(bytes, entry);
    let before = engine.arena().to_vec();
    assert_eq!(engine.compile_stopped_entry(KEY, 1), Ok(2), "{bytes:02x?}");
    assert_eq!(engine.generation(), 2);
    assert_eq!(engine.arena(), before);
    assert_eq!(&engine.artifact_bytes().unwrap()[..8], b"\0asm\x01\0\0\0");
}

#[allow(dead_code)]
pub(super) fn expect_captured_fetch(
    engine: &mut EngineInstance,
    entry: u32,
    address: u32,
    present: bool,
    length: u32,
) {
    use ring3_engine::{
        cpu::x86::decode::decode_one,
        memory::{FaultReason, GuestAddress},
    };
    support_fetch::assert_error(
        decode_one(engine.memory().unwrap(), GuestAddress(entry)).unwrap_err(),
        entry,
        address,
        if present {
            FaultReason::Permission
        } else {
            FaultReason::Unmapped
        },
        length,
    );
    let before = engine.arena().to_vec();
    let prior = engine.artifact_bytes().unwrap().to_vec();
    let ram: Vec<_> = [0x1000, 0x3000, 0x4000]
        .into_iter()
        .filter_map(|page| {
            let mut bytes = vec![0; 4096];
            engine
                .memory()
                .unwrap()
                .read(GuestAddress(page), &mut bytes)
                .ok()
                .map(|_| (page, bytes))
        })
        .collect();
    let id = engine.compile_stopped_resident_entry(KEY).unwrap();
    assert_eq!(engine.arena(), before);
    assert_eq!(engine.generation(), 1);
    assert_eq!(engine.artifact_bytes().unwrap(), prior);
    assert_eq!(
        &engine.resident_bytes(id.get()).unwrap()[..8],
        b"\0asm\x01\0\0\0"
    );
    for (page, bytes) in ram {
        let mut after = vec![0; 4096];
        engine
            .memory()
            .unwrap()
            .read(GuestAddress(page), &mut after)
            .unwrap();
        assert_eq!(after, bytes);
    }
}
