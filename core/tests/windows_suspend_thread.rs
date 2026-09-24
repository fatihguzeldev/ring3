#[path = "support/imported_executable.rs"]
mod imported_executable;
#[path = "support/suspended_thread_cases.rs"]
mod suspended_thread_cases;

#[test]
fn suspended_guest_preserves_counts_across_host_budgets() {
    suspended_thread_cases::suspended_guest_preserves_counts_across_host_budgets();
}

#[path = "support/event_wait_control.rs"]
mod event_wait_control;
#[path = "support/suspended_event_cases.rs"]
mod suspended_event_cases;

#[test]
fn suspended_event_waits_preserve_signal_and_handle_ownership() {
    suspended_event_cases::suspended_event_waits_preserve_signal_and_handle_ownership();
}

use event_wait_control::*;
use ring3_core::execution::{Permissions, ProcessStop, Register32, StopReason};

#[test]
fn count_limit_and_invalid_handles_preflight_error_writes() {
    let mut p = process();
    let child = call(&mut p, 0x548, &[0, 0, CODE, 0, 4, 0]);
    let closed = call(&mut p, 0x548, &[0, 0, CODE, 0, 4, 0]);
    let event = call(&mut p, 0x53c, &[0, 0, 0, 0]);
    assert_eq!(call(&mut p, 0x21c, &[closed]), 1);
    put(&mut p, PRIMARY + 0x34, 77);
    p.memory
        .protect(u64::from(PRIMARY), 4096, Permissions::READ)
        .unwrap();
    for previous in 1..127 {
        assert_eq!(call(&mut p, 0x554, &[child]), previous);
    }
    for target in [child, closed, event, 0, u32::MAX] {
        let before = prepare(&mut p, 0x554, &[target]);
        for _ in 0..2 {
            let run = p.run(1);
            assert!(matches!(
                run.reason,
                ProcessStop::Stopped(StopReason::MemoryFault(_))
            ));
            assert_eq!((run.instructions, run.api_calls), (0, 0));
            assert_eq!(p.cpu, before);
            assert_eq!(p.last_error().unwrap(), 77);
        }
    }
    p.memory
        .protect(u64::from(PRIMARY), 4096, Permissions::READ_WRITE)
        .unwrap();
    assert_eq!(call(&mut p, 0x554, &[child]), u32::MAX);
    assert_eq!(p.last_error().unwrap(), 156);
    assert_eq!(call(&mut p, 0x550, &[child]), 127);
    assert_eq!(call(&mut p, 0x554, &[child]), 126);
    for target in [closed, event, 0, u32::MAX] {
        assert_eq!(call(&mut p, 0x554, &[target]), u32::MAX);
        assert_eq!(p.last_error().unwrap(), 6);
    }
}

#[test]
fn incomplete_frames_zero_budget_and_foreign_fs_preserve_counts() {
    let mut p = process();
    let child = call(&mut p, 0x548, &[0, 0, CODE, 0, 4, 0]);
    let valid = prepare(&mut p, 0x554, &[child]);
    for fs in [0, CHILD] {
        p.cpu = valid;
        p.cpu.set_fs_base(fs);
        let before = p.cpu;
        let run = p.run(1);
        assert_eq!(
            run.reason,
            ProcessStop::UnsupportedApi {
                address: before.eip
            }
        );
        assert_eq!((run.instructions, run.api_calls), (0, 0));
        assert_eq!(p.cpu, before);
    }
    p.cpu = valid;
    put(&mut p, 0x1000_fffc, CODE);
    p.cpu.set_register(Register32::Esp, 0x1000_fffc);
    let before = p.cpu;
    for _ in 0..2 {
        let run = p.run(1);
        assert!(matches!(
            run.reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
        assert_eq!((run.instructions, run.api_calls), (0, 0));
        assert_eq!(p.cpu, before);
    }
    p.cpu = valid;
    assert_eq!((p.run(0).instructions, p.run(0).api_calls), (0, 0));
    assert_eq!(p.cpu, valid);
    denied(&mut p, 0x554, &[CURRENT]);
    assert_eq!(call(&mut p, 0x554, &[child]), 1);
    assert_eq!(call(&mut p, 0x550, &[child]), 2);
    assert_eq!(call(&mut p, 0x550, &[child]), 1);
    assert_eq!(
        p.run(100).reason,
        ProcessStop::Stopped(StopReason::Breakpoint)
    );
    assert_eq!(p.cpu.fs_base(), CHILD);
    denied(&mut p, 0x554, &[child]);
    denied(&mut p, 0x554, &[CURRENT]);
    let other = call(&mut p, 0x548, &[0, 0, CODE, 0, 4, 0]);
    p.memory
        .protect(u64::from(CHILD), 4096, Permissions::NONE)
        .unwrap();
    assert_eq!(call(&mut p, 0x554, &[other]), 1);
    assert_eq!(call(&mut p, 0x550, &[other]), 2);
    assert_eq!(call(&mut p, 0x550, &[child]), 0);
}

#[test]
fn stack_cleanup_overflow_precedes_count_and_scheduling_changes() {
    use ring3_core::execution::MemoryError;
    let mut p = process();
    let child = call(&mut p, 0x548, &[0, 0, CODE, 0, 4, 0]);
    p.memory
        .map_zeroed(0xffff_f000, 4096, Permissions::READ_WRITE)
        .unwrap();
    put(&mut p, 0xffff_fff8, CODE);
    put(&mut p, 0xffff_fffc, child);
    for offset in [0x554, 0x550] {
        p.cpu.eip = 0x7000_0000 + offset;
        p.cpu.set_register(Register32::Esp, 0xffff_fff8);
        let before = p.cpu;
        for _ in 0..2 {
            let run = p.run(1);
            assert_eq!(
                run.reason,
                ProcessStop::Stopped(StopReason::MemoryFault(MemoryError::AddressOverflow))
            );
            assert_eq!((run.instructions, run.api_calls), (0, 0));
            assert_eq!(p.cpu, before);
        }
    }
    assert_eq!(call(&mut p, 0x554, &[child]), 1);
    assert_eq!(call(&mut p, 0x550, &[child]), 2);
}
