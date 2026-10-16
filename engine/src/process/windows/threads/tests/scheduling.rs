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
        synchronization::{INFINITE, WaitMode32, WaitOutcome32},
        thread::{ThreadSchedule32, ThreadState32},
    },
};
const KEY: u64 = 0x5132;
const ENTRY: u32 = 0x70000;
const RETURN: u32 = 0x70010;
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
        RETURN,
        super::super::THREAD_RETURN_ID,
        SLEEP,
        crate::windows::WindowsApi32::Sleep.id(),
    ];
    for (index, value) in values.iter().enumerate() {
        engine.arena_mut().unwrap()[TRANSFER_OFFSET + index * 4..TRANSFER_OFFSET + index * 4 + 4]
            .copy_from_slice(&value.to_le_bytes());
    }
    let unit = engine.compile_resident_with_gates(3, 2).unwrap().get();
    let mut state = State32 {
        eip: ENTRY,
        ..Default::default()
    };
    state.registers[4] = 0x11f00;
    encode_state_current(&state, &mut engine.arena_mut().unwrap()[..STATE_SIZE]).unwrap();
    (engine, unit)
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
