use super::dinput_device_cases;
use super::dinput_mouse_cases;
use super::dinput_mouse_property_cases;
use super::dinput_property_cases;

#[test]
fn imported_mouse_buffer_setting_matches_across_budgets() {
    dinput_mouse_property_cases::imported_mouse_buffer_setting_across_budgets();
}

use super::dinput_device_calls;
use dinput_device_calls::*;
use dinput_property_cases::HEADER;

const SET: u32 = 0x7000_05a4;
const RELEASE_MOUSE: u32 = 0x7000_0594;

fn words(p: &mut Process32, address: u32, values: &[u32]) {
    let bytes: Vec<_> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
    p.memory.write(u64::from(address), &bytes).unwrap();
}

fn setup() -> (Process32, u32, u32) {
    let mut p = process();
    let root = root(&mut p);
    p.memory
        .write(u64::from(GUID), &dinput_mouse_cases::MOUSE)
        .unwrap();
    assert_eq!(call(&mut p, CREATE, &[root, GUID, DATA, 0]), 0);
    let device = word(&p, DATA);
    p.memory
        .map_zeroed(0x3000_0000, 4096, Permissions::READ_WRITE)
        .unwrap();
    words(&mut p, HEADER, &[20, 16, 0, 0, 16]);
    (p, root, device)
}

#[test]
fn alias_and_root_retirement_need_no_format_window_or_teb_and_keep_capabilities() {
    let (mut p, root, device) = setup();
    p.memory
        .write(u64::from(IID), &dinput_device_cases::DEVICE)
        .unwrap();
    assert_eq!(call(&mut p, 0x7000_058c, &[device, IID, DATA]), 0);
    let alias = word(&p, DATA);
    assert_eq!(call(&mut p, ROOT_RELEASE, &[root]), 0);
    put(&mut p, DATA + 128, 44);
    assert_eq!(call(&mut p, 0x7000_05a0, &[device, DATA + 128]), 0);
    let mut caps = [0; 44];
    p.memory.read(u64::from(DATA + 128), &mut caps).unwrap();
    let windows = p.window_snapshots();
    let pages = p.memory.mapped_pages();
    put(&mut p, 0x7ffd_e034, 77);
    put(&mut p, 0x7ffd_e040, 88);
    p.memory
        .protect(0x7ffd_e000, 4096, Permissions::NONE)
        .unwrap();
    for value in [0, 1, 16, 1024, 1025, u32::MAX, 0] {
        words(&mut p, HEADER, &[20, 16, 0, 0, value]);
        p.memory
            .protect(0x3000_0000, 4096, Permissions::READ)
            .unwrap();
        assert_eq!(call(&mut p, SET, &[alias, 1, HEADER]), 0);
        assert_eq!(word(&p, HEADER + 16), value);
        p.memory
            .protect(0x3000_0000, 4096, Permissions::READ_WRITE)
            .unwrap();
    }
    assert_eq!(call(&mut p, 0x7000_05a0, &[alias, DATA + 128]), 0);
    let mut after = [0; 44];
    p.memory.read(u64::from(DATA + 128), &mut after).unwrap();
    assert_eq!(after, caps);
    assert_eq!(p.memory.mapped_pages(), pages);
    assert_eq!(p.window_snapshots(), windows);
    assert_eq!(call(&mut p, RELEASE_MOUSE, &[device]), 1);
    assert_eq!(call(&mut p, RELEASE_MOUSE, &[alias]), 0);
    p.memory
        .protect(0x7ffd_e000, 4096, Permissions::READ_WRITE)
        .unwrap();
    assert_eq!(p.last_error().unwrap(), 77);
    assert_eq!(word(&p, 0x7ffd_e040), 88);
}

#[test]
fn receiver_property_and_header_checks_precede_unneeded_reads() {
    let (mut p, root, device) = setup();
    let keyboard = create(&mut p, root);
    for receiver in [0, root, keyboard, device + 1, device + 4, u32::MAX] {
        denied(&mut p, SET, &[receiver, 1, 0]);
    }
    for property in [0, 2, 0xffff, 0x10001, HEADER, u32::MAX] {
        denied(&mut p, SET, &[device, property, 0x5000_0000]);
    }
    assert_eq!(call(&mut p, SET, &[device, 1, 0]), 0x8007_0057);
    for header in [
        [0, 16, 0, 0, 16],
        [19, 16, 0, 0, 16],
        [21, 16, 0, 0, 16],
        [20, 0, 0, 0, 16],
        [20, 15, 0, 0, 16],
        [20, 17, 0, 0, 16],
        [20, 16, 1, 0, 16],
    ] {
        words(&mut p, HEADER, &header);
        assert_eq!(call(&mut p, SET, &[device, 1, HEADER]), 0x8007_0057);
    }
    for how in [1, 2, 3, 4, u32::MAX] {
        words(&mut p, HEADER, &[20, 16, 1, how, 16]);
        denied(&mut p, SET, &[device, 1, HEADER]);
    }
    words(&mut p, HEADER, &[20, 16, 0, 0, 16]);
    assert_eq!(call(&mut p, SET, &[device, 1, HEADER]), 0);
    assert_eq!(call(&mut p, RELEASE_MOUSE, &[device]), 0);
    denied(&mut p, SET, &[device, 1, 0]);
    assert_eq!(call(&mut p, RELEASE, &[keyboard]), 0);
}

#[test]
fn prefix_sizes_precede_missing_body_and_cross_page_fault_recovers_on_same_mouse() {
    let (mut p, _, device) = setup();
    for prefix in [[20, 15], [19, 16]] {
        words(&mut p, 0x3000_0ff8, &prefix);
        assert_eq!(call(&mut p, SET, &[device, 1, 0x3000_0ff8]), 0x8007_0057);
    }
    words(&mut p, 0x3000_0ff8, &[20, 16]);
    for header in [0x3000_0ff8, 0x3000_0ffd, 0x5000_0000] {
        prepare(&mut p, SET, &[device, 1, header]);
        fault(&mut p);
    }
    p.memory
        .map_zeroed(0x3000_1000, 4096, Permissions::NONE)
        .unwrap();
    prepare(&mut p, SET, &[device, 1, 0x3000_0ff8]);
    fault(&mut p);
    p.memory
        .protect(0x3000_1000, 4096, Permissions::READ_WRITE)
        .unwrap();
    words(&mut p, 0x3000_1000, &[0, 0, 32]);
    assert_eq!(call(&mut p, SET, &[device, 1, 0x3000_0ff8]), 0);
    p.memory
        .map_zeroed(0xffff_f000, 4096, Permissions::READ_WRITE)
        .unwrap();
    words(&mut p, 0xffff_ffed, &[20, 16]);
    prepare(&mut p, SET, &[device, 1, 0xffff_ffed]);
    refused(
        &mut p,
        &ProcessStop::Stopped(StopReason::MemoryFault(MemoryError::AddressOverflow)),
    );
    words(&mut p, 0xffff_ffec, &[20, 16, 0, 0, 64]);
    assert_eq!(call(&mut p, SET, &[device, 1, 0xffff_ffec]), 0);
    assert_eq!(call(&mut p, RELEASE_MOUSE, &[device]), 0);
}

#[test]
fn checked_frames_and_zero_budget_do_not_apply_the_setting() {
    let (mut p, _, device) = setup();
    let before = prepare(&mut p, SET, &[device, 1, HEADER]);
    let run = p.run(0);
    assert_eq!((run.instructions, run.api_calls), (0, 0));
    assert_eq!(p.cpu, before);
    put(&mut p, 0x1000_fffc, CODE);
    p.cpu.set_register(Register32::Esp, 0x1000_fffc);
    fault(&mut p);
    p.memory
        .map_zeroed(0xffff_f000, 4096, Permissions::READ_WRITE)
        .unwrap();
    words(&mut p, 0xffff_fff0, &[CODE, device, 1, HEADER]);
    p.cpu.set_register(Register32::Esp, 0xffff_fff0);
    refused(
        &mut p,
        &ProcessStop::Stopped(StopReason::MemoryFault(MemoryError::AddressOverflow)),
    );
    assert_eq!(call(&mut p, SET, &[device, 1, HEADER]), 0);
    assert_eq!(call(&mut p, RELEASE_MOUSE, &[device]), 0);
}

#[test]
fn foreign_and_scheduled_children_cannot_set_mouse_properties() {
    let (mut p, _, device) = setup();
    for fs in [0, 0x1101_0000, 0x1234_0000] {
        p.cpu.set_fs_base(fs);
        p.cpu.eip = SET;
        p.cpu.set_register(Register32::Esp, 0x5000_0000);
        refused(&mut p, &ProcessStop::UnsupportedApi { address: SET });
    }
    p.cpu.set_fs_base(0x7ffd_e000);
    assert_eq!(call(&mut p, SET, &[device, 1, HEADER]), 0);
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
