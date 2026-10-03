use super::*;
use crate::{
    abi::x86::{encode_exit_v3, encode_state},
    cpu::{ExecutionExit, x86::State32},
    memory::GuestAddress,
    windows::CallingConvention32,
};

const KEY: u64 = 91;

fn descriptors(engine: &mut EngineInstance, replacement: bool) {
    let target = if replacement { 0x4000 } else { 0x1100 };
    let pairs = [
        (0x1000_u32, 2_u32),
        (target, 1),
        (0x1200, 2),
        (0x1300, 2),
        (0x1000, 17),
        (0x1200, 18),
        (0x1300, 19),
    ];
    for (index, (address, value)) in pairs.into_iter().enumerate() {
        let start = TRANSFER_OFFSET + index * 8;
        engine.arena_mut().unwrap()[start..start + 4].copy_from_slice(&address.to_le_bytes());
        engine.arena_mut().unwrap()[start + 4..start + 8].copy_from_slice(&value.to_le_bytes());
    }
}

fn stop(engine: &mut EngineInstance, pc: u32, reason: ExitReason) {
    let mut state = decode_state(&engine.arena()[..STATE_SIZE]).unwrap();
    state.eip = pc;
    encode_state(&state, &mut engine.arena_mut().unwrap()[..STATE_SIZE]).unwrap();
    encode_exit_v3(
        &ExecutionExit { retired: 7, reason },
        &mut engine.arena_mut().unwrap()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
    )
    .unwrap();
}

fn fixture(previous_generation: u32) -> (EngineInstance, u32, u32) {
    let mut engine = EngineInstance::new(3, KEY).unwrap();
    for (address, permissions) in [(0x1000, 7), (0x4000, 7), (0x8000, 3)] {
        engine.map(address, 1, permissions).unwrap();
    }
    for (address, bytes) in [
        (0x1000, &[0x0f, 0x0b][..]),
        (0x1100, &[0x90][..]),
        (0x1200, &[0x0f, 0x0b][..]),
        (0x1300, &[0x0f, 0x0b][..]),
        (0x4000, &[0x90][..]),
    ] {
        engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
            .copy_from_slice(bytes);
        engine.upload(address, bytes.len() as u32).unwrap();
    }
    // seed before compilation so even the last-generation test has a real matching binding.
    engine.generation = previous_generation;
    descriptors(&mut engine, false);
    let generation = engine.compile_with_gates(4, 3).unwrap();
    engine.write32(0x8010, 0x5000).unwrap();
    let mut state = State32 {
        eip: 0x1000,
        ..State32::default()
    };
    state.registers[4] = 0x8010;
    encode_state(&state, &mut engine.arena_mut().unwrap()[..STATE_SIZE]).unwrap();
    stop(&mut engine, 0x1000, ExitReason::Gate { id: 17 });
    let outer = engine
        .capture_call(KEY, generation, CallingConvention32::Cdecl, 0)
        .unwrap();
    assert_eq!(outer.token, 1);
    let callback = engine
        .begin_callback(KEY, generation, outer.token, 0x1100, 0x1200, 18, &[])
        .unwrap();
    assert_eq!(callback.token, 2);
    stop(&mut engine, 0x4000, ExitReason::NeedCode);
    descriptors(&mut engine, true);
    (engine, generation, callback.token)
}

fn authority(engine: &EngineInstance) -> (u32, u32, PendingOwner, u32, u32) {
    let callback = engine.callback.as_ref().unwrap();
    let record = callback.replacement_record().unwrap();
    (
        engine.generation,
        record.generation,
        callback.outer.owner,
        record.token,
        engine.call_token,
    )
}

#[test]
fn final_install_rejects_changed_cpu_bytes_without_publication() {
    for offset in [32, EXIT_OFFSET + 20] {
        let (mut engine, generation, token) = fixture(0);
        let prepared = engine
            .prepare_callback_code(KEY, generation, token, 4, 3)
            .unwrap();
        engine.arena_mut().unwrap()[offset] ^= 1;
        let arena = engine.arena().to_vec();
        let artifact = engine.artifact_bytes().unwrap().to_vec();
        let before = authority(&engine);
        assert_eq!(
            engine.install_callback_code(prepared),
            Err(HostError::Call(CallError::StateChanged))
        );
        assert_eq!(engine.arena(), arena);
        assert_eq!(engine.artifact_bytes().unwrap(), artifact);
        assert_eq!(authority(&engine), before);
        assert!(engine.pending_call.is_none());
    }
}

#[test]
fn final_install_rejects_new_snapshot_even_when_old_region_is_current() {
    let (mut engine, generation, token) = fixture(0);
    let prepared = engine
        .prepare_callback_code(KEY, generation, token, 4, 3)
        .unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x4000), &[0x90])
        .unwrap();
    let arena = engine.arena().to_vec();
    let artifact = engine.artifact_bytes().unwrap().to_vec();
    let before = authority(&engine);
    let new_snapshot = engine
        .memory()
        .unwrap()
        .snapshot_code(GuestAddress(0x4000), 1)
        .unwrap();
    assert_eq!(
        engine.install_callback_code(prepared),
        Err(HostError::CodeInvalidated)
    );
    assert_eq!(engine.arena(), arena);
    assert_eq!(engine.artifact_bytes().unwrap(), artifact);
    assert_eq!(authority(&engine), before);
    assert!(engine.memory().unwrap().is_code_current(&new_snapshot));
}

#[test]
fn final_install_rejects_aborted_token_without_consuming_outer_authority() {
    let (mut engine, generation, token) = fixture(0);
    let prepared = engine
        .prepare_callback_code(KEY, generation, token, 4, 3)
        .unwrap();
    engine.abort_callback(KEY, token).unwrap();
    let arena = engine.arena().to_vec();
    let artifact = engine.artifact_bytes().unwrap().to_vec();
    assert_eq!(
        engine.install_callback_code(prepared),
        Err(HostError::Call(CallError::InvalidToken))
    );
    assert_eq!(engine.arena(), arena);
    assert_eq!(engine.artifact_bytes().unwrap(), artifact);
    assert!(engine.callback.is_none());
    assert_eq!(engine.pending_call.as_ref().unwrap().token, 1);
    assert_eq!(
        engine.pending_call.as_ref().unwrap().owner,
        PendingOwner::Replacement(generation)
    );
    assert_eq!(engine.call_token, token);
    engine.complete_call(KEY, generation, 1, 55).unwrap();
}

#[test]
fn final_install_rejects_close_without_restoring_any_authority() {
    let (mut engine, generation, token) = fixture(0);
    let prepared = engine
        .prepare_callback_code(KEY, generation, token, 4, 3)
        .unwrap();
    engine.close();
    let arena = engine.arena().to_vec();
    assert_eq!(
        engine.install_callback_code(prepared),
        Err(HostError::Closed)
    );
    assert_eq!(engine.arena(), arena);
    assert!(engine.callback.is_none());
    assert!(engine.pending_call.is_none());
    assert!(engine.artifact.is_none());
    assert!(!engine.is_open());
    assert_eq!(engine.call_token, token);
}

#[test]
fn last_artifact_generation_migrates_once_and_exhaustion_is_atomic() {
    let (mut engine, generation, token) = fixture(u32::MAX - 2);
    assert_eq!(generation, u32::MAX - 1);
    let cpu = engine.arena()[..96].to_vec();
    assert_eq!(
        engine.resume_callback_code(KEY, generation, token, 4, 3),
        Ok(u32::MAX)
    );
    assert_eq!(engine.arena()[..96], cpu);
    assert_eq!(
        authority(&engine),
        (
            u32::MAX,
            u32::MAX,
            PendingOwner::Replacement(u32::MAX),
            2,
            2
        )
    );
    let arena = engine.arena().to_vec();
    let artifact = engine.artifact_bytes().unwrap().to_vec();
    assert_eq!(
        engine.resume_callback_code(KEY, u32::MAX, token, 4, 3),
        Err(HostError::GenerationExhausted)
    );
    assert_eq!(engine.arena(), arena);
    assert_eq!(engine.artifact_bytes().unwrap(), artifact);
    assert_eq!(
        authority(&engine),
        (
            u32::MAX,
            u32::MAX,
            PendingOwner::Replacement(u32::MAX),
            2,
            2
        )
    );
    engine.abort_callback(KEY, token).unwrap();
    assert_eq!(
        engine.pending_call.as_ref().unwrap().owner,
        PendingOwner::Replacement(u32::MAX)
    );
    engine.complete_call(KEY, u32::MAX, 1, 77).unwrap();
    assert_eq!(engine.call_token, 2);
}
