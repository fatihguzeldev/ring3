use super::*;
use crate::{
    memory::{PAGE_SIZE, Permissions},
    process::windows::{exceptions::ExceptionPhase32, threads::StackGuardOutcome32},
    windows::thread::{ThreadApi32, ThreadSchedule32},
};

pub(super) fn child() -> EngineInstance {
    let mut engine = engine();
    engine.configure_cpu_clock(1_000_000_000, 0).unwrap();
    engine
        .configure_synchronization(Default::default())
        .unwrap();
    let (generation, token) = capture_id(
        &mut engine,
        ThreadApi32::CreateThread.id(),
        &[0, 0, 0x401000, 0, 0, 0],
    );
    engine
        .complete_windows_call(KEY, generation, token)
        .unwrap();
    assert_ne!(
        decode_state(&engine.arena()[..STATE_SIZE])
            .unwrap()
            .registers[0],
        0
    );
    assert_eq!(
        engine.schedule_windows_thread().unwrap(),
        ThreadSchedule32::Running { thread: 2 }
    );
    engine
}

#[test]
fn child_stack_alarm_grows_one_page_and_retries_exact_instruction_without_guest_handler() {
    let mut engine = child();
    let before = engine.windows_thread_info(2).unwrap();
    let guard_address = before.stack_base - PAGE_SIZE;
    let (unit, epoch) = permission_event(
        &mut engine,
        guard_address + 8,
        PagingAccess32::Write,
        0x401000,
    );
    let state = decode_state(&engine.arena()[..STATE_SIZE]).unwrap();
    let mapped = engine.memory().unwrap().mapped_pages();
    let dispatch = engine
        .dispatch_resident_windows_exception(KEY, unit, epoch)
        .unwrap();
    assert_eq!(dispatch.phase, ExceptionPhase32::Resumed);
    assert!(engine.pending_windows_exception().is_none());
    assert!(engine.cpu_event.is_none());
    assert_eq!(decode_state(&engine.arena()[..STATE_SIZE]).unwrap(), state);
    let after = engine.windows_thread_info(2).unwrap();
    assert_eq!(after.stack_base, guard_address);
    assert_eq!(after.stack_pages, before.stack_pages + 1);
    assert_eq!(engine.memory().unwrap().mapped_pages(), mapped + 1);
    assert_eq!(
        engine
            .memory()
            .unwrap()
            .permissions_at(GuestAddress(guard_address)),
        Ok(Permissions::READ_WRITE)
    );
    assert!(
        engine
            .memory()
            .unwrap()
            .guard_page(GuestAddress(guard_address - PAGE_SIZE))
            .is_some()
    );
    let mut limit = [0; 4];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(after.teb + 8), &mut limit)
        .unwrap();
    assert_eq!(u32::from_le_bytes(limit), guard_address);
}

#[test]
fn child_stack_growth_version_failure_does_not_consume_alarm_or_move_stack_limit() {
    let mut engine = child();
    let before = engine.windows_thread_info(2).unwrap();
    let guard_address = before.stack_base - PAGE_SIZE;
    let (_, epoch) = permission_event(&mut engine, guard_address, PagingAccess32::Read, 0x401000);
    let guard = engine
        .memory()
        .unwrap()
        .guard_page(GuestAddress(guard_address))
        .unwrap();
    let mapped = engine.memory().unwrap().mapped_pages();
    let version = engine.memory().unwrap().transaction_version();
    engine.memory.as_mut().unwrap().exhaust_versions_for_test();
    assert!(engine.dispatch_windows_exception(KEY, epoch).is_err());
    assert_eq!(engine.windows_thread_info(2).unwrap(), before);
    assert_eq!(engine.memory().unwrap().mapped_pages(), mapped);
    assert!(guard.is_current(engine.memory().unwrap()));
    assert!(engine.cpu_event.is_some());
    engine
        .memory
        .as_mut()
        .unwrap()
        .restore_version_for_test(version);
    assert_eq!(
        engine.dispatch_windows_exception(KEY, epoch).unwrap().phase,
        ExceptionPhase32::Resumed
    );
}

#[test]
fn exhausted_child_reservation_delivers_stack_overflow_on_the_final_emergency_page() {
    let mut engine = child();
    for _ in 0..64 {
        let info = engine.windows_thread_info(2).unwrap();
        let guard = engine
            .memory()
            .unwrap()
            .guard_page(GuestAddress(info.stack_base - PAGE_SIZE))
            .unwrap();
        match engine.grow_windows_stack_guard(guard).unwrap() {
            StackGuardOutcome32::Grown => continue,
            StackGuardOutcome32::Overflow(_) => break,
            StackGuardOutcome32::NotStack => panic!("child owns its actual stack guard"),
        }
    }
    let before = engine.windows_thread_info(2).unwrap();
    let guard_address = before.stack_base - PAGE_SIZE;
    let mut state = decode_state(&engine.arena()[..STATE_SIZE]).unwrap();
    state.registers[4] = before.stack_base + 16;
    encode_state_current(&state, &mut engine.arena_mut().unwrap()[..STATE_SIZE]).unwrap();
    engine
        .exceptions
        .handlers
        .add(false, true, 0x401000)
        .unwrap();
    let (_, epoch) = permission_event(
        &mut engine,
        guard_address + PAGE_SIZE - 4,
        PagingAccess32::Write,
        0x401000,
    );
    let dispatch = engine.dispatch_windows_exception(KEY, epoch).unwrap();
    assert_eq!(
        (dispatch.phase, dispatch.code),
        (ExceptionPhase32::Vectored, 0xc00000fd)
    );
    assert_eq!(
        engine.windows_thread_info(2).unwrap().stack_base,
        guard_address
    );
    assert!(
        engine
            .memory()
            .unwrap()
            .guard_page(GuestAddress(guard_address))
            .is_none()
    );
    assert!(dispatch.record >= guard_address && dispatch.record < before.stack_base);
}
