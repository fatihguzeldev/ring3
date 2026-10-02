use ring3_engine::abi::x86::{encode_exit_v3, encode_state};
use ring3_engine::cpu::{ExecutionExit, ExitReason, x86::State32};
use ring3_engine::memory::GuestAddress;
use ring3_engine::process::{EngineInstance, HostError};
use ring3_engine::windows::CallingConvention32;

const KEY: u64 = 0x1234_5678_9abc_def0;

fn upload(engine: &mut EngineInstance, address: u32, bytes: &[u8]) {
    engine.arena_mut().unwrap()[140..140 + bytes.len()].copy_from_slice(bytes);
    engine.upload(address, bytes.len() as u32).unwrap();
}

fn helper(engine: &EngineInstance) -> [u32; 6] {
    let bytes = &engine.arena()[100..140];
    assert_eq!(
        &bytes[..16],
        &[82, 51, 77, 72, 2, 0, 1, 0, 40, 0, 0, 0, 0, 0, 0, 0]
    );
    std::array::from_fn(|index| {
        u32::from_le_bytes(bytes[16 + index * 4..20 + index * 4].try_into().unwrap())
    })
}

fn read(engine: &mut EngineInstance, address: u32, width: u32) {
    let before = engine.arena().to_vec();
    let result = if width == 1 {
        engine.read8(address)
    } else {
        engine.read16(address)
    };
    assert_eq!(result, Ok(()));
    assert_eq!(&engine.arena()[..100], &before[..100]);
    assert_eq!(&engine.arena()[140..], &before[140..]);
    assert_eq!(helper(engine)[5], width);
}

#[test]
fn unaligned_page_final_and_address_final_reads_do_not_overfetch() {
    let mut engine = EngineInstance::new(2, KEY).unwrap();
    engine.map(0x8000, 1, 7).unwrap();
    engine.map(0xffff_f000, 1, 7).unwrap();
    upload(&mut engine, 0x8ffd, &[0x21, 0xef, 0x80]);
    upload(&mut engine, u32::MAX - 1, &[0xff, 0x80]);
    let page_snapshot = engine
        .memory()
        .unwrap()
        .snapshot_code(GuestAddress(0x8ffd), 3)
        .unwrap();
    let final_snapshot = engine
        .memory()
        .unwrap()
        .snapshot_code(GuestAddress(u32::MAX - 1), 2)
        .unwrap();
    for (address, width, value) in [
        (0x8ffd, 1, 0x21),
        (0x8ffd, 2, 0xef21),
        (0x8ffe, 2, 0x80ef),
        (0x8fff, 1, 0x80),
        (u32::MAX - 1, 2, 0x80ff),
        (u32::MAX, 1, 0x80),
    ] {
        read(&mut engine, address, width);
        assert_eq!(helper(&engine), [0, value, 0, 0, 0, width]);
        assert!(engine.memory().unwrap().is_code_current(&page_snapshot));
        assert!(engine.memory().unwrap().is_code_current(&final_snapshot));
    }
    assert_eq!(engine.memory().unwrap().mapped_pages(), 2);
    assert_eq!(engine.generation(), 0);
}

#[test]
fn crossing_word_faults_publish_first_inaccessible_byte_and_allow_exact_retry() {
    let mut engine = EngineInstance::new(2, KEY).unwrap();
    engine.map(0x8000, 1, 7).unwrap();
    upload(&mut engine, 0x8fff, &[0x7f]);
    read(&mut engine, 0x8fff, 1);
    assert_eq!(helper(&engine), [0, 0x7f, 0, 0, 0, 1]);
    read(&mut engine, 0x8fff, 2);
    assert_eq!(helper(&engine), [1, 0, 1, 0x9000, 1, 2]);
    engine.map(0x9000, 1, 7).unwrap();
    upload(&mut engine, 0x9000, &[0x80]);
    read(&mut engine, 0x8fff, 2);
    assert_eq!(helper(&engine), [0, 0x807f, 0, 0, 0, 2]);
    engine.protect(0x9000, 1, 4).unwrap();
    let snapshot = engine
        .memory()
        .unwrap()
        .snapshot_code(GuestAddress(0x8fff), 2)
        .unwrap();
    read(&mut engine, 0x8fff, 2);
    assert_eq!(helper(&engine), [1, 0, 2, 0x9000, 1, 2]);
    read(&mut engine, 0x8ffe, 2);
    assert_eq!(helper(&engine), [0, 0x7f00, 0, 0, 0, 2]);
    assert!(engine.memory().unwrap().is_code_current(&snapshot));
    let mut bytes = [0; 2];
    engine
        .memory()
        .unwrap()
        .fetch(GuestAddress(0x8fff), &mut bytes)
        .unwrap();
    assert_eq!(bytes, [0x7f, 0x80]);
}

#[test]
fn word_overflow_replaces_poisoned_success_without_wrapping_and_byte_remains_valid() {
    let mut engine = EngineInstance::new(1, KEY).unwrap();
    engine.map(0xffff_f000, 1, 7).unwrap();
    upload(&mut engine, u32::MAX - 1, &[0xff, 0x80]);
    read(&mut engine, u32::MAX - 1, 2);
    assert_eq!(helper(&engine), [0, 0x80ff, 0, 0, 0, 2]);
    read(&mut engine, u32::MAX, 2);
    assert_eq!(helper(&engine), [1, 0, 3, u32::MAX, 1, 2]);
    read(&mut engine, u32::MAX, 1);
    assert_eq!(helper(&engine), [0, 0x80, 0, 0, 0, 1]);
    read(&mut engine, 0, 1);
    assert_eq!(helper(&engine), [1, 0, 1, 0, 1, 1]);
}

#[test]
fn write_only_and_execute_only_data_reject_read_without_touching_bytes_or_cpu_arena() {
    for bits in [2, 4] {
        let mut engine = EngineInstance::new(1, KEY).unwrap();
        engine.map(0x8000, 1, 7).unwrap();
        upload(&mut engine, 0x8000, &[0x34, 0x12]);
        engine.arena_mut().unwrap()[..100].fill(0x5a);
        engine.protect(0x8000, 1, bits).unwrap();
        for width in [1, 2] {
            read(&mut engine, 0x8000, width);
            assert_eq!(helper(&engine), [1, 0, 2, 0x8000, 1, width]);
        }
        engine.protect(0x8000, 1, 3).unwrap();
        let mut bytes = [0; 2];
        engine
            .memory()
            .unwrap()
            .read(GuestAddress(0x8000), &mut bytes)
            .unwrap();
        assert_eq!(bytes, [0x34, 0x12]);
    }
}

#[test]
fn narrow_helpers_remain_lifecycle_usable_without_artifact_or_after_code_invalidation() {
    let mut engine = EngineInstance::new(2, KEY).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    engine.map(0x8000, 1, 3).unwrap();
    upload(&mut engine, 0x1000, &[0x90]);
    upload(&mut engine, 0x8000, &[0x34, 0x12]);
    read(&mut engine, 0x8000, 1);
    assert_eq!(helper(&engine), [0, 0x34, 0, 0, 0, 1]);
    assert_eq!(engine.generation(), 0);
    engine.arena_mut().unwrap()[140..148].copy_from_slice(&[0, 0x10, 0, 0, 1, 0, 0, 0]);
    assert_eq!(engine.compile(1), Ok(1));
    let snapshot = engine
        .memory()
        .unwrap()
        .snapshot_code(GuestAddress(0x1000), 1)
        .unwrap();
    read(&mut engine, 0x8000, 2);
    assert!(engine.memory().unwrap().is_code_current(&snapshot));
    assert_eq!(engine.guard(KEY, 1), Ok(()));
    upload(&mut engine, 0x1000, &[0x90]);
    assert_eq!(engine.guard(KEY, 1), Err(HostError::CodeInvalidated));
    read(&mut engine, 0x8000, 2);
    assert_eq!(helper(&engine), [0, 0x1234, 0, 0, 0, 2]);
    assert_eq!(engine.generation(), 1);
    assert_eq!(engine.artifact_bytes(), Err(HostError::CodeInvalidated));
    engine.close();
    let arena = engine.arena().to_vec();
    assert_eq!(engine.read8(0x8000), Err(HostError::Closed));
    assert_eq!(engine.read16(u32::MAX), Err(HostError::Closed));
    assert_eq!(engine.arena(), arena);
}

#[test]
fn reads_during_native_injected_pending_call_preserve_private_completion_authority() {
    let mut engine = EngineInstance::new(3, KEY).unwrap();
    for address in [0x1000, 0x2000, 0x8000] {
        engine.map(address, 1, 7).unwrap();
    }
    upload(&mut engine, 0x1000, &[0x0f, 0x0b]);
    upload(&mut engine, 0x2000, &[0x90]);
    upload(&mut engine, 0x8000, &[0, 0x20, 0, 0, 0x34, 0x12, 0, 0]);
    engine.arena_mut().unwrap()[140..164].copy_from_slice(&[
        0, 0x10, 0, 0, 2, 0, 0, 0, 0, 0x20, 0, 0, 1, 0, 0, 0, 0, 0x10, 0, 0, 7, 0, 0, 0,
    ]);
    assert_eq!(engine.compile_with_gates(2, 1), Ok(1));
    // native canonical stop injection; actual execution is covered separately.
    let state = State32 {
        registers: [1, 2, 3, 4, 0x8000, 6, 7, 8],
        eip: 0x1000,
        eflags: 0xcd7,
    };
    encode_state(&state, &mut engine.arena_mut().unwrap()[..56]).unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 3,
            reason: ExitReason::Gate { id: 7 },
        },
        &mut engine.arena_mut().unwrap()[56..96],
    )
    .unwrap();
    let record = engine
        .capture_call(KEY, 1, CallingConvention32::Cdecl, 1)
        .unwrap();
    read(&mut engine, 0x8004, 1);
    assert_eq!(helper(&engine), [0, 0x34, 0, 0, 0, 1]);
    read(&mut engine, 0x8004, 2);
    assert_eq!(helper(&engine), [0, 0x1234, 0, 0, 0, 2]);
    engine
        .complete_call(KEY, 1, record.token, 0x5566_7788)
        .unwrap();
    let state = ring3_engine::abi::x86::decode_state(&engine.arena()[..56]).unwrap();
    assert_eq!(state.registers, [0x5566_7788, 2, 3, 4, 0x8004, 6, 7, 8]);
    assert_eq!(state.eip, 0x2000);
    assert_eq!(state.eflags, 0xcd7);
}
