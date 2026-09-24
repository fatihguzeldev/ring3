#[allow(dead_code)]
#[path = "support/dinput_device_cases.rs"]
mod dinput_device_cases;
#[path = "support/dinput_mouse_caps_cases.rs"]
mod dinput_mouse_caps_cases;
#[allow(dead_code)]
#[path = "support/dinput_mouse_cases.rs"]
mod dinput_mouse_cases;
#[path = "support/imported_executable.rs"]
mod imported_executable;

#[test]
fn imported_mouse_capability_layouts_match_across_budgets() {
    dinput_mouse_caps_cases::imported_mouse_capabilities_across_budgets();
}

#[allow(dead_code)]
#[path = "support/dinput_device_calls.rs"]
mod dinput_device_calls;
use dinput_device_calls::*;
use dinput_mouse_caps_cases::CAPS;

const GET: u32 = 0x7000_05a0;
const RELEASE_MOUSE: u32 = 0x7000_0594;

fn setup() -> (Process32, u32, u32) {
    let mut p = process();
    let root = root(&mut p);
    p.memory
        .write(u64::from(GUID), &dinput_mouse_cases::MOUSE)
        .unwrap();
    assert_eq!(call(&mut p, CREATE, &[root, GUID, DATA, 0]), 0);
    let device = word(&p, DATA);
    (p, root, device)
}

fn expected(size: u32) -> Vec<u8> {
    [size, 5, 0x202, 3, 8, 0, 0, 0, 0, 0, 0]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .take(size as usize)
        .collect()
}

fn bytes(p: &Process32, address: u32, count: usize) -> Vec<u8> {
    let mut result = vec![0; count];
    p.memory.read(u64::from(address), &mut result).unwrap();
    result
}

#[test]
fn aliases_and_root_retirement_do_not_change_caps_refs_pages_or_error_storage() {
    let (mut p, root, device) = setup();
    p.memory
        .write(u64::from(IID), &dinput_device_cases::DEVICE)
        .unwrap();
    assert_eq!(call(&mut p, 0x7000_058c, &[device, IID, DATA]), 0);
    let alias = word(&p, DATA);
    assert_eq!(call(&mut p, ROOT_RELEASE, &[root]), 0);
    put(&mut p, 0x7ffd_e034, 77);
    put(&mut p, 0x7ffd_e040, 88);
    p.memory
        .protect(0x7ffd_e000, 4096, Permissions::NONE)
        .unwrap();
    let pages = p.memory.mapped_pages();
    for size in [24, 44, 24] {
        p.memory.write(u64::from(CAPS - 1), &[0xa5; 46]).unwrap();
        put(&mut p, CAPS, size);
        assert_eq!(call(&mut p, GET, &[alias, CAPS]), 0);
        assert_eq!(bytes(&p, CAPS, size as usize), expected(size));
        assert_eq!(bytes(&p, CAPS - 1, 1), [0xa5]);
        assert!(
            bytes(&p, CAPS + size, 45 - size as usize)
                .iter()
                .all(|b| *b == 0xa5)
        );
    }
    assert_eq!(p.memory.mapped_pages(), pages);
    assert_eq!(call(&mut p, RELEASE_MOUSE, &[device]), 1);
    assert_eq!(call(&mut p, RELEASE_MOUSE, &[alias]), 0);
    p.memory
        .protect(0x7ffd_e000, 4096, Permissions::READ_WRITE)
        .unwrap();
    assert_eq!(p.last_error().unwrap(), 77);
    assert_eq!(word(&p, 0x7ffd_e040), 88);
}

#[test]
fn null_and_size_prefix_errors_precede_output_body_checks() {
    let (mut p, _, device) = setup();
    assert_eq!(call(&mut p, GET, &[device, 0]), 0x8000_4003);
    p.memory
        .map_zeroed(0x3000_0000, 4096, Permissions::READ_WRITE)
        .unwrap();
    for size in [0, 4, 23, 25, 43, 45, u32::MAX] {
        p.memory
            .protect(0x3000_0000, 4096, Permissions::READ_WRITE)
            .unwrap();
        put(&mut p, 0x3000_0ffc, size);
        p.memory
            .protect(0x3000_0000, 4096, Permissions::READ)
            .unwrap();
        assert_eq!(call(&mut p, GET, &[device, 0x3000_0ffc]), 0x8007_0057);
        assert_eq!(word(&p, 0x3000_0ffc), size);
    }
    p.memory
        .protect(0x3000_0000, 4096, Permissions::READ_WRITE)
        .unwrap();
    put(&mut p, 0x3000_0001, 44);
    p.memory
        .protect(0x3000_0000, 4096, Permissions::READ)
        .unwrap();
    prepare(&mut p, GET, &[device, 0x3000_0001]);
    fault(&mut p);
    assert_eq!(word(&p, 0x3000_0001), 44);
    for output in [0x5000_0000, u32::MAX - 1] {
        prepare(&mut p, GET, &[device, output]);
        fault(&mut p);
    }
    put(&mut p, CAPS, 44);
    assert_eq!(call(&mut p, GET, &[device, CAPS]), 0);
    assert_eq!(call(&mut p, RELEASE_MOUSE, &[device]), 0);
}

#[test]
fn cross_page_write_faults_leave_the_writable_prefix_intact_and_recover() {
    let (mut p, _, device) = setup();
    p.memory
        .map_zeroed(0x3000_0000, 4096, Permissions::READ_WRITE)
        .unwrap();
    p.memory.write(0x3000_0ff0, &[0xa5; 16]).unwrap();
    put(&mut p, 0x3000_0ff0, 44);
    let before = bytes(&p, 0x3000_0ff0, 16);
    prepare(&mut p, GET, &[device, 0x3000_0ff0]);
    fault(&mut p);
    assert_eq!(bytes(&p, 0x3000_0ff0, 16), before);
    p.memory
        .map_zeroed(0x3000_1000, 4096, Permissions::READ)
        .unwrap();
    prepare(&mut p, GET, &[device, 0x3000_0ff0]);
    fault(&mut p);
    assert_eq!(bytes(&p, 0x3000_0ff0, 16), before);
    assert_eq!(bytes(&p, 0x3000_1000, 28), [0; 28]);
    p.memory
        .protect(
            0x3000_1000,
            4096,
            Permissions {
                write: true,
                ..Permissions::NONE
            },
        )
        .unwrap();
    assert_eq!(call(&mut p, GET, &[device, 0x3000_0ff0]), 0);
    p.memory
        .protect(0x3000_1000, 4096, Permissions::READ_WRITE)
        .unwrap();
    assert_eq!(bytes(&p, 0x3000_0ff0, 44), expected(44));
    assert_eq!(call(&mut p, RELEASE_MOUSE, &[device]), 0);
}

#[test]
fn legacy_extent_and_overflow_checks_do_not_touch_unavailable_extension() {
    let (mut p, _, device) = setup();
    p.memory
        .map_zeroed(0x3000_0000, 4096, Permissions::READ_WRITE)
        .unwrap();
    put(&mut p, 0x3000_0fe8, 24);
    assert_eq!(call(&mut p, GET, &[device, 0x3000_0fe8]), 0);
    assert_eq!(bytes(&p, 0x3000_0fe8, 24), expected(24));
    put(&mut p, 0x3000_0fe8, 44);
    let before = bytes(&p, 0x3000_0fe8, 24);
    prepare(&mut p, GET, &[device, 0x3000_0fe8]);
    fault(&mut p);
    assert_eq!(bytes(&p, 0x3000_0fe8, 24), before);
    p.memory
        .map_zeroed(0xffff_f000, 4096, Permissions::READ_WRITE)
        .unwrap();
    put(&mut p, 0xffff_fffc, 44);
    prepare(&mut p, GET, &[device, 0xffff_fffc]);
    fault(&mut p);
    assert_eq!(word(&p, 0xffff_fffc), 44);
    assert_eq!(call(&mut p, RELEASE_MOUSE, &[device]), 0);
}

#[test]
fn receiver_and_checked_frame_guards_precede_output_and_live_return_alias_is_defined() {
    let (mut p, root, device) = setup();
    let keyboard = create(&mut p, root);
    for receiver in [0, root, keyboard, device + 1, device + 4, u32::MAX] {
        denied(&mut p, GET, &[receiver, 0]);
    }
    put(&mut p, CAPS, 44);
    let before = prepare(&mut p, GET, &[device, CAPS]);
    let result = p.run(0);
    assert_eq!((result.instructions, result.api_calls), (0, 0));
    assert_eq!(p.cpu, before);
    assert_eq!(word(&p, CAPS), 44);
    put(&mut p, 0x1000_fffc, CODE);
    p.cpu.set_register(Register32::Esp, 0x1000_fffc);
    fault(&mut p);
    p.memory
        .map_zeroed(0xffff_f000, 4096, Permissions::READ_WRITE)
        .unwrap();
    for (address, value) in [
        (0xffff_fff4, CODE),
        (0xffff_fff8, device),
        (0xffff_fffc, CAPS),
    ] {
        put(&mut p, address, value);
    }
    p.cpu.set_register(Register32::Esp, 0xffff_fff4);
    refused(
        &mut p,
        &ProcessStop::Stopped(StopReason::MemoryFault(MemoryError::AddressOverflow)),
    );
    put(&mut p, STACK - 4, 44);
    assert_eq!(call(&mut p, GET, &[device, STACK - 4]), 0);
    assert_eq!(p.cpu.eip, 5);
    assert_eq!(bytes(&p, STACK - 4, 44), expected(44));
    assert_eq!(call(&mut p, RELEASE_MOUSE, &[device]), 0);
    denied(&mut p, GET, &[device, 0]);
    assert_eq!(call(&mut p, RELEASE, &[keyboard]), 0);
}

#[test]
fn foreign_and_scheduled_child_identity_stops_before_output_reads() {
    let (mut p, _, device) = setup();
    for fs in [0, 0x1101_0000, 0x1234_0000] {
        p.cpu.set_fs_base(fs);
        p.cpu.eip = GET;
        p.cpu.set_register(Register32::Esp, 0x5000_0000);
        refused(&mut p, &ProcessStop::UnsupportedApi { address: GET });
    }
    p.cpu.set_fs_base(0x7ffd_e000);
    put(&mut p, CAPS, 44);
    assert_eq!(call(&mut p, GET, &[device, CAPS]), 0);
    let child = call(&mut p, 0x7000_0548, &[0, 0, CODE, 0, 4, 0]);
    assert_eq!(call(&mut p, 0x7000_0550, &[child]), 1);
    assert_eq!(
        p.run(100).reason,
        ProcessStop::Stopped(StopReason::Breakpoint)
    );
    assert_eq!(p.cpu.fs_base(), 0x1101_0000);
    p.cpu.eip = GET;
    p.cpu.set_register(Register32::Esp, 0x5000_0000);
    refused(&mut p, &ProcessStop::UnsupportedApi { address: GET });
}
