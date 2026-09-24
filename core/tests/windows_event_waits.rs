#[path = "support/event_wait_cases.rs"]
mod event_wait_cases;
#[path = "support/imported_executable.rs"]
mod imported_executable;

#[test]
fn event_handshake_runs_across_host_budgets() {
    event_wait_cases::event_handshake_runs_across_host_budgets();
}

use ring3_core::execution::{Permissions, Process32, ProcessStop, Register32, StopReason};
#[path = "support/event_wait_control.rs"]
mod event_wait_control;
use event_wait_control::*;

#[test]
fn event_aliases_grant_all_or_one_waiter_and_keep_grants_after_reset() {
    for (manual, repeated) in [(true, false), (false, false), (false, true)] {
        let mut p = process();
        p.memory.write(u64::from(DATA), b"ready\0").unwrap();
        let event = call(&mut p, 0x53c, &[0, u32::from(manual), 0, DATA]);
        let alias = call(&mut p, 0x53c, &[0, u32::from(!manual), 1, DATA]);
        let unused = call(&mut p, 0x53c, &[0, 0, 0, DATA]);
        let first_thread = park_child(&mut p, event);
        park_child(&mut p, event);
        assert_eq!(call(&mut p, 0x254, &[CURRENT, 15]), 1);
        denied(&mut p, 0x21c, &[event]);
        assert_eq!(call(&mut p, 0x21c, &[first_thread]), 1);
        assert_eq!(call(&mut p, 0x21c, &[unused]), 1);
        assert_eq!(call(&mut p, 0x540, &[alias]), 1);
        for _ in 0..2 {
            assert_eq!(
                call(&mut p, 0x214, &[alias, 0]),
                if manual { 0 } else { 258 }
            );
        }
        denied(&mut p, 0x21c, &[event]);
        assert_eq!(call(&mut p, 0x544, &[alias]), 1);
        if repeated {
            assert_eq!(call(&mut p, 0x540, &[alias]), 1);
        }
        assert_eq!(call(&mut p, 0x254, &[CURRENT, 0]), 1);
        assert_eq!(
            p.run(100).reason,
            ProcessStop::Stopped(StopReason::Breakpoint)
        );
        assert_eq!(p.cpu.fs_base(), CHILD);
        assert_eq!(p.cpu.register(Register32::Eax), 0);
        assert_eq!(p.cpu.register(Register32::Esp), CHILD - 8);
        assert_eq!(call(&mut p, 0x254, &[CURRENT, (-2_i32).cast_unsigned()]), 1);
        assert_eq!(
            p.run(100).reason,
            ProcessStop::Stopped(StopReason::Breakpoint)
        );
        if !manual && !repeated {
            assert_eq!(p.cpu.fs_base(), PRIMARY);
            denied(&mut p, 0x21c, &[event]);
            assert_eq!(call(&mut p, 0x540, &[alias]), 1);
            assert_eq!(
                p.run(100).reason,
                ProcessStop::Stopped(StopReason::Breakpoint)
            );
        }
        assert_eq!(p.cpu.fs_base(), CHILD + 0x11000);
        assert_eq!(p.cpu.register(Register32::Eax), 0);
        assert_eq!(call(&mut p, 0x254, &[CURRENT, (-2_i32).cast_unsigned()]), 1);
        p.run(1);
        assert_eq!(p.cpu.fs_base(), PRIMARY);
        assert_eq!(call(&mut p, 0x21c, &[event]), 1);
        assert_eq!(call(&mut p, 0x214, &[alias, 0]), 258);
    }
}

#[test]
fn released_wait_retries_its_return_slot_without_retargeting_or_recharging() {
    let mut p = process();
    let event = call(&mut p, 0x53c, &[0, 0, 0, 0]);
    park_child(&mut p, event);
    assert_eq!(call(&mut p, 0x254, &[CURRENT, 15]), 1);
    assert_eq!(call(&mut p, 0x540, &[event]), 1);
    assert_eq!(call(&mut p, 0x544, &[event]), 1);
    let stack = CHILD - 20;
    put(&mut p, stack, CODE + 0x80);
    put(&mut p, stack + 4, 0);
    put(&mut p, stack + 8, 0);
    p.memory
        .protect(u64::from(CHILD - 4096), 4096, Permissions::NONE)
        .unwrap();
    assert_eq!(call(&mut p, 0x254, &[CURRENT, 0]), 1);
    let first = p.run(10000);
    assert!(matches!(
        first.reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!((first.instructions, first.api_calls), (0, 0));
    let before = p.cpu;
    assert_eq!(before.fs_base(), CHILD);
    for _ in 0..2 {
        let run = p.run(10000);
        assert_eq!(run, first);
        assert_eq!(p.cpu, before);
    }
    for changed in [0, 1, 2] {
        p.cpu = before;
        match changed {
            0 => p.cpu.set_fs_base(PRIMARY),
            1 => p.cpu.eip = CODE,
            _ => p.cpu.set_register(Register32::Esp, stack + 4),
        }
        let invalid = p.cpu;
        assert_eq!(p.run(0).instructions, 0);
        assert_eq!(p.cpu, invalid);
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
    p.memory
        .protect(u64::from(CHILD - 4096), 4096, Permissions::READ_WRITE)
        .unwrap();
    let run = p.run(1);
    assert_eq!(run.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!((run.instructions, run.api_calls), (1, 0));
    assert_eq!(p.cpu.eip, CODE + 0x81);
    assert_eq!(p.cpu.register(Register32::Esp), CHILD - 8);
    assert_eq!(p.cpu.register(Register32::Eax), 0);
}

#[test]
fn all_blocked_threads_return_idle_without_reexecuting_the_wait() {
    let mut p = process();
    let event = call(&mut p, 0x53c, &[0, 1, 0, 0]);
    denied(&mut p, 0x214, &[event, u32::MAX]);
    park_child(&mut p, event);
    let valid = prepare(&mut p, 0x214, &[event, u32::MAX]);
    put(&mut p, 0x1000_fffc, CODE);
    p.cpu.set_register(Register32::Esp, 0x1000_fffc);
    let invalid = p.cpu;
    let fault = p.run(1);
    assert!(matches!(
        fault.reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!((fault.instructions, fault.api_calls), (0, 0));
    assert_eq!(p.cpu, invalid);
    p.cpu = valid;
    denied(&mut p, 0x214, &[event, 1]);
    let before = prepare(&mut p, 0x214, &[event, u32::MAX]);
    let run = p.run(1);
    assert_eq!(
        run.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((run.instructions, run.api_calls), (0, 1));
    assert_eq!(p.cpu, before);
    assert_eq!(
        p.run(0).reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    for budget in [1, 4096, 10000] {
        let run = p.run(budget);
        assert_eq!(run.reason, ProcessStop::WaitingForSynchronization);
        assert_eq!((run.instructions, run.api_calls), (0, 0));
        assert_eq!(p.cpu, before);
    }
}

#[path = "support/dll_executable.rs"]
#[allow(dead_code)]
mod dll_executable;

#[test]
fn loader_pinned_wait_refuses_to_park_and_can_retry_when_already_signaled() {
    use ring3_core::execution::{GuestModule, ProcessOptions};
    let mut code = vec![0x83, 0x7c, 0x24, 8, 2, 0x75, 0, 0x6a, 0xff, 0xff, 0x35];
    code.extend_from_slice(&DATA.to_le_bytes());
    code.extend_from_slice(&[0xb8, 0x14, 2, 0, 0x70, 0xff, 0xd0, 0xc2, 12, 0]);
    code[6] = u8::try_from(code.len() - 7).unwrap();
    code.extend_from_slice(&[0xb8, 1, 0, 0, 0, 0xc2, 12, 0]);
    let dll = dll_executable::dll(0x5000_0000, &code, None);
    let mut p = Process32::load_with_options(
        &imported_executable::pe32(&[0xcc], "kernel32.dll", &["WaitForSingleObject"]),
        96,
        ProcessOptions {
            modules: &[GuestModule {
                name: "wait.dll",
                bytes: &dll,
            }],
            ..ProcessOptions::default()
        },
    )
    .unwrap();
    assert_eq!(
        p.run(100).reason,
        ProcessStop::Stopped(StopReason::Breakpoint)
    );
    let event = call(&mut p, 0x53c, &[0, 1, 0, 0]);
    put(&mut p, DATA, event);
    let child = call(&mut p, 0x548, &[0, 0, CODE, event, 4, 0]);
    assert_eq!(call(&mut p, 0x550, &[child]), 1);
    let run = p.run(100);
    assert_eq!(
        run.reason,
        ProcessStop::UnsupportedApi {
            address: 0x7000_0214
        }
    );
    assert_eq!(run.api_calls, 0);
    assert_eq!(p.cpu.fs_base(), CHILD);
    let before = p.cpu;
    for _ in 0..2 {
        let run = p.run(10000);
        assert_eq!(
            run.reason,
            ProcessStop::UnsupportedApi {
                address: before.eip
            }
        );
        assert_eq!((run.instructions, run.api_calls), (0, 0));
        assert_eq!(p.cpu, before);
    }
    assert_eq!(call(&mut p, 0x540, &[event]), 1);
    p.cpu = before;
    let run = p.run(100);
    assert_eq!(run.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!(run.api_calls, 1);
    assert_eq!(p.cpu.fs_base(), CHILD);
}
