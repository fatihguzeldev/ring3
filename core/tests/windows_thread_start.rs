#[path = "support/dll_executable.rs"]
#[allow(dead_code)]
mod dll_executable;
#[path = "support/imported_executable.rs"]
mod imported_executable;
#[path = "support/thread_start_cases.rs"]
mod thread_start_cases;

#[test]
fn ordered_notifications_precede_each_child_entry() {
    thread_start_cases::ordered_notifications_precede_each_child_entry();
}

#[test]
fn notification_frame_faults_preserve_progress_for_retry() {
    thread_start_cases::retryable_notification_frames();
}

use ring3_core::execution::{Cpu32, Permissions, Process32, ProcessStop, Register32, StopReason};
use thread_start_cases::{
    ENTER, FIRST, LOG, RETURN, SECOND, call, child, prepare_at, process, put, reach_return, word,
};

fn rejected(p: &mut Process32, cpu: Cpu32) {
    p.cpu = cpu;
    let run = p.run(1);
    assert_eq!(run.reason, ProcessStop::UnsupportedApi { address: cpu.eip });
    assert_eq!((run.instructions, run.api_calls), (0, 0));
    assert_eq!(p.cpu, cpu);
}

#[test]
fn private_entry_traps_require_the_right_child_phase_stack_and_owner() {
    let mut p = process(false);
    let primary = p.cpu;
    let (handle, first) = child(&mut p, 42);
    let (_, second) = child(&mut p, 73);
    for mut cpu in [primary, first] {
        cpu.eip = RETURN;
        rejected(&mut p, cpu);
    }
    for teb in [primary.fs_base(), 0x5000_0000] {
        let mut bad = first;
        bad.set_fs_base(teb);
        rejected(&mut p, bad);
    }
    let mut wrong_stack = first;
    wrong_stack.set_register(Register32::Esp, first.register(Register32::Esp) - 4);
    rejected(&mut p, wrong_stack);
    p.cpu = first;
    assert_eq!(
        p.run(0).reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!(p.cpu, first);
    reach_return(&mut p);
    let pending = p.cpu;
    assert_eq!(call(&mut p, 0x21c, &[handle]), 1);
    rejected(&mut p, second);
    let mut own_stack_return = second;
    own_stack_return.eip = RETURN;
    rejected(&mut p, own_stack_return);
    let mut foreign_return = pending;
    foreign_return.set_fs_base(second.fs_base());
    rejected(&mut p, foreign_return);
    p.cpu = pending;
    assert_eq!(
        p.run(1000).reason,
        ProcessStop::Stopped(StopReason::Breakpoint)
    );
    assert_eq!(word(&p, LOG), 0x123);
    for trap in [ENTER, RETURN] {
        let mut replay = first;
        replay.eip = trap;
        rejected(&mut p, replay);
    }
    p.cpu = second;
    assert_eq!(
        p.run(1000).reason,
        ProcessStop::Stopped(StopReason::Breakpoint)
    );
    assert_eq!(word(&p, LOG), 0x123_123);
}

fn prepare_third(p: &mut Process32) {
    p.memory
        .write(u64::from(LOG + 0x40), b"third.dll\0")
        .unwrap();
    prepare(p, 0x14, &[LOG + 0x40]);
}

#[test]
fn notification_owner_excludes_foreign_policy_and_deferred_attach() {
    let mut p = process(true);
    let primary = p.cpu;
    let (_, first) = child(&mut p, 42);
    let (_, second) = child(&mut p, 73);
    p.cpu = first;
    reach_return(&mut p);
    let pending = p.cpu;
    for offset in [0xfc, 0x14] {
        p.cpu = primary;
        put(&mut p, primary.fs_base() + 0x34, &[77]);
        if offset == 0xfc {
            prepare(&mut p, offset, &[SECOND]);
        } else {
            prepare_third(&mut p);
        }
        let before = p.cpu;
        rejected(&mut p, before);
        assert_eq!(word(&p, primary.fs_base() + 0x34), 77);
    }
    p.cpu = pending;
    assert_eq!(call(&mut p, 0xfc, &[SECOND]), 1);
    assert_eq!(
        p.run(1000).reason,
        ProcessStop::Stopped(StopReason::Breakpoint)
    );
    assert_eq!(word(&p, LOG), 1);
    p.cpu = primary;
    prepare_third(&mut p);
    assert_eq!(
        p.run(1000).reason,
        ProcessStop::Stopped(StopReason::Breakpoint)
    );
    assert_eq!(word(&p, LOG), 1);
    p.cpu = second;
    assert_eq!(
        p.run(1000).reason,
        ProcessStop::Stopped(StopReason::Breakpoint)
    );
    assert_eq!(word(&p, LOG), 0x113);
}

#[test]
fn first_frame_fault_publishes_no_owner_or_snapshot() {
    let mut p = process(true);
    let primary = p.cpu;
    let (_, cpu) = child(&mut p, 42);
    p.cpu = cpu;
    let page = u64::from(cpu.fs_base() - 4096);
    p.memory.protect(page, 4096, Permissions::READ).unwrap();
    assert!(matches!(
        p.run(1).reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(p.cpu, cpu);
    p.cpu = primary;
    prepare_third(&mut p);
    assert_eq!(
        p.run(1000).reason,
        ProcessStop::Stopped(StopReason::Breakpoint)
    );
    p.memory
        .protect(page, 4096, Permissions::READ_WRITE)
        .unwrap();
    p.cpu = cpu;
    assert_eq!(
        p.run(1000).reason,
        ProcessStop::Stopped(StopReason::Breakpoint)
    );
    assert_eq!(word(&p, LOG), 0x123);
}

#[test]
fn pending_process_attach_finishes_before_child_notifications() {
    let mut p = process(true);
    prepare_third(&mut p);
    assert_eq!(p.run(1).api_calls, 1);
    let attach = p.cpu;
    let (_, cpu) = child(&mut p, 42);
    rejected(&mut p, cpu);
    assert_eq!(word(&p, FIRST + 0x2194), 0);
    p.cpu = attach;
    assert_eq!(
        p.run(1000).reason,
        ProcessStop::Stopped(StopReason::Breakpoint)
    );
    p.cpu = cpu;
    assert_eq!(
        p.run(1000).reason,
        ProcessStop::Stopped(StopReason::Breakpoint)
    );
    assert_eq!(word(&p, LOG), 0x123);
}

fn prepare(p: &mut Process32, offset: u32, args: &[u32]) {
    prepare_at(p, offset, args, 0x1000_c000);
}

#[test]
fn primary_initialization_must_finish_before_child_entry() {
    let mut p = thread_start_cases::loaded(false);
    let startup = p.cpu;
    let (_, cpu) = child(&mut p, 42);
    rejected(&mut p, cpu);
    p.cpu = startup;
    assert_eq!(
        p.run(1000).reason,
        ProcessStop::Stopped(StopReason::Breakpoint)
    );
    p.cpu = cpu;
    assert_eq!(
        p.run(1000).reason,
        ProcessStop::Stopped(StopReason::Breakpoint)
    );
    assert_eq!(word(&p, LOG), 0x123);
}

#[test]
fn fully_disabled_notifications_reach_the_entry_without_a_callback() {
    let mut p = process(false);
    for handle in [FIRST, SECOND, thread_start_cases::THIRD] {
        assert_eq!(call(&mut p, 0xfc, &[handle]), 1);
    }
    for argument in [42, 73] {
        let (_, cpu) = child(&mut p, argument);
        p.cpu = cpu;
        let run = p.run(3);
        assert_eq!(run.reason, ProcessStop::Stopped(StopReason::Breakpoint));
        assert_eq!((run.instructions, run.api_calls), (3, 0));
        assert_eq!(word(&p, LOG), 0);
        assert_eq!(word(&p, LOG + 4), argument);
    }
}
