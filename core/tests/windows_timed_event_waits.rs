#[path = "support/event_wait_control.rs"]
#[allow(dead_code)]
mod event_wait_control;
#[path = "support/imported_executable.rs"]
mod imported_executable;
#[path = "support/timed_event_cases.rs"]
mod timed_event_cases;

#[test]
fn finite_wait_uses_exact_host_time_across_budgets() {
    timed_event_cases::finite_wait_uses_exact_host_time_across_budgets();
}

use event_wait_control::*;
use ring3_core::execution::{
    ClockError, Permissions, Process32, ProcessStop, Register32, StopReason,
};
use std::time::Duration;
use timed_event_cases::timed_process;

fn park(p: &mut Process32, event: u32) -> u32 {
    let child = call(p, 0x548, &[0, 0, CODE + 16, event, 4, 0]);
    assert_eq!(call(p, 0x254, &[child, 1]), 1);
    assert_eq!(call(p, 0x550, &[child]), 1);
    assert_eq!(
        p.run(100).reason,
        ProcessStop::Stopped(StopReason::Breakpoint)
    );
    assert_eq!(p.cpu.fs_base(), PRIMARY);
    assert_eq!(call(p, 0x254, &[CURRENT, 15]), 1);
    child
}

fn finish(p: &mut Process32, expected: u32) {
    assert_eq!(call(p, 0x254, &[CURRENT, 0]), 1);
    let run = p.run(100);
    assert_eq!(run.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!((run.instructions, run.api_calls), (2, 0));
    assert_eq!(p.cpu.fs_base(), CHILD);
    assert_eq!(p.cpu.register(Register32::Eax), expected);
}

#[test]
fn signal_and_timeout_publish_once_in_host_time_order() {
    for manual in [false, true] {
        for early in [false, true] {
            let mut p = timed_process(33);
            let event = call(&mut p, 0x53c, &[0, u32::from(manual), 0, 0]);
            park(&mut p, event);
            p.set_elapsed_time(Duration::from_millis(if early { 32 } else { 33 }))
                .unwrap();
            assert_eq!(call(&mut p, 0x540, &[event]), 1);
            assert_eq!(
                call(&mut p, 0x214, &[event, 0]),
                if early && !manual { 258 } else { 0 }
            );
            assert_eq!(call(&mut p, 0x544, &[event]), 1);
            p.set_elapsed_time(Duration::from_millis(100)).unwrap();
            denied(&mut p, 0x21c, &[event]);
            finish(&mut p, if early { 0 } else { 258 });
            assert_eq!(call(&mut p, 0x21c, &[event]), 1);
        }
    }
}

#[test]
fn final_resume_checks_current_signal_before_elapsed_deadline() {
    for manual in [false, true] {
        for (signal, reset, expected) in [(false, false, 258), (true, false, 0), (true, true, 258)]
        {
            let mut p = timed_process(33);
            let event = call(&mut p, 0x53c, &[0, u32::from(manual), 0, 0]);
            let child = park(&mut p, event);
            assert_eq!(call(&mut p, 0x554, &[child]), 0);
            assert_eq!(call(&mut p, 0x554, &[child]), 1);
            p.set_elapsed_time(Duration::from_millis(34)).unwrap();
            if signal {
                assert_eq!(call(&mut p, 0x540, &[event]), 1);
            }
            assert_eq!(call(&mut p, 0x550, &[child]), 2);
            if reset {
                assert_eq!(call(&mut p, 0x544, &[event]), 1);
            }
            denied(&mut p, 0x21c, &[event]);
            assert_eq!(call(&mut p, 0x550, &[child]), 1);
            assert_eq!(
                call(&mut p, 0x214, &[event, 0]),
                if manual && signal && !reset { 0 } else { 258 }
            );
            finish(&mut p, expected);
        }
    }
}

#[test]
fn timed_out_return_faults_retain_result_and_original_handle_after_suspend() {
    let mut p = timed_process(33);
    p.memory.write(u64::from(DATA + 32), b"timer\0").unwrap();
    let event = call(&mut p, 0x53c, &[0, 0, 0, DATA + 32]);
    let alias = call(&mut p, 0x53c, &[0, 0, 0, DATA + 32]);
    let child = park(&mut p, event);
    p.set_elapsed_time(Duration::from_millis(33)).unwrap();
    assert_eq!(call(&mut p, 0x554, &[child]), 0);
    assert_eq!(call(&mut p, 0x540, &[alias]), 1);
    assert_eq!(call(&mut p, 0x214, &[alias, 0]), 0);
    assert_eq!(call(&mut p, 0x544, &[alias]), 1);
    denied(&mut p, 0x21c, &[event]);
    put(&mut p, CHILD - 20, CODE);
    put(&mut p, CHILD - 16, 0);
    put(&mut p, CHILD - 12, u32::MAX);
    assert_eq!(call(&mut p, 0x550, &[child]), 1);
    p.memory
        .protect(u64::from(CHILD - 4096), 4096, Permissions::NONE)
        .unwrap();
    assert_eq!(call(&mut p, 0x254, &[CURRENT, 0]), 1);
    let first = p.run(100);
    assert!(matches!(
        first.reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    let before = p.cpu;
    assert_eq!((first.instructions, first.api_calls), (0, 0));
    for _ in 0..2 {
        assert_eq!(p.run(100), first);
        assert_eq!(p.cpu, before);
    }
    p.memory
        .protect(u64::from(CHILD - 4096), 4096, Permissions::READ_WRITE)
        .unwrap();
    let run = p.run(1);
    assert_eq!(run.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!((run.instructions, run.api_calls), (1, 0));
    assert_eq!(p.cpu.register(Register32::Eax), 258);
    assert_eq!(p.cpu.register(Register32::Esp), CHILD - 8);
    assert_eq!(call(&mut p, 0x21c, &[event]), 1);
}

#[test]
fn maximum_finite_deadlines_do_not_wrap_or_saturate_at_the_clock_ceiling() {
    let interval = u64::from(u32::MAX - 1) * 1_000_000;
    for start in [0, i64::MAX.cast_unsigned()] {
        let mut p = timed_process(u32::MAX - 1);
        p.set_elapsed_time(Duration::from_nanos(start)).unwrap();
        let event = call(&mut p, 0x53c, &[0, 0, 0, 0]);
        park(&mut p, event);
        if start == 0 {
            p.set_elapsed_time(Duration::from_nanos(interval - 1))
                .unwrap();
        } else {
            assert_eq!(
                p.set_elapsed_time(Duration::from_nanos(start + 1)),
                Err(ClockError::OutOfRange)
            );
        }
        assert_eq!(call(&mut p, 0x254, &[CURRENT, 0]), 1);
        p.cpu.eip = CODE + 2;
        assert_eq!(p.run(8192).instructions, 8192);
        assert_eq!(p.cpu.fs_base(), PRIMARY);
        if start == 0 {
            p.set_elapsed_time(Duration::from_nanos(interval)).unwrap();
        } else {
            assert_eq!(call(&mut p, 0x540, &[event]), 1);
        }
        assert_eq!(
            p.run(100).reason,
            ProcessStop::Stopped(StopReason::Breakpoint)
        );
        assert_eq!(p.cpu.fs_base(), CHILD);
        assert_eq!(
            p.cpu.register(Register32::Eax),
            if start == 0 { 258 } else { 0 }
        );
    }
}

#[test]
fn all_blocked_threads_resume_only_after_a_later_clock_sample_and_positive_budget() {
    let mut p = timed_process(33);
    let event = call(&mut p, 0x53c, &[0, 0, 0, 0]);
    let other = call(&mut p, 0x53c, &[0, 1, 0, 0]);
    park(&mut p, event);
    let before = prepare(&mut p, 0x214, &[other, u32::MAX]);
    let parked = p.run(1);
    assert_eq!((parked.instructions, parked.api_calls), (0, 1));
    assert_eq!(p.cpu, before);
    for elapsed in [0, 32, 32] {
        p.set_elapsed_time(Duration::from_millis(elapsed)).unwrap();
        let run = p.run(100);
        assert_eq!(run.reason, ProcessStop::WaitingForSynchronization);
        assert_eq!((run.instructions, run.api_calls), (0, 0));
        assert_eq!(p.cpu, before);
    }
    p.set_elapsed_time(Duration::from_millis(33)).unwrap();
    let zero = p.run(0);
    assert_eq!((zero.instructions, zero.api_calls), (0, 0));
    assert_eq!(p.cpu, before);
    for change in [0, 1, 2] {
        p.cpu = before;
        match change {
            0 => p.cpu.set_fs_base(CHILD),
            1 => p.cpu.eip = CODE,
            _ => p
                .cpu
                .set_register(Register32::Esp, before.register(Register32::Esp) + 4),
        }
        let invalid = p.cpu;
        let run = p.run(1);
        assert_eq!(
            run.reason,
            ProcessStop::UnsupportedApi {
                address: invalid.eip
            }
        );
        assert_eq!((run.instructions, run.api_calls), (0, 0));
        assert_eq!(p.cpu, invalid);
    }
    p.cpu = before;
    let run = p.run(100);
    assert_eq!(run.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!((run.instructions, run.api_calls), (2, 0));
    assert_eq!(p.cpu.fs_base(), CHILD);
    assert_eq!(p.cpu.register(Register32::Eax), 258);
}

#[test]
fn an_expired_waiter_does_not_take_a_later_signal_from_another_waiter() {
    let mut p = timed_process(33);
    let event = call(&mut p, 0x53c, &[0, 0, 0, 0]);
    park(&mut p, event);
    p.set_elapsed_time(Duration::from_millis(10)).unwrap();
    assert_eq!(call(&mut p, 0x254, &[CURRENT, 0]), 1);
    park(&mut p, event);
    p.set_elapsed_time(Duration::from_millis(33)).unwrap();
    assert_eq!(call(&mut p, 0x540, &[event]), 1);
    denied(&mut p, 0x21c, &[event]);
    finish(&mut p, 258);
    denied(&mut p, 0x21c, &[event]);
    assert_eq!(call(&mut p, 0x254, &[CURRENT, (-2_i32).cast_unsigned()]), 1);
    let run = p.run(100);
    assert_eq!(run.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!((run.instructions, run.api_calls), (2, 0));
    assert_eq!(p.cpu.fs_base(), CHILD + 0x11000);
    assert_eq!(p.cpu.register(Register32::Eax), 0);
    assert_eq!(call(&mut p, 0x214, &[event, 0]), 258);
    assert_eq!(call(&mut p, 0x21c, &[event]), 1);
}

#[test]
fn finite_wait_frame_overflow_does_not_register_or_capture_an_early_deadline() {
    use ring3_core::execution::MemoryError;
    let mut p = timed_process(33);
    let event = call(&mut p, 0x53c, &[0, 0, 0, 0]);
    park(&mut p, event);
    p.memory
        .map_zeroed(0xffff_f000, 4096, Permissions::READ_WRITE)
        .unwrap();
    put(&mut p, 0xffff_fff4, CODE);
    put(&mut p, 0xffff_fff8, event);
    put(&mut p, 0xffff_fffc, 33);
    p.cpu.eip = 0x7000_0214;
    p.cpu.set_register(Register32::Esp, 0xffff_fff4);
    let invalid = p.cpu;
    for _ in 0..2 {
        let run = p.run(1);
        assert_eq!(
            run.reason,
            ProcessStop::Stopped(StopReason::MemoryFault(MemoryError::AddressOverflow))
        );
        assert_eq!((run.instructions, run.api_calls), (0, 0));
        assert_eq!(p.cpu, invalid);
    }
    p.set_elapsed_time(Duration::from_millis(5)).unwrap();
    let before = prepare(&mut p, 0x214, &[event, 33]);
    let run = p.run(1);
    assert_eq!((run.instructions, run.api_calls), (0, 1));
    assert_eq!(p.cpu, before);
    p.set_elapsed_time(Duration::from_millis(33)).unwrap();
    let child = p.run(100);
    assert_eq!(child.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!(p.cpu.fs_base(), CHILD);
    p.set_elapsed_time(Duration::from_millis(38)).unwrap();
    let parent = p.run(100);
    assert_eq!(parent.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!((parent.instructions, parent.api_calls), (1, 0));
    assert_eq!(p.cpu.fs_base(), PRIMARY);
    assert_eq!(p.cpu.register(Register32::Eax), 258);
}
