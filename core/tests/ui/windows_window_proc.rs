use super::window_proc_executable;

use ring3_core::execution::{Cpu32, Permissions, Process32, ProcessStop, Register32, StopReason};

const API: u32 = 0x7000_02a4;
const RETURN: u32 = 0x7000_0ff8;
const STACK: u32 = 0x1000_ff00;
const PROCEDURE: u32 = 0x0040_1060;

fn process() -> Process32 {
    Process32::load(&window_proc_executable::guest(), 32).unwrap()
}

fn prepare(p: &mut Process32, procedure: u32) {
    p.cpu.eip = API;
    p.cpu.set_register(Register32::Esp, STACK);
    for (index, value) in [0x0040_1050_u32, procedure, 11, 13, 17, 19]
        .iter()
        .enumerate()
    {
        p.memory
            .write(u64::from(STACK) + index as u64 * 4, &value.to_le_bytes())
            .unwrap();
    }
}

fn put(p: &mut Process32, address: u32, value: u32) {
    p.memory
        .write(u64::from(address), &value.to_le_bytes())
        .unwrap();
}

fn bytes(p: &Process32, address: u32, length: usize) -> Vec<u8> {
    let mut data = vec![0; length];
    p.memory.read(u64::from(address), &mut data).unwrap();
    data
}

fn threaded_process() -> (Process32, u32) {
    let mut p = Process32::load(&window_proc_executable::guest(), 64).unwrap();
    let handle = create_child(&mut p);
    (p, handle)
}

fn create_child(p: &mut Process32) -> u32 {
    for (index, value) in [0x0040_1050, 0, 0, PROCEDURE, 0, 4, 0]
        .into_iter()
        .enumerate()
    {
        put(p, STACK + u32::try_from(index).unwrap() * 4, value);
    }
    p.cpu.eip = 0x7000_0548;
    p.cpu.set_register(Register32::Esp, STACK);
    assert_eq!(p.run(1).api_calls, 1);
    p.cpu.register(Register32::Eax)
}

fn pending_callback(p: &mut Process32, teb: u32, stack: u32) -> Cpu32 {
    p.cpu.set_fs_base(teb);
    p.cpu.eip = API;
    p.cpu.set_register(Register32::Esp, stack);
    for (index, value) in [0x0040_1050, PROCEDURE, 11, 13, 17, 19]
        .into_iter()
        .enumerate()
    {
        put(p, stack + u32::try_from(index).unwrap() * 4, value);
    }
    assert_eq!(p.run(1).api_calls, 1);
    assert_eq!(p.run(5).instructions, 5);
    assert_eq!(p.cpu.eip, RETURN);
    p.cpu
}

#[test]
fn pending_callbacks_finish_in_either_thread_order_after_handle_close() {
    for child_first in [false, true] {
        let (mut p, handle) = threaded_process();
        let primary = pending_callback(&mut p, 0x7ffd_e000, STACK);
        let child = pending_callback(&mut p, 0x1101_0000, 0x1100_ff00);
        p.cpu.eip = 0x7000_021c;
        p.cpu.set_register(Register32::Esp, 0x1000_fe00);
        put(&mut p, 0x1000_fe00, 0x0040_1050);
        put(&mut p, 0x1000_fe04, handle);
        assert_eq!(p.run(1).api_calls, 1);
        for cpu in if child_first {
            [child, primary]
        } else {
            [primary, child]
        } {
            p.cpu = cpu;
            p.memory
                .protect(u64::from(cpu.fs_base()), 4096, Permissions::NONE)
                .unwrap();
            assert_eq!(
                p.run(1).reason,
                ProcessStop::Stopped(StopReason::Breakpoint)
            );
            assert_eq!(
                p.cpu.register(Register32::Esp),
                cpu.register(Register32::Esp) + 24
            );
            assert_eq!(p.cpu.register(Register32::Eax), 60);
        }
    }
}

#[test]
fn foreign_and_unknown_actors_cannot_pop_a_matching_callback_stack() {
    let (mut p, _) = threaded_process();
    let primary = pending_callback(&mut p, 0x7ffd_e000, STACK);
    for teb in [0x1101_0000, 0x5000_0000] {
        p.cpu = primary;
        p.cpu.set_fs_base(teb);
        let before = p.cpu;
        let run = p.run(1);
        assert_eq!(run.reason, ProcessStop::UnsupportedApi { address: RETURN });
        assert_eq!((run.instructions, run.api_calls), (0, 0));
        assert_eq!(p.cpu, before);
    }
    p.cpu = primary;
    assert_eq!(
        p.run(1).reason,
        ProcessStop::Stopped(StopReason::Breakpoint)
    );
    prepare(&mut p, PROCEDURE);
    p.cpu.set_fs_base(0x5000_0000);
    let before = p.cpu;
    let saved = bytes(&p, STACK - 20, 20);
    assert_eq!(
        p.run(1).reason,
        ProcessStop::UnsupportedApi { address: API }
    );
    assert_eq!(p.cpu, before);
    assert_eq!(bytes(&p, STACK - 20, 20), saved);
}

#[test]
fn imported_procedure_executes_guest_arguments_and_returns_whole_or_stepwise() {
    let mut final_cpu = None;
    for budget in [1, 100] {
        let mut p = process();
        let mut counts = (0, 0);
        for _ in 0..100 {
            let run = p.run(budget);
            counts.0 += run.instructions;
            counts.1 += run.api_calls;
            if run.reason != ProcessStop::Stopped(StopReason::InstructionLimit) {
                assert_eq!(run.reason, ProcessStop::Stopped(StopReason::Breakpoint));
                break;
            }
        }
        assert_eq!(counts, (12, 1));
        assert_eq!(p.cpu.register(Register32::Eax), 60);
        assert_eq!(p.cpu.register(Register32::Esp), 0x1001_0000);
        if let Some(expected) = final_cpu {
            assert_eq!(p.cpu, expected);
        }
        final_cpu = Some(p.cpu);
    }
}

#[test]
fn a_return_at_the_budget_boundary_waits_for_a_positive_budget() {
    let mut p = process();
    prepare(&mut p, PROCEDURE);
    let run = p.run(1);
    assert_eq!((run.instructions, run.api_calls), (0, 1));
    assert_eq!(p.cpu.eip, PROCEDURE);
    assert_eq!(p.cpu.register(Register32::Esp), STACK - 20);
    let run = p.run(5);
    assert_eq!(
        run.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((run.instructions, run.api_calls), (5, 0));
    assert_eq!(p.cpu.eip, RETURN);
    assert_eq!(p.cpu.register(Register32::Esp), STACK);
    let before = p.cpu;
    assert_eq!(p.run(0).api_calls, 0);
    assert_eq!(p.cpu, before);
    let run = p.run(1);
    assert_eq!(run.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!((run.instructions, run.api_calls), (1, 0));
    assert_eq!(p.cpu.register(Register32::Esp), STACK + 24);
    assert_eq!(p.cpu.register(Register32::Eax), 60);
}

#[test]
fn nested_guest_and_api_calls_keep_lifo_results_and_stepwise_state() {
    let mut final_cpu = None;
    for budget in [1, 100] {
        let mut p = Process32::load(&window_proc_executable::nested(), 32).unwrap();
        let mut counts = (0, 0);
        loop {
            let run = p.run(budget);
            counts.0 += run.instructions;
            counts.1 += run.api_calls;
            if run.reason != ProcessStop::Stopped(StopReason::InstructionLimit) {
                assert_eq!(run.reason, ProcessStop::Stopped(StopReason::Breakpoint));
                break;
            }
            assert!(counts.0 + counts.1 < 100);
        }
        assert_eq!(counts, (23, 3));
        assert_eq!(p.cpu.register(Register32::Eax), 61);
        assert_eq!(p.cpu.register(Register32::Esp), 0x1001_0000);
        assert_eq!(p.last_error().unwrap(), 77);
        if let Some(expected) = final_cpu {
            assert_eq!(p.cpu, expected);
        }
        final_cpu = Some(p.cpu);
    }
}

#[test]
fn setup_reads_the_complete_original_call_frame_before_writing() {
    let mut p = process();
    p.cpu.eip = API;
    p.cpu.set_register(Register32::Esp, 0x1000_fff0);
    let before = p.cpu;
    let frame = bytes(&p, 0x1000_ffdc, 20);
    let run = p.run(1);
    assert!(matches!(
        run.reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!((run.instructions, run.api_calls), (0, 0));
    assert_eq!(p.cpu, before);
    assert_eq!(bytes(&p, 0x1000_ffdc, 20), frame);
}

#[test]
fn setup_preflights_the_entire_callback_frame_and_can_retry() {
    let mut p = process();
    prepare(&mut p, PROCEDURE);
    let frame = bytes(&p, STACK, 24);
    p.memory.write(0x1000_1008, &frame).unwrap();
    p.cpu.set_register(Register32::Esp, 0x1000_1008);
    p.memory
        .protect(0x1000_0000, 4096, Permissions::READ)
        .unwrap();
    let before = p.cpu;
    let frame = bytes(&p, 0x1000_0ff4, 20);
    let run = p.run(1);
    assert!(matches!(
        run.reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!((run.instructions, run.api_calls), (0, 0));
    assert_eq!(p.cpu, before);
    assert_eq!(bytes(&p, 0x1000_0ff4, 20), frame);
    p.memory
        .protect(0x1000_0000, 4096, Permissions::READ_WRITE)
        .unwrap();
    assert_eq!(p.run(1).api_calls, 1);
    assert_eq!(p.cpu.eip, PROCEDURE);
    assert_eq!(p.cpu.register(Register32::Esp), 0x1000_0ff4);
}

#[test]
fn callback_stack_underflow_does_not_wrap_into_guest_memory() {
    let mut p = process();
    prepare(&mut p, PROCEDURE);
    let frame = bytes(&p, STACK, 24);
    p.memory
        .map_zeroed(0, 4096, Permissions::READ_WRITE)
        .unwrap();
    p.memory.write(16, &frame).unwrap();
    p.cpu.set_register(Register32::Esp, 16);
    let before = p.cpu;
    let run = p.run(1);
    assert!(matches!(
        run.reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!((run.instructions, run.api_calls), (0, 0));
    assert_eq!(p.cpu, before);
    assert_eq!(bytes(&p, 16, 24), frame);
}

#[test]
fn invalid_procedure_faults_during_normal_guest_execution() {
    let mut p = process();
    prepare(&mut p, 0xdead_beef);
    assert_eq!(p.run(1).api_calls, 1);
    assert_eq!(p.cpu.eip, 0xdead_beef);
    let before = p.cpu;
    let run = p.run(1);
    assert!(matches!(
        run.reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!((run.instructions, run.api_calls), (0, 0));
    assert_eq!(p.cpu, before);
}

#[test]
fn malformed_callback_cleanup_keeps_the_continuation_for_repair() {
    let mut p = Process32::load(&window_proc_executable::pe32(&[0xc3]), 32).unwrap();
    prepare(&mut p, PROCEDURE);
    let run = p.run(10);
    assert_eq!(run.reason, ProcessStop::UnsupportedApi { address: RETURN });
    assert_eq!((run.instructions, run.api_calls), (1, 1));
    assert_eq!(p.cpu.register(Register32::Esp), STACK - 16);
    let before = p.cpu;
    assert_eq!(
        p.run(1).reason,
        ProcessStop::UnsupportedApi { address: RETURN }
    );
    assert_eq!(p.cpu, before);
    p.cpu.set_register(Register32::Esp, STACK);
    assert_eq!(
        p.run(1).reason,
        ProcessStop::Stopped(StopReason::Breakpoint)
    );
    assert_eq!(p.cpu.register(Register32::Esp), STACK + 24);
}

#[test]
fn saved_return_is_reread_after_callback_writes_and_read_fault_repair() {
    let procedure = [
        0xc7, 0x44, 0x24, 20, 0x51, 0x10, 0x40, 0, 0xbb, 0x78, 0x56, 0x34, 0x12, 0xb8, 99, 0, 0, 0,
        0xc2, 16, 0,
    ];
    let mut p = Process32::load(&window_proc_executable::pe32(&procedure), 32).unwrap();
    prepare(&mut p, PROCEDURE);
    assert_eq!(p.run(1).api_calls, 1);
    assert_eq!(p.run(4).instructions, 4);
    assert_eq!(p.cpu.eip, RETURN);
    assert_eq!(bytes(&p, STACK, 4), 0x0040_1051_u32.to_le_bytes());
    p.memory
        .protect(0x1000_f000, 4096, Permissions::NONE)
        .unwrap();
    let before = p.cpu;
    let run = p.run(1);
    assert!(matches!(
        run.reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!((run.instructions, run.api_calls), (0, 0));
    assert_eq!(p.cpu, before);
    p.memory
        .protect(0x1000_f000, 4096, Permissions::READ_WRITE)
        .unwrap();
    assert_eq!(
        p.run(1).reason,
        ProcessStop::Stopped(StopReason::Breakpoint)
    );
    assert_eq!(p.cpu.eip, 0x0040_1052);
    assert_eq!(p.cpu.register(Register32::Esp), STACK + 24);
    assert_eq!(p.cpu.register(Register32::Eax), 99);
    assert_eq!(p.cpu.register(Register32::Ebx), 0x1234_5678);
}

#[test]
fn private_returns_are_not_callable_or_executable_without_a_continuation() {
    let mut p = process();
    prepare(&mut p, RETURN);
    let before = p.cpu;
    let frame = bytes(&p, STACK - 20, 20);
    assert_eq!(
        p.run(1).reason,
        ProcessStop::UnsupportedApi { address: API }
    );
    assert_eq!(p.cpu, before);
    assert_eq!(bytes(&p, STACK - 20, 20), frame);
    for permissions in [Permissions::NONE, Permissions::READ_EXECUTE] {
        p.memory.protect(0x7000_0000, 4096, permissions).unwrap();
        p.cpu.eip = RETURN;
        let before = p.cpu;
        let run = p.run(1);
        assert_eq!(run.reason, ProcessStop::UnsupportedApi { address: RETURN });
        assert_eq!((run.instructions, run.api_calls), (0, 0));
        assert_eq!(p.cpu, before);
    }
}

#[test]
fn callback_exit_remains_terminal_with_a_pending_return() {
    let procedure = [0x6a, 42, 0xb8, 8, 0, 0, 0x70, 0xff, 0xd0, 0xcc];
    let mut p = Process32::load(&window_proc_executable::pe32(&procedure), 32).unwrap();
    prepare(&mut p, PROCEDURE);
    let run = p.run(100);
    assert_eq!(run.reason, ProcessStop::Exited(42));
    assert_eq!((run.instructions, run.api_calls), (3, 2));
    let before = p.cpu;
    for budget in [0, 1, 100] {
        let run = p.run(budget);
        assert_eq!(run.reason, ProcessStop::Exited(42));
        assert_eq!((run.instructions, run.api_calls), (0, 0));
        assert_eq!(p.cpu, before);
    }
}

#[test]
fn direct_calls_do_not_access_thread_error_fields_or_allocate_guest_pages() {
    let mut p = process();
    let pages = p.memory.mapped_pages();
    for address in [0x7ffd_e000, 0x7000_2000] {
        p.memory.protect(address, 4096, Permissions::NONE).unwrap();
    }
    assert_eq!(
        p.run(100).reason,
        ProcessStop::Stopped(StopReason::Breakpoint)
    );
    assert_eq!(p.cpu.register(Register32::Eax), 60);
    assert_eq!(p.memory.mapped_pages(), pages);
}

#[test]
fn recursion_depth_is_bounded_before_mutation_and_released_on_return() {
    let procedure = [
        0x8b, 0x44, 0x24, 8, 0x85, 0xc0, 0x74, 23, 0x48, 0x6a, 0, 0x6a, 0, 0x50, 0x6a, 0, 0x68,
        0x60, 0x10, 0x40, 0, 0xff, 0x15, 0x60, 0x20, 0x40, 0, 0x40, 0xc2, 16, 0, 0x31, 0xc0, 0xc2,
        16, 0,
    ];
    let mut p = Process32::load(&window_proc_executable::pe32(&procedure), 64).unwrap();
    create_child(&mut p);
    prepare(&mut p, PROCEDURE);
    put(&mut p, STACK + 12, 63);
    let run = p.run(5000);
    assert_eq!(run.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!(run.api_calls, 64);
    assert_eq!(p.cpu.register(Register32::Eax), 63);
    assert_eq!(p.cpu.register(Register32::Esp), STACK + 24);

    prepare(&mut p, PROCEDURE);
    put(&mut p, STACK + 12, 64);
    let mut calls = 0;
    for _ in 0..5000 {
        let before = p.cpu;
        let stack = p.cpu.register(Register32::Esp);
        let frame = bytes(&p, stack - 20, 20);
        let run = p.run(1);
        calls += run.api_calls;
        if run.reason == ProcessStop::Stopped(StopReason::InstructionLimit) {
            continue;
        }
        assert_eq!(run.reason, ProcessStop::UnsupportedApi { address: API });
        assert_eq!((run.instructions, run.api_calls), (0, 0));
        assert_eq!(p.cpu, before);
        assert_eq!(bytes(&p, stack - 20, 20), frame);
        assert_eq!(calls, 64);
        let blocked = p.cpu;
        p.cpu.set_fs_base(0x1101_0000);
        p.cpu.eip = API;
        p.cpu.set_register(Register32::Esp, 0x1100_ff00);
        for (index, value) in [0x0040_1050, PROCEDURE, 0, 0, 0, 0].into_iter().enumerate() {
            put(
                &mut p,
                0x1100_ff00 + u32::try_from(index).unwrap() * 4,
                value,
            );
        }
        assert_eq!(
            p.run(100).reason,
            ProcessStop::Stopped(StopReason::Breakpoint)
        );
        assert_eq!(p.cpu.register(Register32::Eax), 0);
        p.cpu = blocked;
        // model the active procedure returning after the unsupported nested call.
        p.cpu.eip = RETURN;
        p.cpu.set_register(Register32::Esp, stack + 44);
        p.cpu.set_register(Register32::Eax, 0);
        assert_eq!(
            p.run(5000).reason,
            ProcessStop::Stopped(StopReason::Breakpoint)
        );
        assert_eq!(p.cpu.register(Register32::Esp), STACK + 24);
        prepare(&mut p, PROCEDURE);
        assert_eq!(
            p.run(5000).reason,
            ProcessStop::Stopped(StopReason::Breakpoint)
        );
        assert_eq!(p.cpu.register(Register32::Eax), 13);
        return;
    }
    panic!("recursion did not reach the callback limit");
}
