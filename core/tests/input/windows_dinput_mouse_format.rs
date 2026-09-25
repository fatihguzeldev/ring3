use super::dinput_mouse_cases;
use super::dinput_mouse_format_cases;
use super::imported_executable;

#[test]
fn imported_standard_mouse_setup_matches_across_budgets() {
    dinput_mouse_format_cases::imported_standard_mouse_format_across_budgets();
}

use super::dinput_device_calls;
use dinput_device_calls::*;
use dinput_mouse_format_cases::{FORMAT, GUIDS, OBJECTS, standard};

const SET: u32 = 0x7000_0598;
const RELEASE_MOUSE: u32 = 0x7000_0594;

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
    standard(&mut p.memory, false);
    (p, root, device)
}

#[test]
fn read_only_unaligned_format_works_at_full_page_budget_without_error_storage() {
    let bytes = imported_executable::pe32(&[0xcc], "dinput.dll", &["DirectInputCreateA"]);
    let pages = process().memory.mapped_pages() + 2;
    let mut p = Process32::load(&bytes, u32::try_from(pages).unwrap()).unwrap();
    let root = root(&mut p);
    p.memory
        .write(u64::from(GUID), &dinput_mouse_cases::MOUSE)
        .unwrap();
    assert_eq!(call(&mut p, CREATE, &[root, GUID, DATA, 0]), 0);
    let device = word(&p, DATA);
    p.memory
        .map_zeroed(0x3000_0000, 4096, Permissions::READ_WRITE)
        .unwrap();
    standard(&mut p.memory, true);
    p.memory
        .protect(0x3000_0000, 4096, Permissions::READ)
        .unwrap();
    put(&mut p, 0x7ffd_e034, 77);
    put(&mut p, 0x7ffd_e040, 88);
    p.memory
        .protect(0x7ffd_e000, 4096, Permissions::NONE)
        .unwrap();
    assert_eq!(call(&mut p, ROOT_RELEASE, &[root]), 0);
    assert_eq!(call(&mut p, SET, &[device, FORMAT]), 0);
    assert_eq!(call(&mut p, SET, &[device, 0]), 0x8000_4003);
    assert_eq!(call(&mut p, RELEASE_MOUSE, &[device]), 0);
    assert_eq!(p.memory.mapped_pages(), pages);
    p.memory
        .protect(0x7ffd_e000, 4096, Permissions::READ_WRITE)
        .unwrap();
    assert_eq!(p.last_error().unwrap(), 77);
    assert_eq!(word(&p, 0x7ffd_e040), 88);
}

#[test]
fn header_size_prefixes_precede_missing_tail_and_unsupported_header_bounds_array_reads() {
    let (mut p, _, device) = setup();
    assert_eq!(call(&mut p, SET, &[device, 0]), 0x8000_4003);
    put(&mut p, 0x1000_fffc, 0);
    assert_eq!(call(&mut p, SET, &[device, 0x1000_fffc]), 0x8007_0057);
    put(&mut p, 0x1000_fffc, 24);
    prepare(&mut p, SET, &[device, 0x1000_fffc]);
    fault(&mut p);
    put(&mut p, 0x1000_fff8, 24);
    put(&mut p, 0x1000_fffc, 0);
    assert_eq!(call(&mut p, SET, &[device, 0x1000_fff8]), 0x8007_0057);
    put(&mut p, 0x1000_fffc, 16);
    prepare(&mut p, SET, &[device, 0x1000_fff8]);
    fault(&mut p);
    for (offset, value) in [
        (8, 0),
        (8, 1),
        (8, 3),
        (12, 16),
        (12, u32::MAX),
        (16, 0),
        (16, 7),
        (16, 12),
        (16, u32::MAX),
    ] {
        standard(&mut p.memory, false);
        put(&mut p, FORMAT + offset, value);
        put(&mut p, FORMAT + 20, 0x5000_0000);
        denied(&mut p, SET, &[device, FORMAT]);
    }
    standard(&mut p.memory, false);
    assert_eq!(call(&mut p, SET, &[device, FORMAT]), 0);
    assert_eq!(call(&mut p, RELEASE_MOUSE, &[device]), 0);
}

#[test]
fn unsupported_object_selectors_types_offsets_order_and_flags_do_not_change_refs() {
    let (mut p, _, device) = setup();
    for index in [0, 2, 3, 10] {
        for (field, value) in [(4, 20), (8, 0), (8, 0x8000_000c), (12, 1)] {
            standard(&mut p.memory, false);
            put(&mut p, OBJECTS + index * 16 + field, value);
            denied(&mut p, SET, &[device, FORMAT]);
        }
    }
    for guid in [0, GUIDS + 16, GUIDS + 48] {
        standard(&mut p.memory, false);
        put(&mut p, OBJECTS, guid);
        denied(&mut p, SET, &[device, FORMAT]);
    }
    for index in [3, 10] {
        standard(&mut p.memory, false);
        put(&mut p, OBJECTS + index * 16, GUIDS);
        denied(&mut p, SET, &[device, FORMAT]);
    }
    standard(&mut p.memory, false);
    let mut first = [0; 16];
    let mut second = [0; 16];
    p.memory.read(u64::from(OBJECTS + 48), &mut first).unwrap();
    p.memory.read(u64::from(OBJECTS + 64), &mut second).unwrap();
    p.memory.write(u64::from(OBJECTS + 48), &second).unwrap();
    p.memory.write(u64::from(OBJECTS + 64), &first).unwrap();
    denied(&mut p, SET, &[device, FORMAT]);
    standard(&mut p.memory, true);
    assert_eq!(call(&mut p, SET, &[device, FORMAT]), 0);
    assert_eq!(call(&mut p, RELEASE_MOUSE, &[device]), 0);
}

#[test]
fn full_array_and_guid_spans_preflight_before_record_tuples_and_publication() {
    let (mut p, _, device) = setup();
    for format in [0x5000_0000, u32::MAX - 1] {
        prepare(&mut p, SET, &[device, format]);
        fault(&mut p);
    }
    for array in [0, 0x5000_0000, 0x3000_0f51, u32::MAX - 174] {
        standard(&mut p.memory, false);
        put(&mut p, FORMAT + 20, array);
        prepare(&mut p, SET, &[device, FORMAT]);
        fault(&mut p);
    }
    for index in [0, 10] {
        for guid in [0x5000_0000, 0x1000_fff8, u32::MAX - 7] {
            standard(&mut p.memory, false);
            put(&mut p, OBJECTS + index * 16, guid);
            put(&mut p, OBJECTS + index * 16 + 12, 1);
            prepare(&mut p, SET, &[device, FORMAT]);
            fault(&mut p);
        }
    }
    standard(&mut p.memory, false);
    assert_eq!(call(&mut p, SET, &[device, FORMAT]), 0);
    assert_eq!(call(&mut p, RELEASE_MOUSE, &[device]), 0);
}

#[test]
fn receiver_and_checked_frames_precede_format_reads_and_cannot_consume_refs() {
    let (mut p, root, device) = setup();
    let keyboard = create(&mut p, root);
    for receiver in [0, root, keyboard, device + 1, device + 4, u32::MAX] {
        denied(&mut p, SET, &[receiver, 0x5000_0000]);
    }
    denied(&mut p, 0x7000_0578, &[device, 0x5000_0000]);
    let before = prepare(&mut p, SET, &[device, FORMAT]);
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
        (0xffff_fff4, CODE),
        (0xffff_fff8, device),
        (0xffff_fffc, FORMAT),
    ] {
        put(&mut p, address, value);
    }
    p.cpu.set_register(Register32::Esp, 0xffff_fff4);
    refused(
        &mut p,
        &ProcessStop::Stopped(StopReason::MemoryFault(MemoryError::AddressOverflow)),
    );
    assert_eq!(call(&mut p, SET, &[device, FORMAT]), 0);
    assert_eq!(call(&mut p, RELEASE_MOUSE, &[device]), 0);
    denied(&mut p, SET, &[device, 0]);
    assert_eq!(call(&mut p, RELEASE, &[keyboard]), 0);
}

#[test]
fn foreign_and_scheduled_child_identity_cannot_configure_mouse() {
    let (mut p, _, device) = setup();
    for fs in [0, 0x1101_0000, 0x1234_0000] {
        p.cpu.set_fs_base(fs);
        p.cpu.eip = SET;
        p.cpu.set_register(Register32::Esp, 0x5000_0000);
        refused(&mut p, &ProcessStop::UnsupportedApi { address: SET });
    }
    p.cpu.set_fs_base(0x7ffd_e000);
    assert_eq!(call(&mut p, SET, &[device, FORMAT]), 0);
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
