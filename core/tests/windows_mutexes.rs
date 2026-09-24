#[path = "support/imported_executable.rs"]
mod imported_executable;
#[path = "support/mutex_executable.rs"]
mod mutex_executable;

use ring3_core::execution::{
    Cpu32, PAGE_SIZE, Permissions, Process32, ProcessStop, Register32, StopReason,
};

const CREATE: u32 = 0x7000_0210;
const WAIT: u32 = 0x7000_0214;
const RELEASE: u32 = 0x7000_0218;
const CLOSE: u32 = 0x7000_021c;
const STACK: u32 = 0x1000_ff00;
const ERROR: u32 = 0x7ffd_e034;
const FIRST: u32 = 0x7200_0004;

fn load() -> Process32 {
    Process32::load(&mutex_executable::pe32(), 32).unwrap()
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
fn ownership_recursion_release_and_close_have_real_state_and_error_semantics() {
    for initial_owner in [0, 1, u32::MAX] {
        let mut p = load();
        put(&mut p, ERROR, 77);
        let handle = call(&mut p, CREATE, &[0, initial_owner, 0]);
        assert_eq!(handle, FIRST);
        assert_eq!(p.last_error().unwrap(), 0);
        put(&mut p, ERROR, 77);
        for timeout in [0, 123, u32::MAX] {
            assert_eq!(call(&mut p, WAIT, &[handle, timeout]), 0);
        }
        for _ in 0..3 + u32::from(initial_owner != 0) {
            assert_eq!(call(&mut p, RELEASE, &[handle]), 1);
        }
        assert_eq!(p.last_error().unwrap(), 77);
        assert_eq!(call(&mut p, RELEASE, &[handle]), 0);
        assert_eq!(p.last_error().unwrap(), 288);
        assert_eq!(call(&mut p, WAIT, &[handle, 0]), 0);
        put(&mut p, ERROR, 55);
        assert_eq!(call(&mut p, CLOSE, &[handle]), 1);
        assert_eq!(p.last_error().unwrap(), 55);
        assert_eq!(call(&mut p, CLOSE, &[handle]), 0);
        assert_eq!(p.last_error().unwrap(), 6);
        assert_eq!(call(&mut p, WAIT, &[handle, 0]), u32::MAX);
        assert_eq!(call(&mut p, RELEASE, &[handle]), 0);
        assert_eq!(call(&mut p, CREATE, &[0, 0, 0]), FIRST + 4);
    }
}

#[test]
fn process_local_handles_do_not_alias_other_processes_and_limits_free_live_slots() {
    let mut p = load();
    let mut other = load();
    let pages = p.memory.mapped_pages();
    assert_eq!(call(&mut p, CREATE, &[0, 0, 0]), FIRST);
    assert_eq!(call(&mut other, WAIT, &[FIRST, 0]), u32::MAX);
    assert_eq!(call(&mut other, CREATE, &[0, 1, 0]), FIRST);
    for index in 1..4096 {
        assert_eq!(call(&mut p, CREATE, &[0, 0, 0]), FIRST + index * 4);
    }
    assert_eq!(call(&mut p, CREATE, &[0, 0, 0]), 0);
    assert_eq!(p.last_error().unwrap(), 8);
    assert_eq!(call(&mut p, CLOSE, &[FIRST]), 1);
    assert_eq!(call(&mut p, CREATE, &[0, 0, 0]), FIRST + 4096 * 4);
    assert_eq!(call(&mut p, WAIT, &[FIRST, 0]), u32::MAX);
    assert_eq!(call(&mut other, RELEASE, &[FIRST]), 1);
    assert_eq!(p.memory.mapped_pages(), pages);
}

#[test]
fn unsupported_scope_and_invalid_handles_do_not_fabricate_success() {
    let mut p = load();
    for handle in [0, 1, FIRST - 2, 0x7000_0800, 0x6000_0000] {
        for (api, args, expected) in [
            (WAIT, vec![handle, 0], u32::MAX),
            (RELEASE, vec![handle], 0),
            (CLOSE, vec![handle], 0),
        ] {
            assert_eq!(call(&mut p, api, &args), expected);
            assert_eq!(p.last_error().unwrap(), 6);
        }
    }
    p.memory
        .protect(0x7ffd_e000, PAGE_SIZE, Permissions::NONE)
        .unwrap();
    for (api, args) in [
        (CREATE, vec![1, 0, 0]),
        (CREATE, vec![0, 0, 0x0040_2200]),
        (WAIT, vec![u32::MAX, 0]),
        (WAIT, vec![u32::MAX - 1, 0]),
        (CLOSE, vec![u32::MAX]),
        (CLOSE, vec![u32::MAX - 1]),
    ] {
        let before = prepare(&mut p, api, &args);
        let result = p.run(1);
        assert_eq!(result.reason, ProcessStop::UnsupportedApi { address: api });
        assert_eq!((result.instructions, result.api_calls), (0, 0));
        assert_eq!(p.cpu, before);
    }
}

#[test]
fn frame_and_error_faults_preserve_ownership_and_handle_allocation() {
    let mut p = load();
    p.memory
        .protect(0x7ffd_e000, PAGE_SIZE, Permissions::NONE)
        .unwrap();
    let before = prepare(&mut p, CREATE, &[0, 1, 0]);
    let zero = p.run(0);
    assert_eq!((zero.instructions, zero.api_calls), (0, 0));
    assert_eq!(p.cpu, before);
    for (api, args) in [
        (CREATE, vec![0, 1, 0]),
        (WAIT, vec![FIRST, 0]),
        (RELEASE, vec![FIRST]),
        (CLOSE, vec![FIRST]),
    ] {
        let before = prepare(&mut p, api, &args);
        let result = p.run(1);
        assert!(matches!(
            result.reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
        assert_eq!((result.instructions, result.api_calls), (0, 0));
        assert_eq!(p.cpu, before);
    }
    p.memory
        .protect(0x7ffd_e000, PAGE_SIZE, Permissions::READ_WRITE)
        .unwrap();
    assert_eq!(call(&mut p, CREATE, &[0, 1, 0]), FIRST);
    for (api, args) in [
        (CREATE, vec![0, 1, 0]),
        (WAIT, vec![FIRST, 0]),
        (RELEASE, vec![FIRST]),
        (CLOSE, vec![FIRST]),
    ] {
        prepare(&mut p, api, &args);
        p.cpu.set_register(
            Register32::Esp,
            0x1001_0000 - u32::try_from(args.len()).unwrap() * 4,
        );
        let before = p.cpu;
        let result = p.run(1);
        assert!(matches!(
            result.reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
        assert_eq!(result.api_calls, 0);
        assert_eq!(p.cpu, before);
    }
    p.memory
        .protect(0x7ffd_e000, PAGE_SIZE, Permissions::NONE)
        .unwrap();
    assert_eq!(call(&mut p, WAIT, &[FIRST, 0]), 0);
    assert_eq!(call(&mut p, RELEASE, &[FIRST]), 1);
    assert_eq!(call(&mut p, RELEASE, &[FIRST]), 1);
    let before = prepare(&mut p, RELEASE, &[FIRST]);
    let result = p.run(1);
    assert!(matches!(
        result.reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(p.cpu, before);
    assert_eq!(call(&mut p, CLOSE, &[FIRST]), 1);
    p.memory
        .protect(0x7ffd_e000, PAGE_SIZE, Permissions::READ_WRITE)
        .unwrap();
    assert_eq!(call(&mut p, CREATE, &[0, 0, 0]), FIRST + 4);
}

#[test]
fn error_output_can_alias_the_saved_return_slot() {
    let mut p = load();
    prepare(&mut p, CREATE, &[0, 0, 0]);
    for (i, value) in [0x0040_1000, 0, 0, 0].into_iter().enumerate() {
        put(&mut p, ERROR + u32::try_from(i).unwrap() * 4, value);
    }
    p.cpu.set_register(Register32::Esp, ERROR);
    let result = p.run(1);
    assert_eq!(result.api_calls, 1);
    assert_eq!(p.cpu.eip, 0);
    assert_eq!(p.cpu.register(Register32::Eax), FIRST);
    assert_eq!(p.cpu.register(Register32::Esp), ERROR + 16);
    assert_eq!(call(&mut p, CLOSE, &[FIRST]), 1);
}

#[test]
fn imported_lifecycle_agrees_whole_and_single_step() {
    for budget in [1, 50] {
        let mut p = load();
        let (mut steps, mut calls) = (0, 0);
        loop {
            let result = p.run(budget);
            steps += result.instructions;
            calls += result.api_calls;
            if result.reason != ProcessStop::Stopped(StopReason::InstructionLimit) {
                assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
                break;
            }
            assert!(steps + calls < 50);
        }
        assert_eq!((steps, calls), (15, 5));
        assert_eq!(p.cpu.register(Register32::Eax), 1);
        assert_eq!(p.cpu.register(Register32::Ebx), FIRST);
        assert_eq!(p.cpu.register(Register32::Esp), 0x1001_0000);
    }
}

const NAME: u32 = 0x0040_2200;

fn name(p: &mut Process32, bytes: &[u8]) {
    p.memory.write(u64::from(NAME), bytes).unwrap();
}

#[test]
fn named_handles_share_ownership_until_the_last_close() {
    for initial_owner in [0, 1] {
        let mut p = load();
        name(&mut p, b"shared mutex\0");
        let first = call(&mut p, CREATE, &[0, initial_owner, NAME]);
        assert_eq!(first, FIRST);
        assert_eq!(p.last_error().unwrap(), 0);
        let second = call(&mut p, CREATE, &[0, 1, NAME]);
        assert_eq!(second, FIRST + 4);
        assert_eq!(p.last_error().unwrap(), 183);
        assert_eq!(call(&mut p, RELEASE, &[second]), initial_owner);
        assert_eq!(call(&mut p, RELEASE, &[first]), 0);
        assert_eq!(p.last_error().unwrap(), 288);
        assert_eq!(call(&mut p, WAIT, &[first, 0]), 0);
        assert_eq!(call(&mut p, WAIT, &[second, u32::MAX]), 0);
        assert_eq!(call(&mut p, CLOSE, &[first]), 1);
        assert_eq!(call(&mut p, WAIT, &[first, 0]), u32::MAX);
        let third = call(&mut p, CREATE, &[0, 0, NAME]);
        assert_eq!(p.last_error().unwrap(), 183);
        assert_eq!(call(&mut p, RELEASE, &[third]), 1);
        assert_eq!(call(&mut p, RELEASE, &[second]), 1);
        assert_eq!(call(&mut p, RELEASE, &[third]), 0);
        assert_eq!(call(&mut p, CLOSE, &[third]), 1);
        assert_eq!(call(&mut p, CLOSE, &[second]), 1);
        assert_eq!(call(&mut p, CREATE, &[0, 1, NAME]), FIRST + 12);
        assert_eq!(p.last_error().unwrap(), 0);
        assert_eq!(call(&mut p, RELEASE, &[FIRST + 12]), 1);
    }
}

#[test]
fn names_are_owned_case_sensitive_and_process_local() {
    let mut p = load();
    let mut other = load();
    name(&mut p, b"Shared\0");
    name(&mut other, b"Shared\0");
    assert_eq!(call(&mut p, CREATE, &[0, 1, NAME]), FIRST);
    assert_eq!(call(&mut other, CREATE, &[0, 0, NAME]), FIRST);
    assert_eq!(other.last_error().unwrap(), 0);
    assert_eq!(call(&mut other, RELEASE, &[FIRST]), 0);
    name(&mut p, b"shared\0");
    assert_eq!(call(&mut p, CREATE, &[0, 0, NAME]), FIRST + 4);
    assert_eq!(p.last_error().unwrap(), 0);
    assert_eq!(call(&mut p, RELEASE, &[FIRST + 4]), 0);
    name(&mut p, b"Shared\0");
    assert_eq!(call(&mut p, CREATE, &[0, 0, NAME]), FIRST + 8);
    assert_eq!(p.last_error().unwrap(), 183);
    assert_eq!(call(&mut p, RELEASE, &[FIRST + 8]), 1);
}

#[test]
fn named_mutex_reopens_are_bounded_by_live_handle_limit() {
    let mut p = load();
    name(&mut p, b"bounded\0");
    for index in 0..4096 {
        assert_eq!(call(&mut p, CREATE, &[0, 0, NAME]), FIRST + index * 4);
        assert_eq!(p.last_error().unwrap(), if index == 0 { 0 } else { 183 });
    }
    assert_eq!(call(&mut p, CREATE, &[0, 1, NAME]), 0);
    assert_eq!(p.last_error().unwrap(), 8);
    assert_eq!(call(&mut p, RELEASE, &[FIRST]), 0);
    assert_eq!(call(&mut p, CLOSE, &[FIRST]), 1);
    assert_eq!(call(&mut p, CREATE, &[0, 1, NAME]), FIRST + 4096 * 4);
    assert_eq!(p.last_error().unwrap(), 183);
    assert_eq!(call(&mut p, RELEASE, &[FIRST + 4096 * 4]), 0);
}

#[test]
fn unsupported_names_and_memory_faults_do_not_consume_handles() {
    let mut p = load();
    put(&mut p, ERROR, 77);
    for bytes in [
        b"\0".as_slice(),
        b"Global\\lock\0",
        b"Local\\lock\0",
        b"bad\xff\0",
        &[b'a'; 260],
    ] {
        name(&mut p, bytes);
        let before = prepare(&mut p, CREATE, &[0, 0, NAME]);
        assert_eq!(
            p.run(1).reason,
            ProcessStop::UnsupportedApi { address: CREATE }
        );
        assert_eq!(p.cpu, before);
        assert_eq!(p.last_error().unwrap(), 77);
    }
    for pointer in [0x003f_ffff, 0x0040_3000] {
        let before = prepare(&mut p, CREATE, &[0, 0, pointer]);
        assert!(matches!(
            p.run(1).reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
        assert_eq!(p.cpu, before);
        assert_eq!(p.last_error().unwrap(), 77);
    }
    let mut longest = [b'a'; 260];
    longest[259] = 0;
    name(&mut p, &longest);
    assert_eq!(call(&mut p, CREATE, &[0, 1, NAME]), FIRST);
    assert_eq!(p.last_error().unwrap(), 0);
}

#[test]
fn name_and_last_error_faults_preserve_shared_object_lifetime() {
    let mut p = load();
    name(&mut p, b"faults\0");
    p.memory
        .protect(0x7ffd_e000, PAGE_SIZE, Permissions::READ)
        .unwrap();
    let before = prepare(&mut p, CREATE, &[0, 1, NAME]);
    assert!(matches!(
        p.run(1).reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(p.cpu, before);
    p.memory
        .protect(0x7ffd_e000, PAGE_SIZE, Permissions::READ_WRITE)
        .unwrap();
    let first = call(&mut p, CREATE, &[0, 0, NAME]);
    assert_eq!(first, FIRST);
    assert_eq!(p.last_error().unwrap(), 0);
    for page in [0x0040_2000, 0x7ffd_e000] {
        p.memory
            .protect(page, PAGE_SIZE, Permissions::NONE)
            .unwrap();
        let before = prepare(&mut p, CREATE, &[0, 1, NAME]);
        assert!(matches!(
            p.run(1).reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
        assert_eq!(p.cpu, before);
        p.memory
            .protect(page, PAGE_SIZE, Permissions::READ_WRITE)
            .unwrap();
    }
    assert_eq!(call(&mut p, RELEASE, &[first]), 0);
    assert_eq!(call(&mut p, CLOSE, &[first]), 1);
    assert_eq!(call(&mut p, CREATE, &[0, 0, NAME]), FIRST + 4);
    assert_eq!(p.last_error().unwrap(), 0);
}

#[test]
fn named_imported_lifecycle_agrees_whole_and_single_step() {
    for budget in [1, 100] {
        let mut p = Process32::load(&mutex_executable::named_pe32(), 32).unwrap();
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
        assert_eq!((steps, calls), (32, 10));
        for (register, expected) in [
            (Register32::Eax, 0),
            (Register32::Ebx, FIRST),
            (Register32::Esi, FIRST + 4),
            (Register32::Edi, FIRST + 8),
            (Register32::Ebp, 1),
            (Register32::Esp, 0x1001_0000),
        ] {
            assert_eq!(p.cpu.register(register), expected);
        }
        assert_eq!(p.last_error().unwrap(), 288);
    }
}
