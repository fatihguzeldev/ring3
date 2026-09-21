#[path = "support/imported_executable.rs"]
mod imported_executable;
#[path = "support/performance_clock_executable.rs"]
mod performance_clock_executable;

use std::time::Duration;

use ring3_core::execution::{
    ClockError, Cpu32, PAGE_SIZE, Permissions, Process32, ProcessStop, Register32, StopReason,
};

const FREQUENCY: u32 = 0x7000_0220;
const COUNTER: u32 = 0x7000_0224;
const STACK: u32 = 0x1000_ff00;
const OUTPUT: u32 = 0x0040_2280;
const ERROR: u32 = 0x7ffd_e034;

fn load() -> Process32 {
    Process32::load(&performance_clock_executable::pe32(), 32).unwrap()
}

fn put(p: &mut Process32, address: u32, value: u32) {
    p.memory
        .write(u64::from(address), &value.to_le_bytes())
        .unwrap();
}

fn value(p: &Process32, address: u32) -> u64 {
    let mut bytes = [0; 8];
    p.memory.read(u64::from(address), &mut bytes).unwrap();
    u64::from_le_bytes(bytes)
}

fn prepare(p: &mut Process32, api: u32, output: u32) -> Cpu32 {
    p.cpu.eip = api;
    p.cpu.set_register(Register32::Esp, STACK);
    p.cpu.set_register(Register32::Eax, 99);
    p.cpu.eflags = 0xced7;
    put(p, STACK, 0x0040_1000);
    put(p, STACK + 4, output);
    p.cpu
}

fn completed(p: &mut Process32, mut expected: Cpu32, target: u32) {
    let result = p.run(1);
    assert_eq!(
        result.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((result.instructions, result.api_calls), (0, 1));
    expected.eip = target;
    expected.set_register(Register32::Esp, STACK + 8);
    expected.set_register(Register32::Eax, 1);
    assert_eq!(p.cpu, expected);
}

fn fault(p: &mut Process32, expected: Cpu32) {
    let result = p.run(1);
    assert!(matches!(
        result.reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!((result.instructions, result.api_calls), (0, 0));
    assert_eq!(p.cpu, expected);
}

#[test]
fn imported_clock_starts_at_zero_with_a_fixed_frequency() {
    let mut p = Process32::load(&performance_clock_executable::pe32(), 32).unwrap();
    let result = p.run(50);
    assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!((result.instructions, result.api_calls), (8, 3));
    assert_eq!(p.cpu.register(Register32::Eax), 1);
    assert_eq!(p.cpu.register(Register32::Ebx), 1);
    let mut bytes = [0; 24];
    p.memory.read(0x0040_2280, &mut bytes).unwrap();
    let mut expected = [0; 24];
    expected[..8].copy_from_slice(&1_000_000_000_u64.to_le_bytes());
    assert_eq!(bytes, expected);
}

#[test]
fn elapsed_input_is_monotonic_bounded_isolated_and_does_not_execute_guest_work() {
    let mut p = load();
    let before = prepare(&mut p, COUNTER, OUTPUT);
    p.memory.write(u64::from(OUTPUT), &[0x55; 8]).unwrap();
    for nanos in [0, 1, 5_000_000_123, i64::MAX as u64] {
        assert_eq!(p.set_elapsed_time(Duration::from_nanos(nanos)), Ok(()));
        assert_eq!(p.set_elapsed_time(Duration::from_nanos(nanos)), Ok(()));
        assert_eq!(p.cpu, before);
        assert_eq!(value(&p, OUTPUT), 0x5555_5555_5555_5555);
    }
    for (input, error) in [
        (Duration::ZERO, ClockError::WentBackwards),
        (
            Duration::from_nanos(i64::MAX as u64 + 1),
            ClockError::OutOfRange,
        ),
        (Duration::MAX, ClockError::OutOfRange),
    ] {
        assert_eq!(p.set_elapsed_time(input), Err(error));
        assert_eq!(p.cpu, before);
        assert_eq!(value(&p, OUTPUT), 0x5555_5555_5555_5555);
    }
    completed(&mut p, before, 0x0040_1000);
    assert_eq!(value(&p, OUTPUT), i64::MAX as u64);
    let mut other = load();
    let before = prepare(&mut other, COUNTER, OUTPUT);
    completed(&mut other, before, 0x0040_1000);
    assert_eq!(value(&other, OUTPUT), 0);
}

#[test]
fn queries_write_exact_large_integers_and_preserve_the_calling_convention() {
    let mut p = load();
    for nanos in [0, 5_000_000_123, 9_000_000_987] {
        p.set_elapsed_time(Duration::from_nanos(nanos)).unwrap();
        for (api, expected) in [(FREQUENCY, 1_000_000_000), (COUNTER, nanos)] {
            p.memory.write(u64::from(OUTPUT - 1), &[0x55; 10]).unwrap();
            put(&mut p, ERROR, 77);
            let before = prepare(&mut p, api, OUTPUT);
            completed(&mut p, before, 0x0040_1000);
            assert_eq!(value(&p, OUTPUT), expected);
            let mut bytes = [0; 10];
            p.memory.read(u64::from(OUTPUT - 1), &mut bytes).unwrap();
            assert_eq!((bytes[0], bytes[9]), (0x55, 0x55));
            assert_eq!(p.last_error().unwrap(), 77);
        }
    }
}

#[test]
fn outputs_need_only_write_access_and_may_cross_pages_or_end_at_guest32_limit() {
    for api in [FREQUENCY, COUNTER] {
        let mut p = load();
        p.set_elapsed_time(Duration::from_nanos(5_000_000_123))
            .unwrap();
        p.memory
            .map_zeroed(0x0040_3000, PAGE_SIZE, Permissions::READ_WRITE)
            .unwrap();
        p.memory
            .map_zeroed(0xffff_f000, PAGE_SIZE, Permissions::READ_WRITE)
            .unwrap();
        p.memory
            .protect(0x7ffd_e000, PAGE_SIZE, Permissions::NONE)
            .unwrap();
        for output in [OUTPUT + 1, 0x0040_2ffd, u32::MAX - 7] {
            let before = prepare(&mut p, api, output);
            completed(&mut p, before, 0x0040_1000);
            assert_eq!(
                value(&p, output),
                if api == FREQUENCY {
                    1_000_000_000
                } else {
                    5_000_000_123
                }
            );
        }
        p.memory
            .protect(
                0xffff_f000,
                PAGE_SIZE,
                Permissions {
                    write: true,
                    ..Permissions::NONE
                },
            )
            .unwrap();
        let before = prepare(&mut p, api, u32::MAX - 7);
        completed(&mut p, before, 0x0040_1000);
    }
}

#[test]
fn frame_and_output_faults_are_atomic_including_host_mappings_above_guest32() {
    for api in [FREQUENCY, COUNTER] {
        let mut p = load();
        p.set_elapsed_time(Duration::from_nanos(123)).unwrap();
        p.memory
            .map_zeroed(0xffff_f000, 2 * PAGE_SIZE, Permissions::READ_WRITE)
            .unwrap();
        p.memory.write(0xffff_fffc, &[0x55; 8]).unwrap();
        put(&mut p, 0x0040_2ffc, 0x5555_5555);
        for output in [0, 0x0040_1000, 0x0040_2ffc, u32::MAX - 3] {
            let before = prepare(&mut p, api, output);
            fault(&mut p, before);
            assert_eq!(value(&p, u32::MAX - 3), 0x5555_5555_5555_5555);
            let mut tail = [0; 4];
            p.memory.read(0x0040_2ffc, &mut tail).unwrap();
            assert_eq!(tail, [0x55; 4]);
        }
        for stack in [0x1000_fffc, u32::MAX - 3] {
            prepare(&mut p, api, OUTPUT);
            p.cpu.set_register(Register32::Esp, stack);
            p.memory.write(u64::from(OUTPUT), &[0x55; 8]).unwrap();
            let before = p.cpu;
            fault(&mut p, before);
            assert_eq!(value(&p, OUTPUT), 0x5555_5555_5555_5555);
        }
        let before = prepare(&mut p, COUNTER, OUTPUT);
        completed(&mut p, before, 0x0040_1000);
        assert_eq!(value(&p, OUTPUT), 123);
    }
}

#[test]
fn aliases_use_captured_arguments_and_reread_the_written_return_slot() {
    for (api, expected) in [(FREQUENCY, 1_000_000_000_u64), (COUNTER, 5_000_000_123)] {
        let mut p = load();
        p.set_elapsed_time(Duration::from_nanos(5_000_000_123))
            .unwrap();
        for output in [STACK, STACK + 4, ERROR] {
            let before = prepare(&mut p, api, output);
            completed(
                &mut p,
                before,
                if output == STACK {
                    u32::try_from(expected & 0xffff_ffff).unwrap()
                } else {
                    0x0040_1000
                },
            );
            assert_eq!(value(&p, output), expected);
        }
        let before = prepare(&mut p, api, 0);
        let result = p.run(0);
        assert_eq!((result.instructions, result.api_calls), (0, 0));
        assert_eq!(p.cpu, before);
    }
}

#[test]
fn manual_time_is_identical_for_whole_and_single_step_guest_execution() {
    for budget in [1, 50] {
        let mut p = load();
        p.set_elapsed_time(Duration::from_nanos(5_000_000_123))
            .unwrap();
        let (mut instructions, mut calls) = (0, 0);
        loop {
            let result = p.run(budget);
            instructions += result.instructions;
            calls += result.api_calls;
            if result.reason != ProcessStop::Stopped(StopReason::InstructionLimit) {
                assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
                break;
            }
            assert!(instructions + calls < 50);
        }
        assert_eq!((instructions, calls), (8, 3));
        assert_eq!(p.cpu.register(Register32::Esp), 0x1001_0000);
        assert_eq!(value(&p, OUTPUT), 1_000_000_000);
        assert_eq!(value(&p, OUTPUT + 8), 5_000_000_123);
        assert_eq!(value(&p, OUTPUT + 16), 5_000_000_123);
    }
}
