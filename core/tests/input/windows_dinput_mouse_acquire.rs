use super::dinput_acquire_cases;
use super::dinput_device_cases;
use super::dinput_mouse_acquire_cases;
use super::dinput_mouse_cases;
use super::dinput_mouse_format_cases;

#[test]
fn imported_foreground_mouse_acquisition_matches_across_budgets() {
    dinput_mouse_acquire_cases::imported_mouse_acquisition_across_budgets();
}

use super::dinput_device_calls;
use dinput_acquire_cases::WINDOW;
use dinput_device_calls::*;
use dinput_mouse_format_cases::FORMAT;

const ACQUIRE: u32 = 0x7000_05ac;
const UNACQUIRE: u32 = 0x7000_05b0;
const RELEASE_MOUSE: u32 = 0x7000_0594;

fn words(p: &mut Process32, address: u32, values: &[u32]) {
    let bytes: Vec<_> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
    p.memory.write(u64::from(address), &bytes).unwrap();
}

fn mouse(p: &mut Process32, root: u32, configured: bool) -> u32 {
    p.memory
        .write(u64::from(GUID), &dinput_mouse_cases::MOUSE)
        .unwrap();
    assert_eq!(call(p, CREATE, &[root, GUID, DATA, 0]), 0);
    let device = word(p, DATA);
    if configured {
        assert_eq!(call(p, 0x7000_0598, &[device, FORMAT]), 0);
        assert_eq!(call(p, 0x7000_059c, &[device, WINDOW, 5]), 0);
    }
    device
}

fn setup(configured: bool) -> (Process32, u32, u32) {
    let mut p = dinput_mouse_acquire_cases::process();
    p.memory
        .protect(0x0040_1000, 4096, Permissions::READ_WRITE)
        .unwrap();
    p.memory.write(u64::from(CODE), &[0xcc]).unwrap();
    p.memory
        .protect(0x0040_1000, 4096, Permissions::READ_EXECUTE)
        .unwrap();
    let root = root(&mut p);
    let device = mouse(&mut p, root, configured);
    (p, root, device)
}

#[test]
fn acquisition_lifetime_is_not_reference_counted_and_queries_remain_available() {
    let (mut p, root, device) = setup(true);
    let pages = p.memory.mapped_pages();
    let windows = p.window_snapshots();
    put(&mut p, 0x7ffd_e034, 77);
    put(&mut p, 0x7ffd_e040, 88);
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
        assert_eq!(call(&mut p, api, &[device]), value);
    }
    assert_eq!(call(&mut p, ROOT_RELEASE, &[root]), 0);
    assert_eq!(call(&mut p, 0x7000_0590, &[device]), 2);
    assert_eq!(call(&mut p, RELEASE_MOUSE, &[device]), 1);
    put(&mut p, DATA + 128, 44);
    assert_eq!(call(&mut p, 0x7000_05a0, &[device, DATA + 128]), 0);
    words(&mut p, DATA + 128, &[20, 16, 8, 1, 77]);
    assert_eq!(call(&mut p, 0x7000_05a8, &[device, 3, DATA + 128]), 0);
    assert_eq!(word(&p, DATA + 144), 120);
    assert_eq!(call(&mut p, ACQUIRE, &[device]), 1);
    assert_eq!(call(&mut p, RELEASE_MOUSE, &[device]), 0);
    for api in [ACQUIRE, UNACQUIRE] {
        for receiver in [0, root, device, device + 1, device + 4, u32::MAX] {
            denied(&mut p, api, &[receiver]);
        }
    }
    assert_eq!(p.memory.mapped_pages(), pages);
    assert_eq!(p.window_snapshots(), windows);
    p.memory
        .protect(0x7ffd_e000, 4096, Permissions::READ_WRITE)
        .unwrap();
    assert_eq!(p.last_error().unwrap(), 77);
    assert_eq!(word(&p, 0x7ffd_e040), 88);
}

#[test]
fn only_one_mouse_identity_holds_access_until_unacquire_or_final_release() {
    let (mut p, root, first) = setup(true);
    let second = mouse(&mut p, root, true);
    assert_eq!(call(&mut p, ACQUIRE, &[first]), 0);
    assert_eq!(call(&mut p, ACQUIRE, &[second]), 0x8007_0005);
    p.memory
        .write(u64::from(IID), &dinput_device_cases::DEVICE)
        .unwrap();
    assert_eq!(call(&mut p, 0x7000_058c, &[first, IID, DATA]), 0);
    let alias = word(&p, DATA);
    assert_eq!(call(&mut p, ACQUIRE, &[alias]), 1);
    assert_eq!(call(&mut p, RELEASE_MOUSE, &[first]), 1);
    assert_eq!(call(&mut p, ACQUIRE, &[second]), 0x8007_0005);
    assert_eq!(call(&mut p, UNACQUIRE, &[alias]), 0);
    assert_eq!(call(&mut p, ACQUIRE, &[second]), 0);
    assert_eq!(call(&mut p, ACQUIRE, &[alias]), 0x8007_0005);
    assert_eq!(call(&mut p, RELEASE_MOUSE, &[second]), 0);
    assert_eq!(call(&mut p, ACQUIRE, &[alias]), 0);
    denied(&mut p, ACQUIRE, &[second]);
    assert_eq!(call(&mut p, ROOT_RELEASE, &[root]), 0);
    assert_eq!(call(&mut p, ACQUIRE, &[alias]), 1);
    assert_eq!(call(&mut p, RELEASE_MOUSE, &[alias]), 0);
}

#[test]
fn focus_away_and_back_does_not_revive_an_old_exclusive_claim() {
    let (mut p, root, first) = setup(true);
    let second = mouse(&mut p, root, true);
    assert_eq!(call(&mut p, ACQUIRE, &[first]), 0);
    assert_eq!(call(&mut p, 0x7000_04b4, &[WINDOW]), 1);
    assert_eq!(call(&mut p, 0x7000_0440, &[WINDOW, 5]), 1);
    assert_eq!(call(&mut p, 0x7000_04b4, &[WINDOW + 4]), 0);
    assert_eq!(call(&mut p, ACQUIRE, &[first]), 1);
    assert_eq!(call(&mut p, 0x7000_0440, &[WINDOW, 0]), 1);
    assert_eq!(call(&mut p, 0x7000_0440, &[WINDOW, 5]), 0);
    assert_eq!(call(&mut p, ACQUIRE, &[second]), 0);
    assert_eq!(call(&mut p, ACQUIRE, &[first]), 0x8007_0005);
    assert_eq!(call(&mut p, UNACQUIRE, &[first]), 1);
    assert_eq!(call(&mut p, ACQUIRE, &[second]), 1);
    assert_eq!(call(&mut p, UNACQUIRE, &[second]), 0);
    assert_eq!(call(&mut p, ACQUIRE, &[first]), 0);
}

#[test]
fn valid_format_cooperation_and_foreground_are_required() {
    let (mut p, root, device) = setup(false);
    let keyboard = create(&mut p, root);
    for api in [ACQUIRE, UNACQUIRE] {
        denied(&mut p, api, &[keyboard]);
    }
    assert_eq!(call(&mut p, ACQUIRE, &[device]), 0x8007_0057);
    assert_eq!(call(&mut p, UNACQUIRE, &[device]), 1);
    assert_eq!(call(&mut p, 0x7000_0598, &[device, FORMAT]), 0);
    denied(&mut p, ACQUIRE, &[device]);
    assert_eq!(call(&mut p, 0x7000_059c, &[device, WINDOW, 5]), 0);
    assert_eq!(call(&mut p, 0x7000_0440, &[WINDOW, 0]), 1);
    assert_eq!(call(&mut p, ACQUIRE, &[device]), 0x8007_0005);
    assert_eq!(call(&mut p, UNACQUIRE, &[device]), 1);
    assert_eq!(call(&mut p, 0x7000_0440, &[WINDOW, 5]), 0);
    assert_eq!(call(&mut p, ACQUIRE, &[device]), 0);
}

#[test]
fn acquired_setters_keep_validation_prefixes_and_allow_replacement_after_unacquire() {
    let (mut p, _, device) = setup(true);
    assert_eq!(call(&mut p, ACQUIRE, &[device]), 0);
    assert_eq!(call(&mut p, 0x7000_0598, &[device, 0]), 0x8000_4003);
    assert_eq!(call(&mut p, 0x7000_0598, &[device, FORMAT]), 0x8007_00aa);
    p.memory
        .map_zeroed(0xffff_f000, 4096, Permissions::READ_WRITE)
        .unwrap();
    words(&mut p, 0xffff_fff8, &[23, 16]);
    assert_eq!(
        call(&mut p, 0x7000_0598, &[device, 0xffff_fff8]),
        0x8007_0057
    );
    words(&mut p, 0xffff_fff8, &[24, 15]);
    assert_eq!(
        call(&mut p, 0x7000_0598, &[device, 0xffff_fff8]),
        0x8007_0057
    );
    words(&mut p, 0xffff_fff8, &[24, 16]);
    assert_eq!(
        call(&mut p, 0x7000_0598, &[device, 0xffff_fff8]),
        0x8007_00aa
    );
    denied(&mut p, 0x7000_05a4, &[device, 2, 0x5000_0000]);
    assert_eq!(call(&mut p, 0x7000_05a4, &[device, 1, 0]), 0x8007_0057);
    for header in [[19, 16, 0, 0, 32], [20, 15, 0, 0, 32], [20, 16, 1, 0, 32]] {
        words(&mut p, DATA + 96, &header);
        assert_eq!(
            call(&mut p, 0x7000_05a4, &[device, 1, DATA + 96]),
            0x8007_0057
        );
    }
    words(&mut p, DATA + 96, &[20, 16, 0, 1, 32]);
    denied(&mut p, 0x7000_05a4, &[device, 1, DATA + 96]);
    words(&mut p, 0xffff_fff8, &[20, 16]);
    prepare(&mut p, 0x7000_05a4, &[device, 1, 0xffff_fff8]);
    refused(
        &mut p,
        &ProcessStop::Stopped(StopReason::MemoryFault(MemoryError::AddressOverflow)),
    );
    words(&mut p, 0xffff_fff8, &[24, 16]);
    words(&mut p, DATA + 96, &[20, 16, 0, 0, 16]);
    assert_eq!(
        call(&mut p, 0x7000_05a4, &[device, 1, DATA + 96]),
        0x8007_00aa
    );
    assert_eq!(call(&mut p, 0x7000_059c, &[device, 0, 0]), 0x8007_0057);
    denied(&mut p, 0x7000_059c, &[device, 0, 6]);
    assert_eq!(call(&mut p, 0x7000_059c, &[device, 0, 5]), 0x8007_0006);
    assert_eq!(call(&mut p, 0x7000_059c, &[device, WINDOW, 5]), 0x8007_00aa);
    assert_eq!(call(&mut p, ACQUIRE, &[device]), 1);
    assert_eq!(call(&mut p, UNACQUIRE, &[device]), 0);
    prepare(&mut p, 0x7000_0598, &[device, 0xffff_fff8]);
    refused(
        &mut p,
        &ProcessStop::Stopped(StopReason::MemoryFault(MemoryError::AddressOverflow)),
    );
    assert_eq!(call(&mut p, 0x7000_0598, &[device, FORMAT]), 0);
    assert_eq!(call(&mut p, 0x7000_05a4, &[device, 1, DATA + 96]), 0);
    assert_eq!(call(&mut p, 0x7000_059c, &[device, WINDOW, 5]), 0);
    assert_eq!(call(&mut p, ACQUIRE, &[device]), 0);
}

#[test]
fn zero_incomplete_and_overflowing_frames_cannot_unacquire_the_device() {
    let (mut p, _, device) = setup(true);
    assert_eq!(call(&mut p, ACQUIRE, &[device]), 0);
    p.memory
        .map_zeroed(0xffff_f000, 4096, Permissions::READ_WRITE)
        .unwrap();
    for api in [ACQUIRE, UNACQUIRE] {
        let before = prepare(&mut p, api, &[device]);
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
        words(&mut p, 0xffff_fff8, &[CODE, device]);
        p.cpu.set_register(Register32::Esp, 0xffff_fff8);
        refused(
            &mut p,
            &ProcessStop::Stopped(StopReason::MemoryFault(MemoryError::AddressOverflow)),
        );
        assert_eq!(call(&mut p, ACQUIRE, &[device]), 1);
    }
}

#[test]
fn foreign_and_scheduled_children_cannot_change_primary_acquisition() {
    let (mut p, _, device) = setup(true);
    assert_eq!(call(&mut p, ACQUIRE, &[device]), 0);
    for fs in [0, 0x1101_0000, 0x1234_0000] {
        p.cpu.set_fs_base(fs);
        for api in [ACQUIRE, UNACQUIRE] {
            p.cpu.eip = api;
            p.cpu.set_register(Register32::Esp, 0x5000_0000);
            refused(&mut p, &ProcessStop::UnsupportedApi { address: api });
        }
    }
    p.cpu.set_fs_base(0x7ffd_e000);
    assert_eq!(call(&mut p, ACQUIRE, &[device]), 1);
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
