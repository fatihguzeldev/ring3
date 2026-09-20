#[path = "support/imported_executable.rs"]
mod imported_executable;

use ring3_core::execution::{LoadError, Process32, ProcessStop, Register32, StopReason};

fn roundtrip(value: u32) -> Vec<u8> {
    let mut code = vec![0xb8];
    code.extend_from_slice(&value.to_le_bytes());
    code.extend_from_slice(&[
        0x83, 0xc0, 35, 0x50, 0xff, 0x15, 0x60, 0x20, 0x40, 0, 0xff, 0x15, 0x64, 0x20, 0x40, 0,
        0x83, 0xc0, 7, 0x50, 0xff, 0x15, 0x68, 0x20, 0x40, 0, 0x0f, 0x0b,
    ]);
    imported_executable::pe32(
        &code,
        "kErNeL32.DlL",
        &["SetLastError", "GetLastError", "ExitProcess"],
    )
}

#[test]
fn api_roundtrip_preserves_guest_values_and_exits_without_running_following_code() {
    for input in [0_u32, 99, u32::MAX] {
        let mut process = Process32::load(&roundtrip(input), 32).unwrap();
        let initial_stack = process.cpu.register(Register32::Esp);
        let result = process.run(100);
        assert_eq!(result.reason, ProcessStop::Exited(input.wrapping_add(42)));
        assert_eq!(result.instructions, 8);
        assert_eq!(result.api_calls, 3);
        assert_eq!(process.last_error().unwrap(), input.wrapping_add(35));
        assert_eq!(process.cpu.register(Register32::Esp), initial_stack - 8);
        let before = process.cpu;
        let again = process.run(100);
        assert_eq!(again.reason, result.reason);
        assert_eq!((again.instructions, again.api_calls), (0, 0));
        assert_eq!(process.cpu, before);
    }
}

#[test]
fn execution_and_api_work_share_a_resumable_budget() {
    let mut process = Process32::load(&roundtrip(0), 32).unwrap();
    assert_eq!(
        process.run(0).reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    let mut instructions = 0;
    let mut calls = 0;
    for _ in 0..11 {
        let result = process.run(1);
        assert_eq!(result.instructions + result.api_calls, 1);
        instructions += result.instructions;
        calls += result.api_calls;
    }
    assert_eq!((instructions, calls), (8, 3));
    assert_eq!(process.exit_code(), Some(42));
}

#[test]
fn api_stack_fault_preserves_cpu_and_last_error_state() {
    let mut process = Process32::load(&roundtrip(7), 32).unwrap();
    assert_eq!(process.run(4).instructions, 4);
    process.cpu.set_register(Register32::Esp, u32::MAX - 3);
    let before = process.cpu;
    let result = process.run(1);
    assert!(matches!(
        result.reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!((result.instructions, result.api_calls), (0, 0));
    assert_eq!(process.cpu, before);
    assert_eq!(process.last_error().unwrap(), 0);
    assert_eq!(process.exit_code(), None);
}

#[test]
fn unknown_imports_and_reserved_region_collisions_fail_explicitly() {
    for (module, name) in [
        ("KERNEL32.dll", "exitprocess"),
        ("OTHER.dll", "ExitProcess"),
    ] {
        let bytes = imported_executable::pe32(&[0xcc], module, &[name]);
        assert!(matches!(
            Process32::load(&bytes, 32),
            Err(LoadError::UnresolvedImport { .. })
        ));
    }
    for base in [0x1000_0000_u32, 0x7000_0000] {
        let mut bytes = roundtrip(0);
        bytes[0xb4..0xb8].copy_from_slice(&base.to_le_bytes());
        assert!(Process32::load(&bytes, 32).is_err());
    }
}
