use crate::{
    abi::{
        arena::{EXIT_OFFSET, TRANSFER_OFFSET},
        x86::{EXIT_SIZE, STATE_SIZE, encode_exit_v3, encode_state_current},
    },
    cpu::{ExecutionExit, ExitReason, x86::State32},
    memory::{GuestAddress, PageRange},
    process::EngineInstance,
    windows::{
        CallingConvention32,
        synchronization::{INFINITE, SynchronizationApi32, WaitMode32, WaitOutcome32},
        thread::{ThreadSchedule32, ThreadState32},
    },
};
const KEY: u64 = 0x5132;
const ENTRY: u32 = 0x70000;
const RETURN: u32 = 0x70010;
const WAIT: u32 = 0x70030;
const RELEASE_MUTEX: u32 = 0x70040;
pub(super) const SLEEP: u32 = 0x70020;
pub(super) fn engine() -> (EngineInstance, u64) {
    let mut engine = EngineInstance::new(64, KEY).unwrap();
    engine.configure_cpu_clock(1_000_000_000, 0).unwrap();
    engine.map(0x10000, 2, 3).unwrap();
    engine.map(0x70000, 1, 7).unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(ENTRY), &[0x90, 0xc2, 4, 0])
        .unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(RETURN), &[0x0f, 0x0b])
        .unwrap();
    engine
        .configure_synchronization(Default::default())
        .unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(SLEEP), &[0x0f, 0x0b])
        .unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(WAIT), &[0x0f, 0x0b])
        .unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(RELEASE_MUTEX), &[0x0f, 0x0b])
        .unwrap();
    let primary = engine
        .threads
        .prepare_primary_thread(0x10000, PageRange::new(GuestAddress(0x11000), 1).unwrap())
        .unwrap();
    engine.threads.commit_primary_thread(primary);
    let values = [
        ENTRY,
        4,
        RETURN,
        2,
        SLEEP,
        2,
        WAIT,
        2,
        RELEASE_MUTEX,
        2,
        RETURN,
        super::super::THREAD_RETURN_ID,
        SLEEP,
        crate::windows::WindowsApi32::Sleep.id(),
        WAIT,
        crate::windows::WindowsApi32::WaitForSingleObject.id(),
        RELEASE_MUTEX,
        SynchronizationApi32::ReleaseMutex.id(),
    ];
    for (index, value) in values.iter().enumerate() {
        engine.arena_mut().unwrap()[TRANSFER_OFFSET + index * 4..TRANSFER_OFFSET + index * 4 + 4]
            .copy_from_slice(&value.to_le_bytes());
    }
    let unit = engine.compile_resident_with_gates(5, 4).unwrap().get();
    let mut state = State32 {
        eip: ENTRY,
        ..Default::default()
    };
    state.registers[4] = 0x11f00;
    encode_state_current(&state, &mut engine.arena_mut().unwrap()[..STATE_SIZE]).unwrap();
    (engine, unit)
}

fn mutex_wait_fixture(depth: u32) -> (EngineInstance, u64, u32, u32) {
    let (mut engine, unit) = engine();
    let mutex = engine
        .synchronization_mut()
        .unwrap()
        .create_mutex(true, false, None)
        .unwrap()
        .0;
    for _ in 1..depth {
        assert!(
            engine
                .synchronization_mut()
                .unwrap()
                .try_acquire_mutex(mutex)
                .unwrap()
        );
    }
    let child = engine
        .create_windows_thread_at_gate(&[0, 0, ENTRY, 0, 0, 0], RETURN)
        .unwrap();
    assert_eq!(
        engine.schedule_windows_thread().unwrap(),
        ThreadSchedule32::Running { thread: 2 }
    );
    let mut state = crate::abi::x86::decode_state(&engine.arena()[..STATE_SIZE]).unwrap();
    state.eip = WAIT;
    state.registers[4] -= 64;
    for (index, word) in [ENTRY, mutex, INFINITE].iter().enumerate() {
        engine
            .memory
            .as_mut()
            .unwrap()
            .write(
                GuestAddress(state.registers[4] + index as u32 * 4),
                &word.to_le_bytes(),
            )
            .unwrap();
    }
    encode_state_current(&state, &mut engine.arena_mut().unwrap()[..STATE_SIZE]).unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 0,
            reason: ExitReason::Gate {
                id: crate::windows::WindowsApi32::WaitForSingleObject.id(),
            },
        },
        &mut engine.arena_mut().unwrap()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
    )
    .unwrap();
    let captured = engine
        .capture_resident_call(KEY, unit, CallingConvention32::Stdcall, 2)
        .unwrap();
    engine
        .complete_resident_windows_call(KEY, unit, captured.token)
        .unwrap();
    assert_eq!(
        engine.schedule_windows_thread().unwrap(),
        ThreadSchedule32::Running { thread: 1 }
    );
    assert_eq!(
        engine.windows_thread_info(2).unwrap().state,
        ThreadState32::Waiting
    );
    assert_eq!(engine.pending_windows_thread_schedule(), None);
    (engine, unit, mutex, child)
}

fn release_mutex_at_gate(engine: &mut EngineInstance, unit: u64, mutex: u32) {
    let mut state = crate::abi::x86::decode_state(&engine.arena()[..STATE_SIZE]).unwrap();
    state.eip = RELEASE_MUTEX;
    state.registers[4] = 0x11f00;
    engine
        .memory
        .as_mut()
        .unwrap()
        .write_spans(&[
            (GuestAddress(state.registers[4]), &ENTRY.to_le_bytes()),
            (GuestAddress(state.registers[4] + 4), &mutex.to_le_bytes()),
        ])
        .unwrap();
    encode_state_current(&state, &mut engine.arena_mut().unwrap()[..STATE_SIZE]).unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 0,
            reason: ExitReason::Gate {
                id: SynchronizationApi32::ReleaseMutex.id(),
            },
        },
        &mut engine.arena_mut().unwrap()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
    )
    .unwrap();
    let captured = engine
        .capture_resident_call(KEY, unit, CallingConvention32::Stdcall, 1)
        .unwrap();
    let version = engine.memory().unwrap().transaction_version();
    engine
        .complete_resident_windows_call(KEY, unit, captured.token)
        .unwrap();
    assert_eq!(engine.memory().unwrap().transaction_version(), version);
    let result = crate::abi::x86::decode_state(&engine.arena()[..STATE_SIZE]).unwrap();
    assert_eq!(result.registers[0], 1);
    assert!(engine.pending_call.is_none());
}

#[test]
fn release_mutex_handoff_requests_schedule_only_after_final_recursive_release() {
    let (mut engine, unit, mutex, _) = mutex_wait_fixture(2);
    for (owner, depth, scheduled) in [(1, 1, None), (2, 1, Some(2))] {
        release_mutex_at_gate(&mut engine, unit, mutex);
        assert_eq!(
            engine
                .synchronization()
                .unwrap()
                .mutex_state(mutex)
                .unwrap(),
            (owner, depth)
        );
        assert_eq!(engine.pending_windows_thread_schedule(), scheduled);
        assert_eq!(engine.threads.current_id(), 1);
    }
    assert_eq!(
        engine.schedule_windows_thread().unwrap(),
        ThreadSchedule32::Running { thread: 2 }
    );
    let resumed = crate::abi::x86::decode_state(&engine.arena()[..STATE_SIZE]).unwrap();
    assert_eq!(resumed.eip, ENTRY);
    assert_eq!(resumed.registers[0], 0);
    assert!(engine.pending_call.is_none());
    assert!(engine.pending_windows_wait().is_none());
    assert_eq!(engine.pending_windows_thread_schedule(), None);
}

#[test]
fn release_mutex_handoff_keeps_suspended_and_existing_schedule_markers_neutral() {
    for suspended in [false, true] {
        let (mut engine, unit, mutex, child) = mutex_wait_fixture(1);
        if suspended {
            assert_eq!(engine.change_thread_suspend(child, false).unwrap(), 0);
        } else {
            engine.threads.schedule_required = Some(1);
        }
        let marker = engine.pending_windows_thread_schedule();
        release_mutex_at_gate(&mut engine, unit, mutex);
        assert_eq!(
            engine
                .synchronization()
                .unwrap()
                .mutex_state(mutex)
                .unwrap(),
            (2, 1)
        );
        assert_eq!(engine.pending_windows_thread_schedule(), marker);
        assert_eq!(engine.threads.current_id(), 1);
    }
    let (mut engine, unit) = engine();
    let mutex = engine
        .synchronization_mut()
        .unwrap()
        .create_mutex(true, false, None)
        .unwrap()
        .0;
    release_mutex_at_gate(&mut engine, unit, mutex);
    assert_eq!(
        engine
            .synchronization()
            .unwrap()
            .mutex_state(mutex)
            .unwrap(),
        (0, 0)
    );
    assert_eq!(engine.pending_windows_thread_schedule(), None);
}

#[test]
fn child_thread_has_distinct_teb_tls_and_real_return_signals_shared_wait_object() {
    let (mut engine, unit) = engine();
    let tls = engine.allocate_windows_tls().unwrap();
    engine.set_windows_tls(tls, 0x1111).unwrap();
    let handle = engine
        .create_windows_thread_at_gate(&[0, 0, ENTRY, 0xabcd, 0, 0x11000], RETURN)
        .unwrap();
    assert_eq!(
        engine.windows_thread_info(2).unwrap().state,
        ThreadState32::Runnable
    );
    assert_eq!(
        engine.schedule_windows_thread().unwrap(),
        ThreadSchedule32::Running { thread: 2 }
    );
    assert_eq!(engine.threads.current_id(), 2);
    let child = engine.threads.current_teb().unwrap();
    assert_ne!(child, 0x10000);
    assert_eq!(engine.get_windows_tls(tls).unwrap(), 0);
    engine.set_windows_tls(tls, 0x2222).unwrap();
    assert_eq!(
        engine.schedule_windows_thread().unwrap(),
        ThreadSchedule32::Running { thread: 1 }
    );
    assert_eq!(engine.get_windows_tls(tls).unwrap(), 0x1111);
    let wait = engine
        .synchronization_mut()
        .unwrap()
        .wait(&[handle], WaitMode32::Any, INFINITE, false, None)
        .unwrap();
    let WaitOutcome32::Pending(wait) = wait else {
        panic!("live child cannot satisfy thread wait");
    };
    assert_eq!(
        engine.schedule_windows_thread().unwrap(),
        ThreadSchedule32::Running { thread: 2 }
    );
    let stack = engine.windows_thread_info(2).unwrap();
    let mut state = State32 {
        eip: RETURN,
        ..Default::default()
    };
    state.registers[0] = 37;
    state.registers[4] = stack.stack_base + stack.stack_pages * 4096 - 4;
    encode_state_current(&state, &mut engine.arena_mut().unwrap()[..STATE_SIZE]).unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 0,
            reason: ExitReason::Gate {
                id: super::super::THREAD_RETURN_ID,
            },
        },
        &mut engine.arena_mut().unwrap()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
    )
    .unwrap();
    let captured = engine
        .capture_resident_call(KEY, unit, CallingConvention32::Stdcall, 0)
        .unwrap();
    engine
        .complete_resident_windows_call(KEY, unit, captured.token)
        .unwrap();
    assert_eq!(
        engine.windows_thread_info(2).unwrap().state,
        ThreadState32::Terminated
    );
    assert_eq!(engine.windows_thread_info(2).unwrap().exit_code, 37);
    assert!(
        engine
            .synchronization()
            .unwrap()
            .wait_status(wait)
            .unwrap()
            .is_some()
    );
    assert_eq!(engine.pending_windows_thread_schedule(), Some(2));
    assert_eq!(
        engine.schedule_windows_thread().unwrap(),
        ThreadSchedule32::Running { thread: 1 }
    );
    assert_eq!(engine.threads.current_id(), 1);
    assert_eq!(engine.get_windows_tls(tls).unwrap(), 0x1111);
    assert!(
        engine
            .memory()
            .unwrap()
            .read(GuestAddress(child), &mut [0; 1])
            .is_err()
    );
}
#[test]
fn invalid_thread_output_does_not_publish_handle_reservation_or_child_and_suspended_child_never_runs()
 {
    let (mut engine, _) = engine();
    let mapped = engine.memory().unwrap().mapped_pages();
    let regions = engine.windows_memory.reservation_count();
    let revision = engine.synchronization().unwrap().revision();
    assert!(
        engine
            .create_windows_thread_at_gate(&[0, 0, ENTRY, 0, 0, 0x5555], RETURN)
            .is_err()
    );
    assert_eq!(engine.threads.threads.len(), 1);
    assert_eq!(engine.windows_memory.reservation_count(), regions);
    assert_eq!(engine.memory().unwrap().mapped_pages(), mapped);
    assert_eq!(engine.synchronization().unwrap().revision(), revision);
    let handle = engine
        .create_windows_thread_at_gate(&[0, 0, ENTRY, 0, 4, 0], RETURN)
        .unwrap();
    assert_eq!(
        engine.schedule_windows_thread().unwrap(),
        ThreadSchedule32::Running { thread: 1 }
    );
    assert_eq!(engine.change_thread_suspend(handle, true).unwrap(), 1);
    assert_eq!(
        engine.schedule_windows_thread().unwrap(),
        ThreadSchedule32::Running { thread: 2 }
    );
    engine.terminate_windows_thread(2, 99).unwrap();
    assert!(
        engine
            .synchronization()
            .unwrap()
            .validate_thread_access(handle, 1)
            .is_ok()
    );
}

#[test]
fn captured_child_return_ends_its_service_scope_before_resuming_the_parent_wait() {
    let (mut engine, unit) = engine();
    let child = engine
        .create_windows_thread_at_gate(&[0, 0, ENTRY, 0, 0, 0], RETURN)
        .unwrap();
    let mut parent = State32 {
        eip: WAIT,
        ..Default::default()
    };
    parent.registers[4] = 0x11e00;
    for (index, word) in [ENTRY, child, INFINITE].iter().enumerate() {
        engine
            .memory
            .as_mut()
            .unwrap()
            .write(
                GuestAddress(parent.registers[4] + index as u32 * 4),
                &word.to_le_bytes(),
            )
            .unwrap();
    }
    encode_state_current(&parent, &mut engine.arena_mut().unwrap()[..STATE_SIZE]).unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 0,
            reason: ExitReason::Gate {
                id: crate::windows::WindowsApi32::WaitForSingleObject.id(),
            },
        },
        &mut engine.arena_mut().unwrap()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
    )
    .unwrap();
    let wait = engine
        .capture_resident_call(KEY, unit, CallingConvention32::Stdcall, 2)
        .unwrap();
    engine
        .complete_resident_windows_call(KEY, unit, wait.token)
        .unwrap();
    assert_eq!(
        engine.schedule_windows_thread().unwrap(),
        ThreadSchedule32::Running { thread: 2 }
    );
    let child = engine.windows_thread_info(2).unwrap();
    let mut returned = State32 {
        eip: RETURN,
        ..Default::default()
    };
    returned.registers[0] = 37;
    returned.registers[4] = child.stack_base + child.stack_pages * 4096 - 4;
    encode_state_current(&returned, &mut engine.arena_mut().unwrap()[..STATE_SIZE]).unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 0,
            reason: ExitReason::Gate {
                id: super::super::THREAD_RETURN_ID,
            },
        },
        &mut engine.arena_mut().unwrap()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
    )
    .unwrap();
    let call = engine
        .capture_resident_call(KEY, unit, CallingConvention32::Stdcall, 0)
        .unwrap();
    engine
        .complete_resident_windows_call(KEY, unit, call.token)
        .unwrap();
    assert_eq!(engine.threads.current_id(), 2);
    assert!(engine.windows_service.is_none());
    assert_eq!(engine.pending_windows_thread_schedule(), Some(2));
    assert!(matches!(
        engine.guard_dispatch_entry(KEY),
        Err(crate::process::HostError::Call(
            crate::process::CallError::Busy
        ))
    ));
    assert_eq!(
        engine.schedule_windows_thread().unwrap(),
        ThreadSchedule32::Running { thread: 1 }
    );
    assert!(engine.pending_call.is_none());
    assert!(engine.pending_windows_wait().is_none());
    let completed = crate::abi::x86::decode_state(&engine.arena()[..STATE_SIZE]).unwrap();
    assert_eq!(completed.eip, ENTRY);
    assert_eq!(completed.registers[0], 0);
    assert_eq!(completed.registers[4], parent.registers[4] + 12);
    assert_eq!(engine.windows_thread_info(2).unwrap().exit_code, 37);
    assert!(
        engine
            .complete_resident_windows_call(KEY, unit, wait.token)
            .is_err()
    );
}
