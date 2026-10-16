use super::support::{self, KEY};
use crate::{
    abi::x86::{STATE_SIZE, decode_state},
    process::{CallError, HostError},
    windows::{WindowsApi32, thread::ThreadSchedule32, winmm::WinmmApi32},
};

#[test]
fn full_invocation_queue_defers_the_exact_due_timer_and_worker_progress_reclaims_capacity() {
    let mut engine = support::engine(1);
    let first = engine
        .set_multimedia_timer(&[10, 0, 0x401000, 0x1111, 0])
        .unwrap();
    let second = engine
        .set_multimedia_timer(&[10, 0, 0x401000, 0x2222, 0])
        .unwrap();
    assert_ne!(first, 0);
    assert_ne!(second, first);
    support::complete(&mut engine, WindowsApi32::Sleep.id(), &[1000], false);
    engine.advance_cpu_clock(10_000_000).unwrap();
    engine.service_multimedia_timers(10_000_000).unwrap();
    let before = engine
        .winmm
        .timers
        .prepare_due(10_000_000)
        .unwrap()
        .unwrap()
        .identity;
    assert_eq!(before.timer, second);
    engine.service_multimedia_timers(10_000_000).unwrap();
    assert_eq!(
        engine
            .winmm
            .timers
            .prepare_due(10_000_000)
            .unwrap()
            .unwrap()
            .identity,
        before
    );
    assert_eq!(
        engine.schedule_windows_thread().unwrap(),
        ThreadSchedule32::Running { thread: 2 }
    );
    let first_frame = decode_state(&engine.arena()[..STATE_SIZE]).unwrap();
    assert_eq!(first_frame.eip, 0x401000);
    assert_eq!(support::word(&engine, first_frame.registers[4] + 4), first);
    assert_eq!(
        support::word(&engine, first_frame.registers[4] + 12),
        0x1111
    );
    engine.service_multimedia_timers(10_000_000).unwrap();
    assert!(
        engine
            .winmm
            .timers
            .prepare_due(10_000_000)
            .unwrap()
            .is_none()
    );
    support::complete(&mut engine, 0x000ffffe, &[], true);
    assert_eq!(
        engine.schedule_windows_thread().unwrap(),
        ThreadSchedule32::Running { thread: 2 }
    );
    let second_frame = decode_state(&engine.arena()[..STATE_SIZE]).unwrap();
    assert_eq!(
        support::word(&engine, second_frame.registers[4] + 4),
        second
    );
    assert_eq!(
        support::word(&engine, second_frame.registers[4] + 12),
        0x2222
    );
    support::complete(&mut engine, 0x000ffffe, &[], true);
    assert_eq!(engine.multimedia_timer_deadline(), None);
}

#[test]
fn synchronous_kill_keeps_the_provider_token_until_the_real_worker_callback_returns() {
    let mut engine = support::engine(2);
    let id = engine
        .set_multimedia_timer(&[10, 0, 0x401000, 0xabcdef, 0x101])
        .unwrap();
    assert_ne!(id, 0);
    engine.advance_cpu_clock(10_000_000).unwrap();
    engine.service_multimedia_timers(10_000_000).unwrap();
    let (generation, token) =
        support::capture(&mut engine, WinmmApi32::TimeKillEvent.id(), &[id], false);
    engine
        .complete_windows_call(KEY, generation, token)
        .unwrap();
    assert!(engine.winmm_provider_owns_token(token));
    assert!(matches!(
        engine.abandon_call(KEY, token),
        Err(HostError::Call(CallError::Busy))
    ));
    assert_eq!(
        engine.schedule_windows_thread().unwrap(),
        ThreadSchedule32::Running { thread: 2 }
    );
    assert_ne!(
        engine.windows_thread_info(2).unwrap().teb,
        engine.windows_thread_info(1).unwrap().teb
    );
    let state = decode_state(&engine.arena()[..STATE_SIZE]).unwrap();
    assert_eq!(support::word(&engine, state.registers[4] + 4), id);
    assert_eq!(support::word(&engine, state.registers[4] + 8), 0);
    assert_eq!(support::word(&engine, state.registers[4] + 12), 0xabcdef);
    assert_eq!(support::word(&engine, state.registers[4] + 16), 0);
    assert_eq!(support::word(&engine, state.registers[4] + 20), 0);
    support::complete(&mut engine, 0x000ffffe, &[], true);
    assert_eq!(
        engine.schedule_windows_thread().unwrap(),
        ThreadSchedule32::Running { thread: 1 }
    );
    assert!(!engine.winmm_provider_owns_token(token));
    assert_eq!(
        decode_state(&engine.arena()[..STATE_SIZE])
            .unwrap()
            .registers[0],
        0
    );
    assert_eq!(engine.multimedia_timer_deadline(), None);
}
