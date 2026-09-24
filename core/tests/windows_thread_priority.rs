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
        STACK
            + if api == 0x7000_0548 {
                4
            } else {
                u32::try_from(args.len() + 1).unwrap() * 4
            },
    );
    expected.set_register(Register32::Eax, value);
    assert_eq!(p.cpu, expected);
    value
}

fn children() -> (Process32, u32, u32) {
    let mut p = Process32::load(&thread_priority_executable::pe32(), 64).unwrap();
    assert_eq!(call(&mut p, SET, &[THREAD, 2]), 1);
    let args = [0, 0, 0x0040_1000, 0, 4, 0];
    let first = call(&mut p, 0x7000_0548, &args);
    let second = call(&mut p, 0x7000_0548, &args);
    (p, first, second)
}

#[test]
fn registered_targets_own_independent_priorities_for_real_and_pseudo_handles() {
    let (mut p, first, second) = children();
    assert_eq!(call(&mut p, GET, &[first]), 0);
    assert_eq!(call(&mut p, GET, &[second]), 0);
    assert_eq!(call(&mut p, SET, &[first, (-2_i32).cast_unsigned()]), 1);
    for (teb, target, value) in [
        (0x7ffd_e000, THREAD, 2),
        (0x1101_0000, first, (-2_i32).cast_unsigned()),
        (0x1102_1000, second, 0),
    ] {
        p.cpu.set_fs_base(teb);
        put(&mut p, teb + 0x24, 1);
        p.memory
            .protect(u64::from(teb), 4096, Permissions::NONE)
            .unwrap();
        assert_eq!(call(&mut p, GET, &[THREAD]), value);
        assert_eq!(call(&mut p, GET, &[target]), value);
        assert_eq!(call(&mut p, SET, &[THREAD, 15]), 1);
        assert_eq!(call(&mut p, GET, &[target]), 15);
        assert_eq!(call(&mut p, SET, &[target, value]), 1);
        p.memory
            .protect(u64::from(teb), 4096, Permissions::READ_WRITE)
            .unwrap();
    }
    p.cpu.set_fs_base(0x1101_0000);
    assert_eq!(call(&mut p, SET, &[second, 1]), 1);
    assert_eq!(call(&mut p, GET, &[THREAD]), (-2_i32).cast_unsigned());
    p.cpu.set_fs_base(0x1102_1000);
    assert_eq!(call(&mut p, GET, &[THREAD]), 1);
}

#[test]
fn closing_a_handle_preserves_priority_but_prevents_stale_target_access() {
    let (mut p, first, second) = children();
    p.cpu.set_fs_base(0x1101_0000);
    assert_eq!(call(&mut p, SET, &[THREAD, 15]), 1);
    assert_eq!(call(&mut p, 0x7000_021c, &[first]), 1);
    let mutex = call(&mut p, 0x7000_0210, &[0, 0, 0]);
    assert_ne!(first, mutex);
    put(&mut p, ERROR, 77);
    put(&mut p, 0x1101_0034, 88);
    p.memory
        .protect(0x1101_0000, 4096, Permissions::READ)
        .unwrap();
    for handle in [first, mutex, 0, 0x7200_fffc] {
        for (api, args, failure) in [(GET, vec![handle], 0x7fff_ffff), (SET, vec![handle, 3], 0)] {
            let before = prepare(&mut p, api, &args);
            let run = p.run(1);
            assert!(matches!(
                run.reason,
                ProcessStop::Stopped(StopReason::MemoryFault(_))
            ));
            assert_eq!((run.instructions, run.api_calls), (0, 0));
            assert_eq!(p.cpu, before);
            assert_eq!(call(&mut p, GET, &[THREAD]), 15);
            assert_eq!(call(&mut p, GET, &[second]), 0);
            p.memory
                .protect(0x1101_0000, 4096, Permissions::READ_WRITE)
                .unwrap();
            assert_eq!(call(&mut p, api, &args), failure);
            assert_eq!(p.last_error().unwrap(), 6);
            let mut primary_error = [0; 4];
            p.memory.read(u64::from(ERROR), &mut primary_error).unwrap();
            assert_eq!(u32::from_le_bytes(primary_error), 77);
            p.memory
                .protect(0x1101_0000, 4096, Permissions::READ)
                .unwrap();
        }
    }
    assert_eq!(call(&mut p, SET, &[THREAD, 1]), 1);
    assert_eq!(call(&mut p, GET, &[THREAD]), 1);
}

#[test]
fn unknown_pseudo_actors_and_unsupported_values_do_not_change_registered_targets() {
    let (mut p, first, second) = children();
    let pages = p.memory.mapped_pages();
    for teb in [0, u32::MAX, 0x1103_2000] {
        p.cpu.set_fs_base(teb);
        for (api, args) in [(GET, vec![THREAD]), (SET, vec![THREAD, 1])] {
            let before = prepare(&mut p, api, &args);
            let run = p.run(1);
            assert_eq!(run.reason, ProcessStop::UnsupportedApi { address: api });
            assert_eq!((run.instructions, run.api_calls), (0, 0));
            assert_eq!(p.cpu, before);
        }
    }
    p.cpu.set_fs_base(0x1101_0000);
    for handle in [THREAD, first, second] {
        let before = prepare(&mut p, SET, &[handle, 3]);
        assert_eq!(
            p.run(1).reason,
            ProcessStop::UnsupportedApi { address: SET }
        );
        assert_eq!(p.cpu, before);
        assert_eq!(call(&mut p, GET, &[handle]), 0);
    }
    p.cpu.set_fs_base(0x7ffd_e000);
    assert_eq!(call(&mut p, GET, &[THREAD]), 2);
    assert_eq!(p.memory.mapped_pages(), pages);
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
