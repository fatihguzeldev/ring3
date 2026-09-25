use super::cxx_exception_cases;

use cxx_exception_cases::{
    CATCHABLE, CLEANUP, CODE, CONTINUE, HANDLERS, INFO, INNER_ACTION, INNER_INFO, INNER_RECORD,
    INNER_UNWIND, MARKER, OBJECT, RECORD, STACK, THROW, THROW_INFO, UNWIND, fixture_variant, put,
    word,
};
use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};

fn fixture() -> Process32 {
    fixture_variant(false)
}

#[test]
fn pending_throws_and_returns_belong_to_their_registered_threads() {
    let mut process = fixture();
    let throw_cpu = process.cpu;
    put(&mut process, 0x1000_ff00, &[CONTINUE, 0, 0, CODE, 0, 4, 0]);
    process.cpu.eip = 0x7000_0548;
    process.cpu.set_register(Register32::Esp, 0x1000_ff00);
    assert_eq!(process.run(1).api_calls, 1);
    let handle = process.cpu.register(Register32::Eax);
    process.cpu = throw_cpu;
    assert_eq!(process.run(1).api_calls, 1);
    let primary = process.cpu;
    let child_record = 0x1100_ef00;
    let child_stack = child_record - 0x200;
    put(&mut process, child_record - 4, &[child_stack + 12]);
    put(&mut process, child_record, &[u32::MAX, CODE, 2]);
    put(&mut process, child_stack, &[CONTINUE, OBJECT, THROW_INFO]);
    put(&mut process, 0x1101_0000, &[child_record]);
    process.cpu = throw_cpu;
    process.cpu.set_fs_base(0x1101_0000);
    process.cpu.set_register(Register32::Ebp, child_record + 12);
    process.cpu.set_register(Register32::Esp, child_stack);
    assert_eq!(process.run(1).api_calls, 1);
    let child = process.cpu;
    put(&mut process, 0x1000_ff00, &[CONTINUE, handle]);
    process.cpu.eip = 0x7000_021c;
    process.cpu.set_register(Register32::Esp, 0x1000_ff00);
    assert_eq!(process.run(1).api_calls, 1);
    assert_eq!(process.cpu.register(Register32::Eax), 1);
    process.cpu = primary;
    assert_eq!(
        process.run(32).reason,
        ProcessStop::Stopped(StopReason::Breakpoint)
    );
    assert_eq!(process.cpu.register(Register32::Esp), STACK + 12);
    assert_eq!(word(&process, RECORD + 8), 1);
    assert_eq!(word(&process, child_record + 8), 1);
    process.cpu = child;
    assert_eq!(process.run(2).instructions, 2);
    assert_eq!(process.cpu.eip, 0x7000_0ff4);
    let child_return = process.cpu;
    for teb in [0x7ffd_e000, 0x5000_0000] {
        process.cpu = child_return;
        process.cpu.set_fs_base(teb);
        let before = process.cpu;
        let run = process.run(1);
        assert_eq!(
            run.reason,
            ProcessStop::UnsupportedApi {
                address: 0x7000_0ff4
            }
        );
        assert_eq!((run.instructions, run.api_calls), (0, 0));
        assert_eq!(process.cpu, before);
    }
    process.cpu = child_return;
    assert_eq!(
        process.run(32).reason,
        ProcessStop::Stopped(StopReason::Breakpoint)
    );
    assert_eq!(process.cpu.register(Register32::Esp), child_stack + 12);
    assert_eq!(process.cpu.register(Register32::Ecx), 1);
}

#[test]
fn guest_cleanup_runs_before_catch_and_resumes_at_saved_frame_stack() {
    let mut process = fixture();
    let mut reached = false;
    for _ in 0..32 {
        let result = process.run(1);
        if result.reason == ProcessStop::Stopped(StopReason::Breakpoint) {
            reached = true;
            break;
        }
        assert_eq!(
            result.reason,
            ProcessStop::Stopped(StopReason::InstructionLimit)
        );
    }
    assert!(reached);
    assert_eq!(process.cpu.register(Register32::Ecx), 1);
    assert_eq!(word(&process, MARKER), 1);
    assert_eq!(word(&process, RECORD + 8), 1);
    assert_eq!(process.cpu.register(Register32::Esp), STACK + 12);
}

#[test]
fn malformed_metadata_typed_catch_and_object_destructor_stop_without_unwinding() {
    for case in 0..5 {
        let mut process = fixture();
        match case {
            0 => put(&mut process, INFO, &[0]),
            1 => put(&mut process, HANDLERS + 4, &[OBJECT]),
            2 => put(&mut process, THROW_INFO + 4, &[CLEANUP]),
            3 => put(&mut process, UNWIND + 2 * 8, &[2, CLEANUP]),
            _ => put(&mut process, RECORD - 4, &[STACK]),
        }
        let before = process.cpu;
        let result = process.run(1);
        assert_eq!(
            result.reason,
            ProcessStop::UnsupportedApi { address: THROW }
        );
        assert_eq!((result.instructions, result.api_calls), (0, 0));
        assert_eq!(process.cpu, before);
        assert_eq!(word(&process, RECORD + 8), 2);
        assert_eq!(word(&process, MARKER), 0);
    }
}

#[test]
fn inner_cleanup_unlinks_its_frame_before_matching_simple_typed_catch() {
    let mut process = fixture_variant(true);
    let mut reached = false;
    for _ in 0..32 {
        let result = process.run(1);
        if result.reason == ProcessStop::Stopped(StopReason::Breakpoint) {
            reached = true;
            break;
        }
        assert_eq!(
            result.reason,
            ProcessStop::Stopped(StopReason::InstructionLimit)
        );
    }
    assert!(reached);
    assert_eq!(process.cpu.register(Register32::Ecx), 2);
    assert_eq!(word(&process, MARKER), 2);
    assert_eq!(word(&process, INNER_RECORD + 8), u32::MAX);
    assert_eq!(word(&process, process.cpu.fs_base()), RECORD);
    assert_eq!(process.cpu.register(Register32::Esp), STACK + 12);
}

#[test]
fn unsafe_typed_or_inner_metadata_stops_before_guest_cleanup() {
    for case in 0..8 {
        let mut process = fixture_variant(true);
        match case {
            0 => put(&mut process, CATCHABLE + 4, &[OBJECT]),
            1 => put(&mut process, HANDLERS + 8, &[4]),
            2 => put(&mut process, CATCHABLE + 24, &[CLEANUP]),
            3 => put(&mut process, CATCHABLE + 20, &[8]),
            4 => put(&mut process, INNER_UNWIND + 8, &[0, INNER_ACTION]),
            5 => put(&mut process, INNER_INFO + 12, &[1]),
            6 => put(&mut process, THROW_INFO + 4, &[CLEANUP]),
            _ => process.cpu.set_register(Register32::Ebp, INNER_RECORD + 12),
        }
        let before = process.cpu;
        let result = process.run(1);
        assert_eq!(
            result.reason,
            ProcessStop::UnsupportedApi { address: THROW }
        );
        assert_eq!((result.instructions, result.api_calls), (0, 0));
        assert_eq!(process.cpu, before);
        assert_eq!(word(&process, INNER_RECORD + 8), 1);
        assert_eq!(word(&process, process.cpu.fs_base()), INNER_RECORD);
        assert_eq!(word(&process, MARKER), 0);
    }
}

#[test]
fn outer_transition_faults_preserve_cleanup_and_catch_progress() {
    cxx_exception_cases::outer_transition_faults_preserve_cleanup_and_catch_progress();
}

#[test]
fn inner_completion_faults_leave_every_transition_destination_unchanged() {
    cxx_exception_cases::inner_completion_faults_leave_every_transition_destination_unchanged();
}
