use super::imported_executable;
use super::millisecond_clock_executable;

use ring3_core::execution::{
    LoadError, Permissions, Process32, ProcessStop, Register32, StopReason,
};
use std::time::Duration;

const API: u32 = 0x7000_0244;
const STACK: u32 = 0x1000_ff00;

#[test]
fn imported_millisecond_clock_runs_whole_or_stepwise() {
    for budget in [1, 20] {
        let mut p = Process32::load(&millisecond_clock_executable::pe32(), 32).unwrap();
        p.set_elapsed_time(Duration::from_nanos(1_234_999_999))
            .unwrap();
        let mut counts = (0, 0);
        loop {
            let run = p.run(budget);
            counts.0 += run.instructions;
            counts.1 += run.api_calls;
            if run.reason != ProcessStop::Stopped(StopReason::InstructionLimit) {
                assert_eq!(run.reason, ProcessStop::Stopped(StopReason::Breakpoint));
                break;
            }
            assert!(counts.0 + counts.1 < 20);
        }
        assert_eq!(counts, (4, 2));
        assert_eq!(p.cpu.register(Register32::Eax), 1234);
        assert_eq!(p.cpu.register(Register32::Ebx), 1234);
        assert_eq!(p.cpu.register(Register32::Esp), 0x1001_0000);
    }
}

fn query(p: &mut Process32, api: u32, arguments: &[u32], value: u32) {
    p.cpu.eip = api;
    p.cpu.set_register(Register32::Esp, STACK);
    p.cpu.set_register(Register32::Eax, 99);
    p.cpu.eflags = 0xced7;
    for (i, value) in std::iter::once(&0x0040_1000_u32)
        .chain(arguments)
        .enumerate()
    {
        p.memory
            .write(u64::from(STACK) + i as u64 * 4, &value.to_le_bytes())
            .unwrap();
    }
    let mut expected = p.cpu;
    assert_eq!(p.run(0).api_calls, 0);
    assert_eq!(p.cpu, expected);
    let run = p.run(1);
    assert_eq!(
        run.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((run.instructions, run.api_calls), (0, 1));
    expected.eip = 0x0040_1000;
    expected.set_register(
        Register32::Esp,
        STACK + u32::try_from(arguments.len() + 1).unwrap() * 4,
    );
    expected.set_register(Register32::Eax, value);
    assert_eq!(p.cpu, expected);
}

#[test]
fn milliseconds_truncate_wrap_and_share_only_the_owned_clock() {
    let mut p = Process32::load(&millisecond_clock_executable::pe32(), 32).unwrap();
    p.memory.write(0x7ffd_e034, &77_u32.to_le_bytes()).unwrap();
    p.memory
        .protect(0x7ffd_e000, 4096, Permissions::READ)
        .unwrap();
    for (nanos, expected) in [
        (0, 0),
        (999_999, 0),
        (1_000_000, 1),
        (1_999_999, 1),
        (4_294_967_295_999_999, u32::MAX),
        (4_294_967_296_000_000, 0),
        (4_294_967_297_000_000, 1),
    ] {
        p.set_elapsed_time(Duration::from_nanos(nanos)).unwrap();
        query(&mut p, API, &[], expected);
        assert_eq!(p.last_error().unwrap(), 77);
        query(&mut p, 0x7000_0224, &[0x0040_2280], 1);
        let mut bytes = [0; 8];
        p.memory.read(0x0040_2280, &mut bytes).unwrap();
        assert_eq!(u64::from_le_bytes(bytes), nanos);
    }
    let mut other = Process32::load(&millisecond_clock_executable::pe32(), 32).unwrap();
    query(&mut other, API, &[], 0);
}

#[test]
fn no_argument_frame_needs_only_four_bytes_and_bad_frames_do_no_work() {
    let mut p = Process32::load(&millisecond_clock_executable::pe32(), 33).unwrap();
    p.memory
        .map_zeroed(0xffff_f000, 4096, Permissions::READ_WRITE)
        .unwrap();
    p.memory
        .write(0xffff_fffc, &0x0040_1000_u32.to_le_bytes())
        .unwrap();
    p.cpu.eip = API;
    p.cpu.set_register(Register32::Esp, 0xffff_fffc);
    let mut expected = p.cpu;
    assert_eq!(p.run(1).api_calls, 1);
    expected.eip = 0x0040_1000;
    expected.set_register(Register32::Esp, 0);
    expected.set_register(Register32::Eax, 0);
    assert_eq!(p.cpu, expected);
    for stack in [0xffff_fffd, 0x6000_0000] {
        p.cpu.eip = API;
        p.cpu.set_register(Register32::Esp, stack);
        let before = p.cpu;
        let run = p.run(1);
        assert!(matches!(
            run.reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
        assert_eq!((run.instructions, run.api_calls), (0, 0));
        assert_eq!(p.cpu, before);
    }
}

#[test]
fn winmm_has_a_builtin_identity_but_no_unrelated_exports() {
    let mut p = Process32::load(&millisecond_clock_executable::pe32(), 32).unwrap();
    p.memory.write(0x0040_2180, b"WINMM.DLL\0").unwrap();
    query(&mut p, 0x7000_0018, &[0x0040_2180], 0x7000_0814);
    query(&mut p, 0x7000_0014, &[0x0040_2180], 0x7000_0814);
    query(&mut p, 0x7000_001c, &[0x7000_0814], 1);
    let expected = b"C:\\Windows\\System32\\winmm.dll\0";
    query(
        &mut p,
        0x7000_00e0,
        &[0x7000_0814, 0x0040_2280, 64],
        u32::try_from(expected.len() - 1).unwrap(),
    );
    let mut actual = vec![0; expected.len()];
    p.memory.read(0x0040_2280, &mut actual).unwrap();
    assert_eq!(actual, expected);
    for (module, symbol) in [
        ("kernel32.dll", "timeGetTime"),
        ("winmm.dll", "TimeGetTime"),
        ("winmm.dll", "timeBeginPeriod"),
    ] {
        assert!(matches!(
            Process32::load(&imported_executable::pe32(&[0xcc], module, &[symbol]), 32),
            Err(LoadError::UnresolvedImport { .. })
        ));
    }
}
