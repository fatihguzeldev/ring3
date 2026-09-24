#[path = "support/event_executable.rs"]
mod event_executable;
#[path = "support/imported_executable.rs"]
mod imported_executable;

use ring3_core::execution::{
    Cpu32, PAGE_SIZE, Permissions, Process32, ProcessStop, Register32, StopReason,
};

const CREATE: u32 = 0x7000_053c;
const SET: u32 = 0x7000_0540;
const RESET: u32 = 0x7000_0544;
const MUTEX: u32 = 0x7000_0210;
const WAIT: u32 = 0x7000_0214;
const RELEASE: u32 = 0x7000_0218;
const CLOSE: u32 = 0x7000_021c;
const STACK: u32 = 0x1000_ff00;
const NAME: u32 = 0x0040_2200;
const ERROR: u32 = 0x7ffd_e034;

fn load() -> Process32 {
    let bytes = imported_executable::pe32(
        &[0xcc],
        "KERNEL32.dll",
        &[
            "CreateEventA",
            "SetEvent",
            "ResetEvent",
            "WaitForSingleObject",
        ],
    );
    let p = Process32::load(&bytes, 32).unwrap();
    for (index, target) in [CREATE, SET, RESET, WAIT].into_iter().enumerate() {
        let mut word = [0; 4];
        p.memory
            .read(0x0040_2060 + index as u64 * 4, &mut word)
            .unwrap();
        assert_eq!(u32::from_le_bytes(word), target);
    }
    p
}

fn put(p: &mut Process32, address: u32, value: u32) {
    p.memory
        .write(u64::from(address), &value.to_le_bytes())
        .unwrap();
}

fn prepare(p: &mut Process32, api: u32, args: &[u32]) -> Cpu32 {
    p.cpu.eip = api;
    p.cpu.set_register(Register32::Esp, STACK);
    p.cpu.set_register(Register32::Eax, 99);
    p.cpu.eflags = 0xced7;
    for (i, value) in std::iter::once(&0x0040_1000_u32).chain(args).enumerate() {
        put(p, STACK + u32::try_from(i).unwrap() * 4, *value);
    }
    p.cpu
}

fn call(p: &mut Process32, api: u32, args: &[u32]) -> u32 {
    let mut expected = prepare(p, api, args);
    let result = p.run(1);
    assert_eq!(
        result.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((result.instructions, result.api_calls), (0, 1));
    let value = p.cpu.register(Register32::Eax);
    expected.eip = 0x0040_1000;
    expected.set_register(
        Register32::Esp,
        STACK + u32::try_from(args.len() + 1).unwrap() * 4,
    );
    expected.set_register(Register32::Eax, value);
    assert_eq!(p.cpu, expected);
    value
}

#[test]
fn event_state_and_reset_mode_control_zero_timeout_waits() {
    for manual in [0, 1, u32::MAX] {
        for initial in [0, 1, u32::MAX] {
            let mut p = load();
            let handle = call(&mut p, CREATE, &[0, manual, initial, 0]);
            assert_ne!(handle, 0);
            assert_eq!(p.last_error().unwrap(), 0);
            put(&mut p, ERROR, 77);
            assert_eq!(
                call(&mut p, WAIT, &[handle, 0]),
                if initial != 0 { 0 } else { 258 }
            );
            assert_eq!(
                call(&mut p, WAIT, &[handle, 0]),
                if initial != 0 && manual != 0 { 0 } else { 258 }
            );
            assert_eq!(call(&mut p, SET, &[handle]), 1);
            assert_eq!(call(&mut p, SET, &[handle]), 1);
            assert_eq!(call(&mut p, WAIT, &[handle, u32::MAX]), 0);
            assert_eq!(
                call(&mut p, WAIT, &[handle, 0]),
                if manual != 0 { 0 } else { 258 }
            );
            assert_eq!(call(&mut p, RESET, &[handle]), 1);
            assert_eq!(call(&mut p, RESET, &[handle]), 1);
            assert_eq!(call(&mut p, WAIT, &[handle, 0]), 258);
            assert_eq!(p.last_error().unwrap(), 77);
            assert_eq!(call(&mut p, CLOSE, &[handle]), 1);
            assert_eq!(call(&mut p, SET, &[handle]), 0);
            assert_eq!(p.last_error().unwrap(), 6);
        }
    }
}

#[test]
fn reopening_a_named_event_preserves_mode_and_state_until_last_close() {
    let mut p = load();
    p.memory.write(u64::from(NAME), b"shared event\0").unwrap();
    let first = call(&mut p, CREATE, &[0, 1, 0, NAME]);
    let second = call(&mut p, CREATE, &[0, 0, 1, NAME]);
    assert_ne!(first, second);
    assert_eq!(p.last_error().unwrap(), 183);
    assert_eq!(call(&mut p, WAIT, &[second, 0]), 258);
    assert_eq!(call(&mut p, SET, &[first]), 1);
    assert_eq!(call(&mut p, WAIT, &[second, 0]), 0);
    assert_eq!(call(&mut p, CLOSE, &[first]), 1);
    assert_eq!(call(&mut p, WAIT, &[second, 0]), 0);
    assert_eq!(call(&mut p, RESET, &[second]), 1);
    assert_eq!(call(&mut p, CLOSE, &[second]), 1);
    let third = call(&mut p, CREATE, &[0, 0, 1, NAME]);
    assert_eq!(p.last_error().unwrap(), 0);
    assert_eq!(call(&mut p, WAIT, &[third, 0]), 0);
    assert_eq!(call(&mut p, WAIT, &[third, 0]), 258);
}

#[test]
fn event_and_mutex_names_and_handle_types_cannot_alias() {
    let mut p = load();
    p.memory.write(u64::from(NAME), b"object\0").unwrap();
    let event = call(&mut p, CREATE, &[0, 0, 0, NAME]);
    assert_eq!(call(&mut p, MUTEX, &[0, 1, NAME]), 0);
    assert_eq!(p.last_error().unwrap(), 6);
    assert_eq!(call(&mut p, RELEASE, &[event]), 0);
    assert_eq!(p.last_error().unwrap(), 6);
    assert_eq!(call(&mut p, CLOSE, &[event]), 1);
    let mutex = call(&mut p, MUTEX, &[0, 1, NAME]);
    assert_eq!(mutex, event + 4);
    assert_eq!(call(&mut p, CREATE, &[0, 0, 1, NAME]), 0);
    assert_eq!(p.last_error().unwrap(), 6);
    for api in [SET, RESET] {
        assert_eq!(call(&mut p, api, &[mutex]), 0);
        assert_eq!(p.last_error().unwrap(), 6);
    }
    assert_eq!(call(&mut p, RELEASE, &[mutex]), 1);
}

#[test]
fn nonsignaled_blocking_waits_stop_without_changing_state() {
    let mut p = load();
    let handle = call(&mut p, CREATE, &[0, 0, 0, 0]);
    put(&mut p, ERROR, 77);
    for timeout in [1, 1234, u32::MAX] {
        let before = prepare(&mut p, WAIT, &[handle, timeout]);
        let result = p.run(1);
        assert_eq!(result.reason, ProcessStop::UnsupportedApi { address: WAIT });
        assert_eq!((result.instructions, result.api_calls), (0, 0));
        assert_eq!(p.cpu, before);
        assert_eq!(p.last_error().unwrap(), 77);
        assert_eq!(call(&mut p, WAIT, &[handle, 0]), 258);
        assert_eq!(call(&mut p, SET, &[handle]), 1);
        assert_eq!(call(&mut p, WAIT, &[handle, timeout]), 0);
    }
}

#[test]
fn creation_faults_and_incomplete_api_frames_leave_event_state_unchanged() {
    let mut p = load();
    p.memory.write(u64::from(NAME), b"atomic\0").unwrap();
    for (page, permissions) in [
        (0x0040_2000, Permissions::NONE),
        (0x7ffd_e000, Permissions::READ),
    ] {
        p.memory.protect(page, PAGE_SIZE, permissions).unwrap();
        let before = prepare(&mut p, CREATE, &[0, 1, 1, NAME]);
        assert!(matches!(
            p.run(1).reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
        assert_eq!(p.cpu, before);
        p.memory
            .protect(page, PAGE_SIZE, Permissions::READ_WRITE)
            .unwrap();
    }
    let handle = call(&mut p, CREATE, &[0, 1, 0, NAME]);
    assert_eq!(handle, 0x7200_0004);
    assert_eq!(p.last_error().unwrap(), 0);
    for (api, signaled) in [(SET, false), (RESET, true)] {
        prepare(&mut p, api, &[handle]);
        p.cpu.set_register(Register32::Esp, 0x1000_fffc);
        let before = p.cpu;
        assert!(matches!(
            p.run(1).reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
        assert_eq!(p.cpu, before);
        assert_eq!(
            call(&mut p, WAIT, &[handle, 0]),
            if signaled { 0 } else { 258 }
        );
        assert_eq!(call(&mut p, SET, &[handle]), 1);
    }
    p.memory
        .protect(0x7ffd_e000, PAGE_SIZE, Permissions::READ)
        .unwrap();
    let before = prepare(&mut p, CREATE, &[0, 0, 0, NAME]);
    assert!(matches!(
        p.run(1).reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(p.cpu, before);
    assert_eq!(call(&mut p, CLOSE, &[handle]), 1);
    p.memory
        .protect(0x7ffd_e000, PAGE_SIZE, Permissions::READ_WRITE)
        .unwrap();
    assert_eq!(call(&mut p, CREATE, &[0, 0, 0, NAME]), handle + 4);
    assert_eq!(p.last_error().unwrap(), 0);
}

#[test]
fn event_handles_share_the_resource_limit_with_mutexes() {
    let mut p = load();
    p.memory.write(u64::from(NAME), b"bounded event\0").unwrap();
    let event = call(&mut p, CREATE, &[0, 0, 0, NAME]);
    for _ in 1..4096 {
        assert_ne!(call(&mut p, MUTEX, &[0, 0, 0]), 0);
    }
    assert_eq!(call(&mut p, MUTEX, &[0, 0, NAME]), 0);
    assert_eq!(p.last_error().unwrap(), 6);
    assert_eq!(call(&mut p, CREATE, &[0, 0, 1, NAME]), 0);
    assert_eq!(p.last_error().unwrap(), 8);
    assert_eq!(call(&mut p, WAIT, &[event, 0]), 258);
    assert_eq!(call(&mut p, CLOSE, &[event]), 1);
    assert_ne!(call(&mut p, MUTEX, &[0, 0, NAME]), 0);
    assert_eq!(p.last_error().unwrap(), 0);
    assert_eq!(call(&mut p, CREATE, &[0, 0, 0, NAME]), 0);
    assert_eq!(p.last_error().unwrap(), 6);
}

#[test]
fn imported_event_lifecycle_agrees_whole_and_single_step() {
    for manual in [false, true] {
        for budget in [1, 100] {
            let mut p = Process32::load(&event_executable::pe32(manual), 32).unwrap();
            let (mut steps, mut calls) = (0, 0);
            loop {
                let result = p.run(budget);
                steps += result.instructions;
                calls += result.api_calls;
                if result.reason != ProcessStop::Stopped(StopReason::InstructionLimit) {
                    assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
                    break;
                }
                assert!(steps + calls < 100);
            }
            assert_eq!((steps, calls), (28, 8));
            for (register, expected) in [
                (Register32::Eax, 258),
                (Register32::Ebx, 0x7200_0004),
                (Register32::Esi, 258),
                (Register32::Edi, 0),
                (Register32::Ebp, if manual { 0 } else { 258 }),
                (Register32::Esp, 0x1001_0000),
            ] {
                assert_eq!(p.cpu.register(register), expected);
            }
            assert_eq!(p.last_error().unwrap(), 0);
        }
    }
}
