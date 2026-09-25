use super::dinput_acquire_cases;
use super::dinput_format_cases;

#[test]
fn imported_foreground_keyboard_acquisition_matches_across_budgets() {
    dinput_acquire_cases::imported_keyboard_acquisition_across_budgets();
}

use dinput_acquire_cases::{DATA, WINDOW};
use dinput_format_cases::FORMAT;
use ring3_core::execution::{
    Cpu32, MemoryError, Permissions, Process32, ProcessStop, Register32, StopReason,
};

const ACQUIRE: u32 = 0x7000_0584;
const UNACQUIRE: u32 = 0x7000_0588;
const ROOT: u32 = 0x7001_7800;
const DEVICE: u32 = 0x7001_7900;
const STACK: u32 = 0x1000_ef00;
const CODE: u32 = 0x0040_1300;

fn words(p: &mut Process32, address: u32, values: &[u32]) {
    let bytes: Vec<_> = values
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect();
    p.memory.write(u64::from(address), &bytes).unwrap();
}

fn prepare(p: &mut Process32, api: u32, args: &[u32]) -> Cpu32 {
    p.cpu.eip = api;
    p.cpu.set_register(Register32::Esp, STACK);
    words(p, STACK, &[CODE]);
    words(p, STACK + 4, args);
    p.cpu
}

fn call(p: &mut Process32, api: u32, args: &[u32]) -> u32 {
    prepare(p, api, args);
    let run = p.run(1);
    assert_eq!(
        run.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((run.instructions, run.api_calls), (0, 1));
    let cleanup = if api == 0x7000_0548 {
        4
    } else {
        (args.len() + 1) * 4
    };
    assert_eq!(
        p.cpu.register(Register32::Esp),
        STACK + u32::try_from(cleanup).unwrap()
    );
    p.cpu.register(Register32::Eax)
}

fn refused(p: &mut Process32, expected: &ProcessStop) {
    let before = p.cpu;
    for _ in 0..2 {
        let run = p.run(1);
        assert_eq!(&run.reason, expected);
        assert_eq!((run.instructions, run.api_calls), (0, 0));
        assert_eq!(p.cpu, before);
    }
}

fn denied(p: &mut Process32, api: u32, args: &[u32]) {
    prepare(p, api, args);
    refused(p, &ProcessStop::UnsupportedApi { address: api });
}

fn process(configured: bool) -> Process32 {
    let mut p = dinput_acquire_cases::process();
    assert_eq!(call(&mut p, 0x7000_0558, &[1, 0x700, DATA, 0]), 0);
    assert_eq!(call(&mut p, 0x7000_0568, &[ROOT, DATA + 64, DATA, 0]), 0);
    if configured {
        assert_eq!(call(&mut p, 0x7000_0578, &[DEVICE, FORMAT]), 0);
        assert_eq!(call(&mut p, 0x7000_057c, &[DEVICE, WINDOW, 6]), 0);
    }
    p
}

#[test]
fn acquisition_is_not_reference_counted_and_needs_no_teb_or_extra_pages() {
    let mut p = process(true);
    let windows = p.window_snapshots();
    let pages = p.memory.mapped_pages();
    words(&mut p, 0x7ffd_e034, &[77]);
    words(&mut p, 0x7ffd_e040, &[88]);
    p.memory
        .protect(0x7ffd_e000, 4096, Permissions::NONE)
        .unwrap();
    for (api, value) in [
        (ACQUIRE, 0),
        (ACQUIRE, 1),
        (UNACQUIRE, 0),
        (UNACQUIRE, 1),
        (ACQUIRE, 0),
    ] {
        assert_eq!(call(&mut p, api, &[DEVICE]), value);
    }
    assert_eq!(call(&mut p, 0x7000_0564, &[ROOT]), 0);
    assert_eq!(call(&mut p, 0x7000_0570, &[DEVICE]), 2);
    assert_eq!(call(&mut p, 0x7000_0574, &[DEVICE]), 1);
    assert_eq!(call(&mut p, ACQUIRE, &[DEVICE]), 1);
    assert_eq!(call(&mut p, 0x7000_0574, &[DEVICE]), 0);
    for api in [ACQUIRE, UNACQUIRE] {
        for receiver in [0, ROOT, DEVICE, DEVICE + 1, DEVICE + 4, u32::MAX] {
            denied(&mut p, api, &[receiver]);
        }
    }
    assert_eq!(p.window_snapshots(), windows);
    assert_eq!(p.memory.mapped_pages(), pages);
    p.memory
        .protect(0x7ffd_e000, 4096, Permissions::READ_WRITE)
        .unwrap();
    assert_eq!(p.last_error().unwrap(), 77);
    let mut errno = [0; 4];
    p.memory.read(0x7ffd_e040, &mut errno).unwrap();
    assert_eq!(u32::from_le_bytes(errno), 88);
}

#[test]
fn format_cooperation_and_foreground_are_required() {
    let mut p = process(false);
    assert_eq!(call(&mut p, ACQUIRE, &[DEVICE]), 0x8007_0057);
    assert_eq!(call(&mut p, UNACQUIRE, &[DEVICE]), 1);
    assert_eq!(call(&mut p, 0x7000_0578, &[DEVICE, FORMAT]), 0);
    denied(&mut p, ACQUIRE, &[DEVICE]);
    assert_eq!(call(&mut p, 0x7000_057c, &[DEVICE, WINDOW, 6]), 0);
    assert_eq!(call(&mut p, 0x7000_0440, &[WINDOW, 0]), 1);
    assert_eq!(call(&mut p, ACQUIRE, &[DEVICE]), 0x8007_0005);
    assert_eq!(call(&mut p, UNACQUIRE, &[DEVICE]), 1);
    assert_eq!(call(&mut p, 0x7000_0440, &[WINDOW, 5]), 0);
    assert_eq!(call(&mut p, ACQUIRE, &[DEVICE]), 0);
}

#[test]
fn real_hide_and_show_invalidate_acquisition_without_intervening_input_calls() {
    let mut p = process(true);
    assert_eq!(call(&mut p, ACQUIRE, &[DEVICE]), 0);
    assert_eq!(call(&mut p, 0x7000_04b4, &[WINDOW]), 1);
    assert_eq!(call(&mut p, 0x7000_0440, &[WINDOW, 5]), 1);
    assert_eq!(call(&mut p, 0x7000_04b4, &[WINDOW + 4]), 0);
    assert_eq!(call(&mut p, ACQUIRE, &[DEVICE]), 1);
    for _ in 0..2 {
        assert_eq!(call(&mut p, 0x7000_0440, &[WINDOW, 0]), 1);
        assert_eq!(call(&mut p, 0x7000_0440, &[WINDOW, 5]), 0);
        assert_eq!(call(&mut p, ACQUIRE, &[DEVICE]), 0);
        assert_eq!(call(&mut p, ACQUIRE, &[DEVICE]), 1);
    }
}

#[test]
fn acquired_setters_keep_validation_prefixes_and_allow_replacement_after_unacquire() {
    let mut p = process(true);
    assert_eq!(call(&mut p, ACQUIRE, &[DEVICE]), 0);
    assert_eq!(call(&mut p, 0x7000_0578, &[DEVICE, 0]), 0x8000_4003);
    assert_eq!(call(&mut p, 0x7000_0578, &[DEVICE, FORMAT]), 0x8007_00aa);
    p.memory
        .map_zeroed(0xffff_f000, 4096, Permissions::READ_WRITE)
        .unwrap();
    words(&mut p, 0xffff_fff8, &[23, 16]);
    assert_eq!(
        call(&mut p, 0x7000_0578, &[DEVICE, 0xffff_fff8]),
        0x8007_0057
    );
    words(&mut p, 0xffff_fff8, &[24, 15]);
    assert_eq!(
        call(&mut p, 0x7000_0578, &[DEVICE, 0xffff_fff8]),
        0x8007_0057
    );
    words(&mut p, 0xffff_fff8, &[24, 16]);
    assert_eq!(
        call(&mut p, 0x7000_0578, &[DEVICE, 0xffff_fff8]),
        0x8007_00aa
    );
    denied(&mut p, 0x7000_0580, &[DEVICE, 2, 0x5000_0000]);
    assert_eq!(call(&mut p, 0x7000_0580, &[DEVICE, 1, 0]), 0x8007_0057);
    words(&mut p, DATA + 96, &[20, 16, 0, 0, 16]);
    assert_eq!(
        call(&mut p, 0x7000_0580, &[DEVICE, 1, DATA + 96]),
        0x8007_00aa
    );
    assert_eq!(call(&mut p, 0x7000_057c, &[DEVICE, 0, 0]), 0x8007_0057);
    denied(&mut p, 0x7000_057c, &[DEVICE, 0, 5]);
    assert_eq!(call(&mut p, 0x7000_057c, &[DEVICE, 0, 6]), 0x8007_0006);
    assert_eq!(
        call(&mut p, 0x7000_057c, &[DEVICE, WINDOW, 0x16]),
        0x8007_00aa
    );
    assert_eq!(call(&mut p, ACQUIRE, &[DEVICE]), 1);
    assert_eq!(call(&mut p, UNACQUIRE, &[DEVICE]), 0);
    prepare(&mut p, 0x7000_0578, &[DEVICE, 0xffff_fff8]);
    refused(
        &mut p,
        &ProcessStop::Stopped(StopReason::MemoryFault(MemoryError::AddressOverflow)),
    );
    assert_eq!(call(&mut p, 0x7000_0578, &[DEVICE, FORMAT]), 0);
    assert_eq!(call(&mut p, 0x7000_0580, &[DEVICE, 1, DATA + 96]), 0);
    assert_eq!(call(&mut p, 0x7000_057c, &[DEVICE, WINDOW, 0x16]), 0);
    assert_eq!(call(&mut p, ACQUIRE, &[DEVICE]), 0);
}

#[test]
fn zero_incomplete_and_overflowing_frames_cannot_unacquire_the_device() {
    let mut p = process(true);
    assert_eq!(call(&mut p, ACQUIRE, &[DEVICE]), 0);
    p.memory
        .map_zeroed(0xffff_f000, 4096, Permissions::READ_WRITE)
        .unwrap();
    for api in [ACQUIRE, UNACQUIRE] {
        let before = prepare(&mut p, api, &[DEVICE]);
        let run = p.run(0);
        assert_eq!((run.instructions, run.api_calls), (0, 0));
        assert_eq!(p.cpu, before);
        words(&mut p, 0x1000_fffc, &[CODE]);
        p.cpu.set_register(Register32::Esp, 0x1000_fffc);
        let before = p.cpu;
        let run = p.run(1);
        assert!(matches!(
            run.reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
        assert_eq!((run.instructions, run.api_calls), (0, 0));
        assert_eq!(p.cpu, before);
        words(&mut p, 0xffff_fff8, &[CODE, DEVICE]);
        p.cpu.set_register(Register32::Esp, 0xffff_fff8);
        refused(
            &mut p,
            &ProcessStop::Stopped(StopReason::MemoryFault(MemoryError::AddressOverflow)),
        );
        assert_eq!(call(&mut p, ACQUIRE, &[DEVICE]), 1);
    }
}

#[test]
fn foreign_and_scheduled_children_cannot_change_primary_acquisition() {
    let mut p = process(true);
    assert_eq!(call(&mut p, ACQUIRE, &[DEVICE]), 0);
    for fs in [0, 0x1101_0000, 0x1234_0000] {
        p.cpu.set_fs_base(fs);
        for api in [ACQUIRE, UNACQUIRE] {
            p.cpu.eip = api;
            p.cpu.set_register(Register32::Esp, 0x5000_0000);
            refused(&mut p, &ProcessStop::UnsupportedApi { address: api });
        }
    }
    p.cpu.set_fs_base(0x7ffd_e000);
    assert_eq!(call(&mut p, ACQUIRE, &[DEVICE]), 1);
    let child = call(&mut p, 0x7000_0548, &[0, 0, CODE, 0, 4, 0]);
    assert_eq!(call(&mut p, 0x7000_0550, &[child]), 1);
    assert_eq!(
        p.run(100).reason,
        ProcessStop::Stopped(StopReason::Breakpoint)
    );
    assert_eq!(p.cpu.fs_base(), 0x1101_0000);
    for api in [ACQUIRE, UNACQUIRE] {
        p.cpu.eip = api;
        p.cpu.set_register(Register32::Esp, 0x5000_0000);
        refused(&mut p, &ProcessStop::UnsupportedApi { address: api });
    }
}
