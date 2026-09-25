use super::environment_executable;

use ring3_core::execution::{Process32, ProcessOptions, ProcessStop, Register32, StopReason};

#[test]
fn imported_environment_query_runs_whole_or_one_step_at_a_time() {
    for budget in [1, 50] {
        let mut p = Process32::load_with_options(
            &environment_executable::pe32(),
            32,
            ProcessOptions {
                environment: &[b"Demo=value"],
                ..ProcessOptions::default()
            },
        )
        .unwrap();
        let mut counts = (0, 0);
        loop {
            let result = p.run(budget);
            counts.0 += result.instructions;
            counts.1 += result.api_calls;
            if result.reason != ProcessStop::Stopped(StopReason::InstructionLimit) {
                assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
                break;
            }
            assert!(counts.0 + counts.1 < 50);
        }
        assert_eq!(counts, (5, 1));
        assert_eq!(p.cpu.register(Register32::Eax), 5);
        assert_eq!(p.cpu.register(Register32::Esp), 0x1001_0000);
        let mut bytes = [0; 6];
        p.memory.read(0x0040_2280, &mut bytes).unwrap();
        assert_eq!(&bytes, b"value\0");
    }
}

use ring3_core::execution::{Cpu32, PAGE_SIZE, Permissions};
const API: u32 = 0x7000_0240;
const STACK: u32 = 0x1000_ff00;
const SOURCE: u32 = 0x0040_2400;
const OUTPUT: u32 = 0x3000_0000;

fn load(environment: &[&[u8]]) -> Process32 {
    let mut p = Process32::load_with_options(
        &environment_executable::pe32(),
        128,
        ProcessOptions {
            environment,
            ..ProcessOptions::default()
        },
    )
    .unwrap();
    p.memory
        .map_zeroed(u64::from(OUTPUT), 9 * PAGE_SIZE, Permissions::READ_WRITE)
        .unwrap();
    p.memory.write(u64::from(SOURCE), b"demo\0").unwrap();
    p.memory.write(0x7ffd_e034, &77_u32.to_le_bytes()).unwrap();
    p
}

fn prepare(p: &mut Process32, source: u32, output: u32, capacity: u32) -> Cpu32 {
    p.cpu.eip = API;
    p.cpu.set_register(Register32::Esp, STACK);
    p.cpu.set_register(Register32::Eax, 99);
    p.cpu.eflags = 0xced7;
    for (i, value) in [0x0040_1000_u32, source, output, capacity]
        .into_iter()
        .enumerate()
    {
        p.memory
            .write(u64::from(STACK) + i as u64 * 4, &value.to_le_bytes())
            .unwrap();
    }
    p.cpu
}

fn complete(p: &mut Process32, mut before: Cpu32, result: u32, target: u32) {
    let run = p.run(1);
    assert_eq!(
        run.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((run.instructions, run.api_calls), (0, 1));
    before.eip = target;
    before.set_register(Register32::Esp, STACK + 16);
    before.set_register(Register32::Eax, result);
    assert_eq!(p.cpu, before);
}

fn bytes(p: &Process32, address: u32, size: usize) -> Vec<u8> {
    let mut result = vec![0; size];
    p.memory.read(u64::from(address), &mut result).unwrap();
    result
}

#[test]
fn environment_is_owned_isolated_and_independent_of_crt_mutation() {
    let mut entry = b"DeMo=a=b\xff".to_vec();
    let mut p = load(&[&entry, b"DEMO=second", b"=C:=hidden", b"=empty-key"]);
    entry.fill(b'?');
    drop(entry);
    p.memory.write(0x7000_4000, &[b'?'; 4096]).unwrap();
    p.memory.write(0x7000_2018, &0_u32.to_le_bytes()).unwrap();
    let before = prepare(&mut p, SOURCE, OUTPUT, 16);
    complete(&mut p, before, 4, 0x0040_1000);
    assert_eq!(bytes(&p, OUTPUT, 5), b"a=b\xff\0");
    assert_eq!(p.last_error().unwrap(), 77);
    let mut other = load(&[]);
    let before = prepare(&mut other, SOURCE, u32::MAX, u32::MAX);
    complete(&mut other, before, 0, 0x0040_1000);
    assert_eq!(other.last_error().unwrap(), 203);
}

#[test]
fn sizing_exact_copy_and_empty_values_preserve_last_error() {
    for (entry, expected) in [
        (b"Demo=value".as_slice(), b"value\0".as_slice()),
        (b"demo=", b"\0"),
    ] {
        let mut p = load(&[entry]);
        let required = u32::try_from(expected.len()).unwrap();
        for capacity in [0, required - 1, required, u32::MAX] {
            p.memory.write(u64::from(OUTPUT), &[0x55; 16]).unwrap();
            let output = if capacity < required {
                u32::MAX
            } else {
                OUTPUT
            };
            let before = prepare(&mut p, SOURCE, output, capacity);
            complete(
                &mut p,
                before,
                if capacity < required {
                    required
                } else {
                    required - 1
                },
                0x0040_1000,
            );
            let mut want = [0x55; 16];
            if capacity >= required {
                want[..expected.len()].copy_from_slice(expected);
            }
            assert_eq!(bytes(&p, OUTPUT, 16), want);
            assert_eq!(p.last_error().unwrap(), 77);
        }
    }
    let mut p = load(&[b"=not-an-empty-variable"]);
    p.memory.write(u64::from(SOURCE), &[0]).unwrap();
    let before = prepare(&mut p, SOURCE, u32::MAX, 0);
    complete(&mut p, before, 0, 0x0040_1000);
    assert_eq!(p.last_error().unwrap(), 203);
}

#[test]
fn aliases_capture_names_arguments_and_reread_return_addresses() {
    let mut p = load(&[b"demo=ABCD"]);
    for output in [SOURCE, STACK, STACK + 4, STACK + 8, STACK + 12, 0x7ffd_e034] {
        p.memory.write(u64::from(SOURCE), b"demo\0").unwrap();
        let before = prepare(&mut p, SOURCE, output, 5);
        let target = if output == STACK {
            0x4443_4241
        } else {
            0x0040_1000
        };
        complete(&mut p, before, 4, target);
        assert_eq!(bytes(&p, output, 5), b"ABCD\0");
    }
}

#[test]
fn faults_and_unsupported_inputs_do_not_publish_partial_results() {
    for (source, output, protect) in [
        (0x6000_0000, OUTPUT, false),
        (SOURCE, OUTPUT + 4094, true),
        (SOURCE, u32::MAX - 2, false),
    ] {
        let mut p = load(&[b"demo=value"]);
        p.memory.write(u64::from(OUTPUT), &[0x55; 4096]).unwrap();
        if protect {
            p.memory
                .protect(u64::from(OUTPUT + 4096), PAGE_SIZE, Permissions::READ)
                .unwrap();
        }
        let before = prepare(&mut p, source, output, 6);
        let run = p.run(1);
        assert!(matches!(
            run.reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
        assert_eq!((run.instructions, run.api_calls), (0, 0));
        assert_eq!(p.cpu, before);
        assert_eq!(bytes(&p, OUTPUT, 4096), vec![0x55; 4096]);
        assert_eq!(p.last_error().unwrap(), 77);
    }
    for name in [b"x\xff\0".as_slice(), b"=C:\0", b"a=b\0"] {
        let mut p = load(&[]);
        p.memory.write(u64::from(SOURCE), name).unwrap();
        let before = prepare(&mut p, SOURCE, OUTPUT, 10);
        let run = p.run(1);
        assert_eq!(run.reason, ProcessStop::UnsupportedApi { address: API });
        assert_eq!((run.instructions, run.api_calls), (0, 0));
        assert_eq!(p.cpu, before);
        assert_eq!(p.last_error().unwrap(), 77);
    }
}

#[test]
fn missing_requires_writable_last_error_but_found_and_short_do_not() {
    let mut p = load(&[b"demo=value"]);
    p.memory
        .protect(0x7ffd_e000, PAGE_SIZE, Permissions::READ)
        .unwrap();
    for capacity in [0, 6] {
        let before = prepare(&mut p, SOURCE, OUTPUT, capacity);
        complete(
            &mut p,
            before,
            if capacity == 0 { 6 } else { 5 },
            0x0040_1000,
        );
    }
    p.memory.write(u64::from(SOURCE), b"absent\0").unwrap();
    let before = prepare(&mut p, SOURCE, OUTPUT, 32);
    let output = bytes(&p, OUTPUT, 32);
    let run = p.run(1);
    assert!(matches!(
        run.reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!((run.instructions, run.api_calls), (0, 0));
    assert_eq!(p.cpu, before);
    assert_eq!(bytes(&p, OUTPUT, 32), output);
    assert_eq!(p.last_error().unwrap(), 77);
}

#[test]
fn scan_and_value_limits_are_explicit_and_terminators_are_not_overread() {
    let mut p = load(&[]);
    p.memory
        .map_zeroed(0xffff_f000, PAGE_SIZE, Permissions::READ_WRITE)
        .unwrap();
    for (name, source) in [(b"\0".as_slice(), u32::MAX), (b"x\0", u32::MAX - 1)] {
        p.memory.write(u64::from(source), name).unwrap();
        let before = prepare(&mut p, source, u32::MAX, 0);
        complete(&mut p, before, 0, 0x0040_1000);
    }
    p.memory.write(u64::from(u32::MAX), b"x").unwrap();
    let before = prepare(&mut p, u32::MAX, OUTPUT, 0);
    assert!(matches!(
        p.run(1).reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(p.cpu, before);
    p.memory
        .write(u64::from(OUTPUT), &vec![b'a'; 32768])
        .unwrap();
    let before = prepare(&mut p, OUTPUT, OUTPUT, 0);
    assert_eq!(
        p.run(1).reason,
        ProcessStop::UnsupportedApi { address: API }
    );
    assert_eq!(p.cpu, before);
    p.memory.write(u64::from(OUTPUT + 32767), &[0]).unwrap();
    let before = prepare(&mut p, OUTPUT, OUTPUT, 0);
    complete(&mut p, before, 0, 0x0040_1000);
    for size in [32766, 32767] {
        let mut entry = b"demo=".to_vec();
        entry.extend(std::iter::repeat_n(b'x', size));
        let mut p = load(&[&entry]);
        let before = prepare(&mut p, SOURCE, OUTPUT, u32::MAX);
        if size == 32766 {
            complete(&mut p, before, 32766, 0x0040_1000);
            assert_eq!(bytes(&p, OUTPUT, 32766), vec![b'x'; 32766]);
            assert_eq!(bytes(&p, OUTPUT + 32766, 2), [0, 0]);
        } else {
            assert_eq!(
                p.run(1).reason,
                ProcessStop::UnsupportedApi { address: API }
            );
            assert_eq!(p.cpu, before);
            assert_eq!(bytes(&p, OUTPUT, 32768), vec![0; 32768]);
        }
    }
}

#[test]
fn zero_budget_and_invalid_frames_leave_state_untouched() {
    let mut p = load(&[b"demo=value"]);
    let before = prepare(&mut p, SOURCE, OUTPUT, 6);
    assert_eq!(p.run(0).api_calls, 0);
    assert_eq!(p.cpu, before);
    for stack in [0x1000_fff4, u32::MAX - 11] {
        p.cpu.set_register(Register32::Esp, stack);
        let before = p.cpu;
        let run = p.run(1);
        assert!(matches!(
            run.reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
        assert_eq!((run.instructions, run.api_calls), (0, 0));
        assert_eq!(p.cpu, before);
        assert_eq!(bytes(&p, OUTPUT, 6), vec![0; 6]);
        assert_eq!(p.last_error().unwrap(), 77);
    }
}
