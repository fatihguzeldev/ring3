use super::window_creation_executable;
use super::window_message_cases;

use ring3_core::execution::{Permissions, Process32, ProcessStop, Register32, StopReason};
use window_message_cases::{PROCEDURE, SEND, STACK, WINDOW, call, created, prepare};

fn unchanged_stop(p: &mut Process32) -> ProcessStop {
    let cpu = p.cpu;
    let result = p.run(1);
    assert_eq!((result.instructions, result.api_calls), (0, 0));
    assert_eq!(p.cpu, cpu);
    result.reason
}

#[test]
fn sends_execute_the_current_procedure_with_real_arguments_and_results() {
    window_message_cases::verify();
}

#[test]
fn default_application_activation_returns_through_guest_procedures() {
    window_message_cases::verify_default_activation();
}

#[test]
fn default_activation_keeps_invalid_window_and_unknown_message_boundaries() {
    let mut p = created();
    for hwnd in [0, 123, WINDOW + 4] {
        assert_eq!(call(&mut p, 0x7000_02ac, &[hwnd, 0x1c, 1, 0]), 0);
        assert_eq!(p.last_error().unwrap(), 1400);
    }
    let windows = p.window_snapshots();
    for message in [0x1b, 0x1d, 0x1_001c] {
        prepare(&mut p, 0x7000_02ac, &[WINDOW, message, 1, 0]);
        assert_eq!(
            unchanged_stop(&mut p),
            ProcessStop::UnsupportedApi {
                address: 0x7000_02ac
            }
        );
        assert_eq!(p.window_snapshots(), windows);
        assert_eq!(p.last_error().unwrap(), 1400);
    }
    p.memory
        .protect(0x7ffd_e000, 4096, Permissions::READ)
        .unwrap();
    assert_eq!(call(&mut p, 0x7000_02ac, &[WINDOW, 0x1c, 0, u32::MAX]), 0);
    prepare(&mut p, 0x7000_02ac, &[0, 0x1c, 1, 0]);
    assert!(matches!(
        unchanged_stop(&mut p),
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
}

#[test]
fn invalid_and_stale_windows_fail_without_dispatch_and_error_writes_are_checked() {
    let mut p = created();
    for handle in [0, 123, WINDOW + 4] {
        assert_eq!(call(&mut p, SEND, &[handle, 0x400, 10, 32]), 0);
        assert_eq!(p.last_error().unwrap(), 1400);
    }
    p.memory
        .protect(0x7ffd_e000, 4096, Permissions::READ)
        .unwrap();
    prepare(&mut p, SEND, &[0, 0x400, 10, 32]);
    assert!(matches!(
        unchanged_stop(&mut p),
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    let mut data = [0; 20];
    p.memory.read(0x0040_2300, &mut data).unwrap();
    assert_eq!(data, [0; 20]);
    let bytes = window_creation_executable::pe32(&[0x31, 0xc0, 0xc2, 16, 0]);
    let mut stale = Process32::load(&bytes, 32).unwrap();
    assert_eq!(
        stale.run(200).reason,
        ProcessStop::Stopped(StopReason::Breakpoint)
    );
    assert_eq!(call(&mut stale, SEND, &[WINDOW, 0x400, 10, 32]), 0);
    assert_eq!(stale.last_error().unwrap(), 1400);
}

#[test]
fn unsupported_targets_and_default_icon_profiles_have_no_effects() {
    let mut p = created();
    call(&mut p, 0x7000_0000, &[77]);
    for handle in [1, 0xffff, u32::MAX] {
        prepare(&mut p, SEND, &[handle, 0x400, 10, 32]);
        assert_eq!(
            unchanged_stop(&mut p),
            ProcessStop::UnsupportedApi { address: SEND }
        );
    }
    for args in [
        [WINDOW, 0x80, 1, 0x7800_0004],
        [WINDOW, 0x80, 2, 0],
        [WINDOW, 0x7f, 2, 0],
        [WINDOW, 0x7f, 0, 96],
    ] {
        prepare(&mut p, 0x7000_02ac, &args);
        assert_eq!(
            unchanged_stop(&mut p),
            ProcessStop::UnsupportedApi {
                address: 0x7000_02ac
            }
        );
    }
    for slot in [0, 1] {
        assert_eq!(call(&mut p, 0x7000_02ac, &[WINDOW, 0x7f, slot, 0]), 0);
    }
    assert_eq!(p.last_error().unwrap(), 77);
}

#[test]
fn input_frame_and_callback_stack_faults_precede_guest_effects() {
    let mut p = created();
    prepare(&mut p, SEND, &[WINDOW, 0x400, 10, 32]);
    let cpu = p.cpu;
    assert_eq!(p.run(0).api_calls, 0);
    assert_eq!(p.cpu, cpu);
    p.cpu.set_register(Register32::Esp, 0x1000_fff0);
    assert!(matches!(
        unchanged_stop(&mut p),
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    p.cpu = cpu;
    p.memory
        .protect(0x1000_e000, 4096, Permissions::READ)
        .unwrap();
    assert!(matches!(
        unchanged_stop(&mut p),
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    p.memory
        .protect(0x1000_e000, 4096, Permissions::READ_WRITE)
        .unwrap();
    assert_eq!(
        p.run(100).reason,
        ProcessStop::Stopped(StopReason::Breakpoint)
    );
    assert_eq!(p.cpu.register(Register32::Eax), 42);
}

#[test]
fn saved_return_fault_retries_without_replaying_completed_message() {
    let mut p = created();
    prepare(&mut p, SEND, &[WINDOW, 0x400, 10, 32]);
    for _ in 0..100 {
        if p.cpu.eip == 0x7000_0ff8 {
            break;
        }
        assert_eq!(
            p.run(1).reason,
            ProcessStop::Stopped(StopReason::InstructionLimit)
        );
    }
    assert_eq!(p.cpu.eip, 0x7000_0ff8);
    assert_eq!(p.cpu.register(Register32::Eax), 42);
    p.memory
        .protect(0x1000_e000, 4096, Permissions::NONE)
        .unwrap();
    assert!(matches!(
        unchanged_stop(&mut p),
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    p.memory
        .protect(0x1000_e000, 4096, Permissions::READ_WRITE)
        .unwrap();
    assert_eq!(
        p.run(100).reason,
        ProcessStop::Stopped(StopReason::Breakpoint)
    );
    assert_eq!(p.cpu.register(Register32::Esp), STACK + 20);
    let mut count = [0; 4];
    p.memory.read(0x0040_2310, &mut count).unwrap();
    assert_eq!(u32::from_le_bytes(count), 1);
}

#[test]
fn unmapped_window_procedure_faults_only_after_dispatch() {
    let mut p = created();
    call(
        &mut p,
        0x7000_02c0,
        &[WINDOW, (-4_i32).cast_unsigned(), 0xdead_beef],
    );
    prepare(&mut p, SEND, &[WINDOW, 0x400, 10, 32]);
    assert_eq!(p.run(1).api_calls, 1);
    assert_eq!(p.cpu.eip, 0xdead_beef);
    assert!(matches!(
        unchanged_stop(&mut p),
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
}

#[test]
fn recursive_sends_share_the_existing_callback_depth_limit() {
    let mut p = created();
    let mut code = [0xff, 0x74, 0x24, 16].repeat(4);
    code.push(0xb8);
    code.extend_from_slice(&SEND.to_le_bytes());
    code.extend_from_slice(&[0xff, 0xd0, 0xc2, 16, 0]);
    p.memory
        .protect(0x0040_1000, 4096, Permissions::READ_WRITE)
        .unwrap();
    p.memory.write(u64::from(PROCEDURE), &code).unwrap();
    p.memory
        .protect(0x0040_1000, 4096, Permissions::READ_EXECUTE)
        .unwrap();
    prepare(&mut p, SEND, &[WINDOW, 0x400, 10, 32]);
    let result = p.run(1000);
    assert_eq!(result.reason, ProcessStop::UnsupportedApi { address: SEND });
    assert_eq!(result.api_calls, 64);
    assert_eq!(unchanged_stop(&mut p), result.reason);
}
