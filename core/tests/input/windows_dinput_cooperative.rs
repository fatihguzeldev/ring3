use super::dinput_cooperative_cases;

#[test]
fn imported_foreground_keyboard_setting_preserves_windows_across_budgets() {
    dinput_cooperative_cases::imported_foreground_keyboard_setting_across_budgets();
}

use dinput_cooperative_cases::{DATA, WINDOW};
use ring3_core::execution::{
    Cpu32, MemoryError, Permissions, Process32, ProcessStop, Register32, StopReason,
};

const SET: u32 = 0x7000_057c;
const STACK: u32 = 0x1000_ef00;
const CODE: u32 = 0x0040_1300;

fn put(p: &mut Process32, address: u32, value: u32) {
    p.memory
        .write(u64::from(address), &value.to_le_bytes())
        .unwrap();
}

fn prepare(p: &mut Process32, api: u32, args: &[u32]) -> Cpu32 {
    p.cpu.eip = api;
    p.cpu.set_register(Register32::Esp, STACK);
    let frame: Vec<_> = std::iter::once(CODE)
        .chain(args.iter().copied())
        .flat_map(u32::to_le_bytes)
        .collect();
    p.memory.write(u64::from(STACK), &frame).unwrap();
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

fn denied(p: &mut Process32, args: &[u32]) {
    prepare(p, SET, args);
    refused(p, &ProcessStop::UnsupportedApi { address: SET });
}

fn process() -> (Process32, u32, u32) {
    let mut p = dinput_cooperative_cases::process();
    p.memory
        .protect(0x0040_1000, 4096, Permissions::READ_WRITE)
        .unwrap();
    p.memory.write(u64::from(CODE), &[0xcc]).unwrap();
    p.memory
        .protect(0x0040_1000, 4096, Permissions::READ_EXECUTE)
        .unwrap();
    assert_eq!(call(&mut p, 0x7000_0558, &[1, 0x700, DATA, 0]), 0);
    assert_eq!(
        call(&mut p, 0x7000_0568, &[0x7001_7800, DATA + 64, DATA, 0]),
        0
    );
    (p, 0x7001_7800, 0x7001_7900)
}

#[test]
fn configuration_needs_no_activation_format_or_teb_and_preserves_windows() {
    let (mut p, root, device) = process();
    let windows = p.window_snapshots();
    assert!(!windows[0].active);
    let pages = p.memory.mapped_pages();
    assert_eq!(call(&mut p, 0x7000_0564, &[root]), 0);
    put(&mut p, 0x7ffd_e034, 77);
    put(&mut p, 0x7ffd_e040, 88);
    p.memory
        .protect(0x7ffd_e000, 4096, Permissions::NONE)
        .unwrap();
    for flags in [6, 0x16, 6] {
        assert_eq!(call(&mut p, SET, &[device, WINDOW, flags]), 0);
    }
    assert_eq!(call(&mut p, SET, &[device, 0, 6]), 0x8007_0006);
    assert_eq!(call(&mut p, SET, &[device, 0, 0]), 0x8007_0057);
    assert_eq!(p.window_snapshots(), windows);
    assert_eq!(p.memory.mapped_pages(), pages);
    assert_eq!(call(&mut p, 0x7000_0574, &[device]), 0);
    p.memory
        .protect(0x7ffd_e000, 4096, Permissions::READ_WRITE)
        .unwrap();
    assert_eq!(p.last_error().unwrap(), 77);
    let mut errno = [0; 4];
    p.memory.read(0x7ffd_e040, &mut errno).unwrap();
    assert_eq!(u32::from_le_bytes(errno), 88);
}

#[test]
fn flag_pair_errors_precede_window_errors_and_other_profiles_remain_unsupported() {
    let (mut p, root, device) = process();
    let windows = p.window_snapshots();
    for flags in [0, 1, 2, 3, 4, 7, 8, 12, 14, 0x8000_0000, u32::MAX] {
        assert_eq!(call(&mut p, SET, &[device, 0, flags]), 0x8007_0057);
    }
    for flags in [5, 9, 10, 0x15, 0x1a, 0x26, 0x8000_0006] {
        denied(&mut p, &[device, 0, flags]);
    }
    for window in [0, 1, WINDOW + 4, root, device, u32::MAX] {
        assert_eq!(call(&mut p, SET, &[device, window, 6]), 0x8007_0006);
    }
    assert_eq!(call(&mut p, SET, &[device, WINDOW, 0x16]), 0);
    assert_eq!(p.window_snapshots(), windows);
    assert_eq!(call(&mut p, 0x7000_0574, &[device]), 0);
    for receiver in [0, root, device, device + 1, device + 4, u32::MAX] {
        denied(&mut p, &[receiver, 0, 0]);
    }
}

#[test]
fn zero_budget_incomplete_and_overflowing_frames_cannot_change_settings() {
    let (mut p, _, device) = process();
    let windows = p.window_snapshots();
    let before = prepare(&mut p, SET, &[device, WINDOW, 6]);
    let run = p.run(0);
    assert_eq!((run.instructions, run.api_calls), (0, 0));
    assert_eq!(p.cpu, before);
    put(&mut p, 0x1000_fffc, CODE);
    p.cpu.set_register(Register32::Esp, 0x1000_fffc);
    let before = p.cpu;
    let run = p.run(1);
    assert!(matches!(
        run.reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!((run.instructions, run.api_calls), (0, 0));
    assert_eq!(p.cpu, before);
    p.memory
        .map_zeroed(0xffff_f000, 4096, Permissions::READ_WRITE)
        .unwrap();
    for (offset, value) in [(0, CODE), (4, device), (8, WINDOW), (12, 6)] {
        put(&mut p, 0xffff_fff0 + offset, value);
    }
    p.cpu.set_register(Register32::Esp, 0xffff_fff0);
    refused(
        &mut p,
        &ProcessStop::Stopped(StopReason::MemoryFault(MemoryError::AddressOverflow)),
    );
    assert_eq!(call(&mut p, SET, &[device, WINDOW, 6]), 0);
    assert_eq!(p.window_snapshots(), windows);
    assert_eq!(call(&mut p, 0x7000_0574, &[device]), 0);
}

#[test]
fn foreign_and_scheduled_child_calls_stop_before_frames_or_window_lookup() {
    let (mut p, _, _) = process();
    for fs in [0, 0x1101_0000, 0x1234_0000] {
        p.cpu.set_fs_base(fs);
        p.cpu.eip = SET;
        p.cpu.set_register(Register32::Esp, 0x5000_0000);
        refused(&mut p, &ProcessStop::UnsupportedApi { address: SET });
    }
    p.cpu.set_fs_base(0x7ffd_e000);
    let child = call(&mut p, 0x7000_0548, &[0, 0, CODE, 0, 4, 0]);
    assert_eq!(call(&mut p, 0x7000_0550, &[child]), 1);
    assert_eq!(
        p.run(100).reason,
        ProcessStop::Stopped(StopReason::Breakpoint)
    );
    assert_eq!(p.cpu.fs_base(), 0x1101_0000);
    p.cpu.eip = SET;
    p.cpu.set_register(Register32::Esp, 0x5000_0000);
    refused(&mut p, &ProcessStop::UnsupportedApi { address: SET });
}
