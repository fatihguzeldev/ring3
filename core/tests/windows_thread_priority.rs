#[path = "support/imported_executable.rs"]
mod imported_executable;
#[path = "support/thread_priority_executable.rs"]
mod thread_priority_executable;

use ring3_core::execution::{Cpu32, Permissions, Process32, ProcessStop, Register32, StopReason};

const SET: u32 = 0x7000_0254;
const GET: u32 = 0x7000_0258;
const STACK: u32 = 0x1000_ff00;
const ERROR: u32 = 0x7ffd_e034;
const THREAD: u32 = u32::MAX - 1;

fn load() -> Process32 {
    Process32::load(&thread_priority_executable::pe32(), 32).unwrap()
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
fn imported_thread_priority_runs_whole_or_stepwise() {
    for budget in [1, 100] {
        let mut p = load();
        let mut counts = (0, 0);
        loop {
            let result = p.run(budget);
            counts.0 += result.instructions;
            counts.1 += result.api_calls;
            if result.reason != ProcessStop::Stopped(StopReason::InstructionLimit) {
                assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
                break;
            }
            assert!(counts.0 + counts.1 < 100);
        }
        assert_eq!(counts, (8, 4));
        assert_eq!(p.cpu.register(Register32::Eax), 1);
        assert_eq!(p.cpu.register(Register32::Esp), 0x1001_0000);
    }
}

#[test]
fn all_relative_priorities_persist_without_memory_or_host_dependencies() {
    let mut p = load();
    let mut other = load();
    assert_eq!(call(&mut p, GET, &[THREAD]), 0);
    put(&mut p, ERROR, 77);
    put(&mut p, 0x7000_2020, 88);
    let pages = p.memory.mapped_pages();
    for page in [0x7ffd_e000, 0x7000_2000] {
        p.memory.protect(page, 4096, Permissions::NONE).unwrap();
    }
    for value in [-15_i32, -2, -1, 0, 1, 2, 15, 1] {
        let bits = u32::from_ne_bytes(value.to_ne_bytes());
        assert_eq!(call(&mut p, SET, &[THREAD, bits]), 1);
        assert_eq!(call(&mut p, GET, &[THREAD]), bits);
        assert_eq!(call(&mut other, GET, &[THREAD]), 0);
    }
    for page in [0x7ffd_e000, 0x7000_2000] {
        p.memory
            .protect(page, 4096, Permissions::READ_WRITE)
            .unwrap();
    }
    assert_eq!(p.last_error().unwrap(), 77);
    let mut errno = [0; 4];
    p.memory.read(0x7000_2020, &mut errno).unwrap();
    assert_eq!(errno, 88_u32.to_le_bytes());
    assert_eq!(p.memory.mapped_pages(), pages);
}

#[test]
fn invalid_handles_and_unsupported_values_do_not_mutate_priority() {
    let mut p = load();
    assert_eq!(call(&mut p, SET, &[THREAD, 2]), 1);
    for handle in [0, 1, u32::MAX, 0x7200_0004, 0x7400_0004] {
        assert_eq!(call(&mut p, SET, &[handle, 1]), 0);
        assert_eq!(p.last_error().unwrap(), 6);
        assert_eq!(call(&mut p, GET, &[handle]), 0x7fff_ffff);
        assert_eq!(p.last_error().unwrap(), 6);
        assert_eq!(call(&mut p, GET, &[THREAD]), 2);
    }
    put(&mut p, ERROR, 77);
    for value in [
        3,
        7,
        14,
        16,
        0xffff_fffd,
        0xffff_fff0,
        0x10000,
        0x20000,
        0x8000_0000,
        0x10001,
    ] {
        let before = prepare(&mut p, SET, &[THREAD, value]);
        let result = p.run(1);
        assert_eq!(result.reason, ProcessStop::UnsupportedApi { address: SET });
        assert_eq!((result.instructions, result.api_calls), (0, 0));
        assert_eq!(p.cpu, before);
        assert_eq!(p.last_error().unwrap(), 77);
        assert_eq!(call(&mut p, GET, &[THREAD]), 2);
    }
}

#[test]
fn frame_and_error_faults_and_zero_budgets_preserve_priority() {
    let mut p = load();
    assert_eq!(call(&mut p, SET, &[THREAD, 2]), 1);
    let before = prepare(&mut p, SET, &[THREAD, 1]);
    assert_eq!(p.run(0).api_calls, 0);
    assert_eq!(p.cpu, before);
    p.memory
        .protect(0x7ffd_e000, 4096, Permissions::NONE)
        .unwrap();
    for (api, args) in [(SET, vec![0, 1]), (GET, vec![0])] {
        let before = prepare(&mut p, api, &args);
        let result = p.run(1);
        assert!(matches!(
            result.reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
        assert_eq!(result.api_calls, 0);
        assert_eq!(p.cpu, before);
        assert_eq!(call(&mut p, GET, &[THREAD]), 2);
    }
    for (api, args) in [(SET, vec![THREAD, 1]), (GET, vec![THREAD])] {
        for stack in [0x1000_fffc, u32::MAX - 2] {
            prepare(&mut p, api, &args);
            p.cpu.set_register(Register32::Esp, stack);
            let before = p.cpu;
            assert!(matches!(
                p.run(1).reason,
                ProcessStop::Stopped(StopReason::MemoryFault(_))
            ));
            assert_eq!(p.cpu, before);
            assert_eq!(call(&mut p, GET, &[THREAD]), 2);
        }
    }
    p.memory
        .protect(0x7ffd_e000, 4096, Permissions::READ_WRITE)
        .unwrap();
    prepare(&mut p, SET, &[0, 1]);
    p.cpu.set_register(Register32::Esp, ERROR);
    put(&mut p, ERROR, 0x0040_1000);
    put(&mut p, ERROR + 4, 0);
    put(&mut p, ERROR + 8, 1);
    assert_eq!(p.run(1).api_calls, 1);
    assert_eq!(p.cpu.eip, 6);
    assert_eq!(p.cpu.register(Register32::Esp), ERROR + 12);
    assert_eq!(p.cpu.register(Register32::Eax), 0);
    assert_eq!(call(&mut p, GET, &[THREAD]), 2);
}
