use super::imported_executable;
use super::random_executable;

use ring3_core::execution::{
    LoadError, Permissions, Process32, ProcessStop, Register32, StopReason,
};

const SEED: u32 = 0x7000_0160;
const RANDOM: u32 = 0x7000_0164;
const STACK: u32 = 0x1000_ff00;

#[test]
fn imported_random_sequence_runs_whole_or_stepwise() {
    for budget in [1, 40] {
        let mut p = Process32::load(&random_executable::pe32(), 32).unwrap();
        let mut counts = (0, 0);
        loop {
            let run = p.run(budget);
            counts.0 += run.instructions;
            counts.1 += run.api_calls;
            if run.reason != ProcessStop::Stopped(StopReason::InstructionLimit) {
                assert_eq!(run.reason, ProcessStop::Stopped(StopReason::Breakpoint));
                break;
            }
            assert!(counts.0 + counts.1 < 40);
        }
        assert_eq!(counts, (9, 4));
        assert_eq!(p.cpu.register(Register32::Ebx), 41);
        assert_eq!(p.cpu.register(Register32::Esi), 5890);
        assert_eq!(p.cpu.register(Register32::Eax), 1279);
        assert_eq!(p.cpu.register(Register32::Esp), 0x1001_0000);
    }
}

fn prepare(p: &mut Process32, api: u32, arguments: &[u32]) {
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
}

fn invoke(p: &mut Process32, api: u32, arguments: &[u32], result: u32) {
    prepare(p, api, arguments);
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
    expected.set_register(Register32::Esp, STACK + 4);
    expected.set_register(Register32::Eax, result);
    assert_eq!(p.cpu, expected);
}

#[test]
fn published_sequence_default_reseed_and_unsigned_seeds_preserve_error_cells() {
    let mut p = Process32::load(&random_executable::pe32(), 32).unwrap();
    p.memory.write(0x7000_2020, &123_u32.to_le_bytes()).unwrap();
    p.memory.write(0x7ffd_e034, &77_u32.to_le_bytes()).unwrap();
    for page in [0x7000_2000, 0x7ffd_e000] {
        p.memory.protect(page, 4096, Permissions::READ).unwrap();
    }
    let default = [41, 18467, 6334, 26500, 19169, 15724];
    for value in default {
        invoke(&mut p, RANDOM, &[], value);
    }
    for (seed, values) in [
        (0, &[38, 7719, 21238, 2437, 8855, 11797][..]),
        (1, &default[..]),
        (
            1792,
            &[
                5890, 1279, 19497, 1207, 11420, 3377, 15317, 29489, 9716, 23323,
            ][..],
        ),
        (0x8000_0000, &[38, 7719, 21238, 2437, 8855, 11797][..]),
        (u32::MAX, &[35, 29739, 3374, 11141, 31308, 7870][..]),
    ] {
        for _ in 0..2 {
            invoke(&mut p, SEED, &[seed], 99);
            for &value in values {
                invoke(&mut p, RANDOM, &[], value);
            }
        }
    }
    assert_eq!(p.last_error().unwrap(), 77);
    let mut error = [0; 4];
    p.memory.read(0x7000_2020, &mut error).unwrap();
    assert_eq!(error, 123_u32.to_le_bytes());
}

#[test]
fn independent_processes_and_faulted_frames_do_not_consume_or_reseed() {
    let mut p = Process32::load(&random_executable::pe32(), 33).unwrap();
    let mut other = Process32::load(&random_executable::pe32(), 32).unwrap();
    invoke(&mut p, SEED, &[1792], 99);
    invoke(&mut other, RANDOM, &[], 41);
    p.memory
        .map_zeroed(0xffff_f000, 4096, Permissions::READ_WRITE)
        .unwrap();
    p.memory
        .write(0xffff_fffc, &0x0040_1000_u32.to_le_bytes())
        .unwrap();
    for (api, stack) in [
        (RANDOM, 0x6000_0000),
        (RANDOM, 0xffff_fffd),
        (SEED, 0xffff_fffc),
        (SEED, 0x1000_fffc),
    ] {
        p.cpu.eip = api;
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
    prepare(&mut p, SEED, &[0]);
    assert_eq!(p.run(0).api_calls, 0);
    invoke(&mut p, RANDOM, &[], 5890);
    invoke(&mut other, RANDOM, &[], 18467);
    p.cpu.eip = RANDOM;
    p.cpu.set_register(Register32::Esp, 0xffff_fffc);
    assert_eq!(p.run(1).api_calls, 1);
    assert_eq!(p.cpu.eip, 0x0040_1000);
    assert_eq!(p.cpu.register(Register32::Esp), 0);
    assert_eq!(p.cpu.register(Register32::Eax), 1279);
    invoke(&mut p, RANDOM, &[], 19497);
}

#[test]
fn only_exact_symbols_belong_to_msvcrt() {
    for (module, name) in [
        ("kernel32.dll", "srand"),
        ("kernel32.dll", "rand"),
        ("msvcrt.dll", "Srand"),
        ("msvcrt.dll", "Rand"),
        ("msvcrt.dll", "rand_s"),
    ] {
        assert!(matches!(
            Process32::load(&imported_executable::pe32(&[0xcc], module, &[name]), 32),
            Err(LoadError::UnresolvedImport { .. })
        ));
    }
}
