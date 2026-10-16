use crate::{
    memory::GuestAddress,
    process::{EngineInstance, HostError},
    windows::synchronization::{
        SynchronizationApi32 as Api, SynchronizationError32 as Error, WaitOutcome32,
    },
};
const CS: u32 = 0x10000;
fn engine() -> EngineInstance {
    let mut engine = EngineInstance::new(32, 31).unwrap();
    engine.configure_cpu_clock(1_000_000_000, 0).unwrap();
    engine.map(CS, 1, 3).unwrap();
    engine
        .configure_synchronization(Default::default())
        .unwrap();
    engine
}
fn read(engine: &EngineInstance, offset: u32) -> u32 {
    let mut bytes = [0; 4];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(CS + offset), &mut bytes)
        .unwrap();
    u32::from_le_bytes(bytes)
}
fn value(outcome: WaitOutcome32) -> u32 {
    let WaitOutcome32::Ready(value) = outcome else {
        panic!()
    };
    value.value
}
#[test]
fn critical_section_has_real_guest_debug_info_recursive_ownership_and_reusable_lifetime() {
    let mut engine = engine();
    assert_eq!(
        value(
            engine
                .execute_critical_call(Api::InitializeCriticalSection, &[CS])
                .unwrap()
        ),
        0
    );
    let debug = read(&engine, 0);
    assert_ne!(debug, 0);
    assert_ne!(debug, u32::MAX);
    let mut bytes = [0; 32];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(debug), &mut bytes)
        .unwrap();
    assert_eq!(u32::from_le_bytes(bytes[4..8].try_into().unwrap()), CS);
    for _ in 0..2 {
        engine
            .execute_critical_call(Api::EnterCriticalSection, &[CS])
            .unwrap();
    }
    assert_eq!(read(&engine, 8), 2);
    assert_eq!(read(&engine, 12), 1);
    engine
        .execute_critical_call(Api::LeaveCriticalSection, &[CS])
        .unwrap();
    assert_eq!(read(&engine, 8), 1);
    assert!(matches!(
        engine.execute_critical_call(Api::DeleteCriticalSection, &[CS]),
        Err(HostError::Synchronization(Error::Busy))
    ));
    engine
        .execute_critical_call(Api::LeaveCriticalSection, &[CS])
        .unwrap();
    assert_eq!(read(&engine, 4), u32::MAX);
    assert_eq!(read(&engine, 12), 0);
    engine
        .execute_critical_call(Api::DeleteCriticalSection, &[CS])
        .unwrap();
    assert!(engine.check_windows_runtime_bytes(debug, 32).is_err());
    engine
        .execute_critical_call(Api::InitializeCriticalSectionEx, &[CS, 1234, 0x0100_0000])
        .unwrap();
    assert_eq!(read(&engine, 0), u32::MAX);
    assert_eq!(read(&engine, 20), 0);
}
#[test]
fn contended_critical_section_waits_in_shared_sync_and_failed_try_preserves_owner() {
    let mut engine = engine();
    engine
        .execute_critical_call(Api::InitializeCriticalSectionEx, &[CS, 0, 0x0100_0000])
        .unwrap();
    engine
        .execute_critical_call(Api::EnterCriticalSection, &[CS])
        .unwrap();
    engine
        .synchronization_mut()
        .unwrap()
        .set_current_thread(2)
        .unwrap();
    let revision = engine.synchronization().unwrap().revision();
    let memory_version = engine.memory().unwrap().transaction_version();
    assert_eq!(
        value(
            engine
                .execute_critical_call(Api::TryEnterCriticalSection, &[CS])
                .unwrap()
        ),
        0
    );
    assert_eq!(engine.synchronization().unwrap().revision(), revision);
    assert_eq!(
        engine.memory().unwrap().transaction_version(),
        memory_version
    );
    assert_eq!(read(&engine, 12), 1);
    let WaitOutcome32::Pending(wait) = engine
        .execute_critical_call(Api::EnterCriticalSection, &[CS])
        .unwrap()
    else {
        panic!()
    };
    engine
        .synchronization_mut()
        .unwrap()
        .set_current_thread(1)
        .unwrap();
    engine
        .execute_critical_call(Api::LeaveCriticalSection, &[CS])
        .unwrap();
    assert_eq!(read(&engine, 12), 2);
    assert!(
        engine
            .synchronization()
            .unwrap()
            .wait_status(wait)
            .unwrap()
            .is_some()
    );
    engine
        .synchronization_mut()
        .unwrap()
        .set_current_thread(2)
        .unwrap();
    engine
        .execute_critical_call(Api::LeaveCriticalSection, &[CS])
        .unwrap();
    assert_eq!(read(&engine, 12), 0);
}
#[test]
fn critical_initialization_faults_and_remapped_structs_do_not_publish_or_reuse_authority() {
    let mut engine = engine();
    let revision = engine.synchronization().unwrap().revision();
    assert!(
        engine
            .execute_critical_call(Api::InitializeCriticalSection, &[0x5555])
            .is_err()
    );
    assert_eq!(engine.synchronization().unwrap().revision(), revision);
    assert!(engine.process_heap().is_none());
    engine
        .execute_critical_call(Api::InitializeCriticalSectionEx, &[CS, 0, 0x0100_0000])
        .unwrap();
    engine.unmap(CS, 1).unwrap();
    engine.map(CS, 1, 3).unwrap();
    let revision = engine.synchronization().unwrap().revision();
    assert!(matches!(
        engine.execute_critical_call(Api::TryEnterCriticalSection, &[CS]),
        Err(HostError::Synchronization(Error::Stale))
    ));
    assert_eq!(engine.synchronization().unwrap().revision(), revision);
}

#[test]
fn captured_force_debug_critical_initialization_supplies_writable_crt_spare_and_owned_cleanup() {
    use crate::{
        abi::{arena::*, x86::*},
        cpu::{ExecutionExit, ExitReason, x86::State32},
        windows::CallingConvention32,
    };

    let mut engine = engine();
    const CODE: u32 = 0x20000;
    const STACK: u32 = CS + 0x800;
    engine.map(CODE, 1, 7).unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(CODE), &[0x0f, 0x0b])
        .unwrap();
    engine.protect(CODE, 1, 5).unwrap();
    let api = Api::InitializeCriticalSectionEx;
    for (index, word) in [CODE, 2, CODE, api.id()].into_iter().enumerate() {
        engine.arena_mut().unwrap()[TRANSFER_OFFSET + index * 4..TRANSFER_OFFSET + index * 4 + 4]
            .copy_from_slice(&word.to_le_bytes());
    }
    engine.compile_with_gates(1, 1).unwrap();
    for (index, word) in [CODE + 2, CS, 0, 0x1000_0000].into_iter().enumerate() {
        engine.write32(STACK + index as u32 * 4, word).unwrap();
    }
    let mut state = State32 {
        eip: CODE,
        ..Default::default()
    };
    state.registers[4] = STACK;
    encode_state_current(&state, &mut engine.arena_mut().unwrap()[..STATE_SIZE]).unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 0,
            reason: ExitReason::Gate { id: api.id() },
        },
        &mut engine.arena_mut().unwrap()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
    )
    .unwrap();
    let token = engine
        .capture_call(31, engine.generation, CallingConvention32::Stdcall, 3)
        .unwrap()
        .token;
    engine
        .complete_windows_call(31, engine.generation, token)
        .unwrap();
    assert_eq!(
        decode_state(&engine.arena()[..STATE_SIZE])
            .unwrap()
            .registers[0],
        1
    );
    let debug = read(&engine, 0);
    assert_ne!(debug, 0);
    assert_ne!(debug, u32::MAX);
    engine.write32(debug + 24, 0x4352_544c).unwrap();
    let mut debug_bytes = [0; 32];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(debug), &mut debug_bytes)
        .unwrap();
    assert_eq!(
        u32::from_le_bytes(debug_bytes[4..8].try_into().unwrap()),
        CS
    );
    assert_eq!(
        u32::from_le_bytes(debug_bytes[24..28].try_into().unwrap()),
        0x4352_544c
    );
    engine
        .execute_critical_call(Api::DeleteCriticalSection, &[CS])
        .unwrap();
    assert!(engine.check_windows_runtime_bytes(debug, 32).is_err());

    let revision = engine.synchronization().unwrap().revision();
    let memory_version = engine.memory().unwrap().transaction_version();
    assert!(matches!(
        engine.execute_critical_call(Api::InitializeCriticalSectionEx, &[CS, 0, 0x8000_0000]),
        Err(HostError::Synchronization(Error::InvalidWait))
    ));
    assert_eq!(engine.synchronization().unwrap().revision(), revision);
    assert_eq!(
        engine.memory().unwrap().transaction_version(),
        memory_version
    );
    assert_eq!(read(&engine, 0), 0);
}
