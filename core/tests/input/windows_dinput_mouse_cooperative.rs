use super::dinput_cooperative_cases;
use super::dinput_mouse_cooperative_cases;

#[test]
fn imported_foreground_mouse_setting_preserves_windows_across_budgets() {
    dinput_mouse_cooperative_cases::imported_foreground_mouse_setting_across_budgets();
}

use super::dinput_device_calls;
use dinput_cooperative_cases::{DATA as OUTPUT, WINDOW};
use dinput_device_calls::*;

const SET: u32 = 0x7000_059c;
const RELEASE_MOUSE: u32 = 0x7000_0594;

fn setup() -> (Process32, u32, u32) {
    let mut p = dinput_mouse_cooperative_cases::process();
    p.memory
        .protect(0x0040_1000, 4096, Permissions::READ_WRITE)
        .unwrap();
    p.memory.write(u64::from(CODE), &[0xcc]).unwrap();
    p.memory
        .protect(0x0040_1000, 4096, Permissions::READ_EXECUTE)
        .unwrap();
    assert_eq!(call(&mut p, ROOT_CREATE, &[1, 0x700, OUTPUT, 0]), 0);
    let root = word(&p, OUTPUT);
    assert_eq!(call(&mut p, CREATE, &[root, OUTPUT + 64, OUTPUT, 0]), 0);
    let device = word(&p, OUTPUT);
    (p, root, device)
}

#[test]
fn mouse_policy_needs_no_format_activation_or_teb_and_preserves_windows() {
    let (mut p, root, device) = setup();
    let windows = p.window_snapshots();
    assert!(!windows[0].active);
    let pages = p.memory.mapped_pages();
    assert_eq!(call(&mut p, ROOT_RELEASE, &[root]), 0);
    put(&mut p, 0x7ffd_e034, 77);
    put(&mut p, 0x7ffd_e040, 88);
    p.memory
        .protect(0x7ffd_e000, 4096, Permissions::NONE)
        .unwrap();
    for _ in 0..2 {
        assert_eq!(call(&mut p, SET, &[device, WINDOW, 5]), 0);
    }
    assert_eq!(call(&mut p, SET, &[device, 0, 5]), 0x8007_0006);
    assert_eq!(call(&mut p, SET, &[device, 0, 0]), 0x8007_0057);
    assert_eq!(p.window_snapshots(), windows);
    assert_eq!(p.memory.mapped_pages(), pages);
    assert_eq!(call(&mut p, RELEASE_MOUSE, &[device]), 0);
    p.memory
        .protect(0x7ffd_e000, 4096, Permissions::READ_WRITE)
        .unwrap();
    assert_eq!(p.last_error().unwrap(), 77);
    assert_eq!(word(&p, 0x7ffd_e040), 88);
}

#[test]
fn invalid_flag_pairs_precede_windows_and_wrong_profiles_and_receivers_remain_unsupported() {
    let (mut p, root, device) = setup();
    let keyboard = create(&mut p, root);
    for flags in [0, 1, 2, 3, 4, 7, 8, 12, 14, 0x8000_0000, u32::MAX] {
        assert_eq!(call(&mut p, SET, &[device, 0, flags]), 0x8007_0057);
    }
    for flags in [6, 9, 10, 0x15, 0x16, 0x25, 0x8000_0005] {
        denied(&mut p, SET, &[device, 0, flags]);
    }
    for window in [0, 1, WINDOW + 4, root, device, u32::MAX] {
        assert_eq!(call(&mut p, SET, &[device, window, 5]), 0x8007_0006);
    }
    for receiver in [0, root, keyboard, device + 1, device + 4, u32::MAX] {
        denied(&mut p, SET, &[receiver, WINDOW, 5]);
    }
    denied(&mut p, 0x7000_057c, &[device, WINDOW, 6]);
    assert_eq!(call(&mut p, SET, &[device, WINDOW, 5]), 0);
    assert_eq!(call(&mut p, RELEASE_MOUSE, &[device]), 0);
    denied(&mut p, SET, &[device, 0, 0]);
    assert_eq!(call(&mut p, RELEASE, &[keyboard]), 0);
}

#[test]
fn zero_incomplete_and_overflowing_frames_preserve_mouse_policy_and_windows() {
    let (mut p, _, device) = setup();
    let windows = p.window_snapshots();
    let before = prepare(&mut p, SET, &[device, WINDOW, 5]);
    let result = p.run(0);
    assert_eq!((result.instructions, result.api_calls), (0, 0));
    assert_eq!(p.cpu, before);
    put(&mut p, 0x1000_fffc, CODE);
    p.cpu.set_register(Register32::Esp, 0x1000_fffc);
    fault(&mut p);
    p.memory
        .map_zeroed(0xffff_f000, 4096, Permissions::READ_WRITE)
        .unwrap();
    for (address, value) in [
        (0xffff_fff0, CODE),
        (0xffff_fff4, device),
        (0xffff_fff8, WINDOW),
        (0xffff_fffc, 5),
    ] {
        put(&mut p, address, value);
    }
    p.cpu.set_register(Register32::Esp, 0xffff_fff0);
    refused(
        &mut p,
        &ProcessStop::Stopped(StopReason::MemoryFault(MemoryError::AddressOverflow)),
    );
    assert_eq!(call(&mut p, SET, &[device, WINDOW, 5]), 0);
    assert_eq!(call(&mut p, RELEASE_MOUSE, &[device]), 0);
    assert_eq!(p.window_snapshots(), windows);
}

#[test]
fn foreign_and_scheduled_child_identity_precedes_frame_and_window_reads() {
    let (mut p, _, device) = setup();
    for fs in [0, 0x1101_0000, 0x1234_0000] {
        p.cpu.set_fs_base(fs);
        p.cpu.eip = SET;
        p.cpu.set_register(Register32::Esp, 0x5000_0000);
        refused(&mut p, &ProcessStop::UnsupportedApi { address: SET });
    }
    p.cpu.set_fs_base(0x7ffd_e000);
    assert_eq!(call(&mut p, SET, &[device, WINDOW, 5]), 0);
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
