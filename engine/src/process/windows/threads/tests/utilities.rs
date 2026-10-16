use super::scheduling::engine;
use crate::{
    abi::{arena::*, x86::*},
    cpu::{ExecutionExit, ExitReason},
    memory::GuestAddress,
    process::{HostError, calls::call::PendingOwner},
    windows::thread::{ThreadApi32 as Api, ThreadError32, ThreadSchedule32},
};
#[test]
fn reported_duration_is_exact_epoch_single_use_checked_and_independent_of_idle_clock() {
    let (mut engine, unit) = engine();
    let epoch = engine
        .begin_exception_detection(PendingOwner::Resident(unit))
        .unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 3,
            reason: ExitReason::NeedCode,
        },
        &mut engine.arena_mut().unwrap()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
    )
    .unwrap();
    engine
        .report_windows_thread_execution(epoch, 12_345)
        .unwrap();
    assert_eq!(engine.threads.threads[0].user_ns, 12_345);
    assert!(matches!(
        engine.report_windows_thread_execution(epoch, 12_345),
        Err(HostError::Thread(ThreadError32::Stale))
    ));
    engine.advance_cpu_clock(9_000_000).unwrap();
    assert_eq!(engine.threads.threads[0].user_ns, 12_345);
    let next = engine
        .begin_exception_detection(PendingOwner::Resident(unit))
        .unwrap();
    engine.threads.threads[0].user_ns = u64::MAX - 5;
    assert!(matches!(
        engine.report_windows_thread_execution(next, 6),
        Err(HostError::Thread(ThreadError32::Exhausted))
    ));
    assert_eq!(engine.threads.threads[0].duration_epoch, epoch);
    assert_eq!(engine.threads.threads[0].user_ns, u64::MAX - 5);
    engine.report_windows_thread_execution(next, 5).unwrap();
    assert_eq!(engine.threads.threads[0].user_ns, u64::MAX);
}
#[test]
fn lazy_primary_handle_rights_priority_times_and_switch_expiry_use_one_thread_identity() {
    let (mut engine, unit) = engine();
    let handle = engine.open_windows_thread(1, 0x800, false).unwrap();
    assert_eq!(engine.resolve_query_thread(handle).unwrap(), 1);
    assert!(engine.resolve_thread_handle(handle, 2).is_err());
    assert_eq!(
        engine
            .execute_thread_utility(Api::GetThreadPriority, &[handle])
            .unwrap(),
        0
    );
    engine
        .execute_thread_utility(Api::SetThreadPriority, &[u32::MAX - 1, 2])
        .unwrap();
    assert_eq!(
        engine
            .execute_thread_utility(Api::GetThreadPriority, &[handle])
            .unwrap(),
        2
    );
    engine
        .execute_thread_utility(Api::SetThreadPriority, &[u32::MAX - 1, 0])
        .unwrap();
    engine
        .execute_thread_utility(
            Api::GetThreadTimes,
            &[handle, 0x11000, 0x11008, 0x11010, 0x11018],
        )
        .unwrap();
    let mut kernel = [0xff; 8];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(0x11010), &mut kernel)
        .unwrap();
    assert_eq!(kernel, [0; 8]);
    engine
        .create_windows_thread_at_gate(&[0, 0, 0x70000, 0, 0, 0], 0x70010)
        .unwrap();
    let epoch = engine
        .begin_exception_detection(PendingOwner::Resident(unit))
        .unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 1,
            reason: ExitReason::NeedCode,
        },
        &mut engine.arena_mut().unwrap()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
    )
    .unwrap();
    assert_eq!(
        engine.schedule_windows_thread().unwrap(),
        ThreadSchedule32::Running { thread: 2 }
    );
    assert_eq!(
        engine.schedule_windows_thread().unwrap(),
        ThreadSchedule32::Running { thread: 1 }
    );
    assert!(matches!(
        engine.report_windows_thread_execution(epoch, 1),
        Err(HostError::Thread(ThreadError32::Stale))
    ));
}
