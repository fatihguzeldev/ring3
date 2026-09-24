#[allow(dead_code)]
#[path = "support/dinput_device_cases.rs"]
mod dinput_device_cases;
#[allow(dead_code)]
#[path = "support/dinput_format_cases.rs"]
mod dinput_format_cases;
#[allow(dead_code)]
#[path = "support/dinput_mouse_cases.rs"]
mod dinput_mouse_cases;
#[allow(dead_code)]
#[path = "support/dinput_mouse_format_cases.rs"]
mod dinput_mouse_format_cases;
#[path = "support/dinput_mouse_granularity_cases.rs"]
mod dinput_mouse_granularity_cases;
#[path = "support/imported_executable.rs"]
mod imported_executable;

#[test]
fn imported_mouse_axis_granularity_matches_across_budgets() {
    dinput_mouse_granularity_cases::imported_mouse_axis_granularity_across_budgets();
}

#[allow(dead_code)]
#[path = "support/dinput_device_calls.rs"]
mod dinput_device_calls;
use dinput_device_calls::*;
use dinput_mouse_granularity_cases::HEADER;

const GET: u32 = 0x7000_05a8;
const RELEASE_MOUSE: u32 = 0x7000_0594;

fn words(p: &mut Process32, address: u32, values: &[u32]) {
    let bytes: Vec<_> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
    p.memory.write(u64::from(address), &bytes).unwrap();
}

fn setup(configured: bool) -> (Process32, u32, u32) {
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
    dinput_mouse_format_cases::standard(&mut p.memory, false);
    if configured {
        configure(&mut p, device);
    }
    (p, root, device)
}

fn configure(p: &mut Process32, device: u32) {
    assert_eq!(
        call(p, 0x7000_0598, &[device, dinput_mouse_format_cases::FORMAT]),
        0
    );
}

#[test]
fn aliases_query_owned_format_after_root_and_source_retire_without_other_effects() {
    let (mut p, root, device) = setup(true);
    p.memory
        .write(u64::from(IID), &dinput_device_cases::DEVICE)
        .unwrap();
    assert_eq!(call(&mut p, 0x7000_058c, &[device, IID, DATA]), 0);
    let alias = word(&p, DATA);
    assert_eq!(call(&mut p, ROOT_RELEASE, &[root]), 0);
    p.memory.write(0x3000_0000, &[0; 4096]).unwrap();
    assert_eq!(
        call(
            &mut p,
            0x7000_0598,
            &[device, dinput_mouse_format_cases::FORMAT]
        ),
        0x8007_0057
    );
    p.memory
        .protect(0x3000_0000, 4096, Permissions::NONE)
        .unwrap();
    put(&mut p, 0x7ffd_e034, 77);
    put(&mut p, 0x7ffd_e040, 88);
    p.memory
        .protect(0x7ffd_e000, 4096, Permissions::NONE)
        .unwrap();
    let pages = p.memory.mapped_pages();
    let windows = p.window_snapshots();
    for (offset, value) in [(8, 120), (0, 1), (4, 1), (8, 120)] {
        p.memory.write(u64::from(HEADER - 1), &[0xa5; 22]).unwrap();
        words(&mut p, HEADER, &[20, 16, offset, 1]);
        assert_eq!(call(&mut p, GET, &[alias, 3, HEADER]), 0);
        let mut result = [0; 22];
        p.memory.read(u64::from(HEADER - 1), &mut result).unwrap();
        let expected: Vec<_> = [20_u32, 16, offset, 1, value]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect();
        assert_eq!(&result[1..21], expected);
        assert_eq!((result[0], result[21]), (0xa5, 0xa5));
    }
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
fn property_sizes_modes_and_owned_format_precede_output_checks() {
    let (mut p, _, device) = setup(false);
    for property in [0, 1, 2, 4, HEADER, u32::MAX] {
        denied(&mut p, GET, &[device, property, 0x5000_0000]);
    }
    assert_eq!(call(&mut p, GET, &[device, 3, 0]), 0x8007_0057);
    for prefix in [[0, 16], [19, 16], [21, 16], [20, 0], [20, 15], [20, 17]] {
        words(&mut p, 0x3000_0ff8, &prefix);
        assert_eq!(call(&mut p, GET, &[device, 3, 0x3000_0ff8]), 0x8007_0057);
    }
    for how in [0, 2, 3] {
        words(&mut p, 0x3000_0ff0, &[20, 16, 8, how]);
        denied(&mut p, GET, &[device, 3, 0x3000_0ff0]);
    }
    for how in [4, u32::MAX] {
        words(&mut p, 0x3000_0ff0, &[20, 16, 8, how]);
        assert_eq!(call(&mut p, GET, &[device, 3, 0x3000_0ff0]), 0x8007_0057);
    }
    for offset in [0, 8, 12, u32::MAX] {
        words(&mut p, 0x3000_0ff0, &[20, 16, offset, 1]);
        assert_eq!(call(&mut p, GET, &[device, 3, 0x3000_0ff0]), 0x8007_0002);
    }
    configure(&mut p, device);
    for offset in 12..20 {
        words(&mut p, 0x3000_0ff0, &[20, 16, offset, 1]);
        denied(&mut p, GET, &[device, 3, 0x3000_0ff0]);
    }
    for offset in [1, 3, 5, 7, 9, 11, 20, u32::MAX] {
        words(&mut p, 0x3000_0ff0, &[20, 16, offset, 1]);
        assert_eq!(call(&mut p, GET, &[device, 3, 0x3000_0ff0]), 0x8007_0002);
    }
    words(&mut p, HEADER, &[20, 16, 8, 1, 77]);
    assert_eq!(call(&mut p, GET, &[device, 3, HEADER]), 0);
    assert_eq!(word(&p, HEADER + 16), 120);
    assert_eq!(call(&mut p, RELEASE_MOUSE, &[device]), 0);
}

#[test]
fn readonly_header_and_write_only_result_have_independent_permissions() {
    let (mut p, _, device) = setup(true);
    p.memory
        .map_zeroed(0x3000_1000, 8192, Permissions::READ_WRITE)
        .unwrap();
    words(&mut p, 0x3000_1ff0, &[20, 16, 8, 1]);
    p.memory.write(0x3000_2000, &[0xa5; 5]).unwrap();
    p.memory
        .protect(0x3000_1000, 4096, Permissions::READ)
        .unwrap();
    p.memory
        .protect(
            0x3000_2000,
            4096,
            Permissions {
                write: true,
                ..Permissions::NONE
            },
        )
        .unwrap();
    assert_eq!(call(&mut p, GET, &[device, 3, 0x3000_1ff0]), 0);
    p.memory
        .protect(0x3000_2000, 4096, Permissions::READ)
        .unwrap();
    assert_eq!(word(&p, 0x3000_2000), 120);
    let mut fence = [0];
    p.memory.read(0x3000_2004, &mut fence).unwrap();
    assert_eq!(fence, [0xa5]);
    assert_eq!(word(&p, 0x3000_1ff0), 20);
    assert_eq!(word(&p, 0x3000_1ff4), 16);
    assert_eq!(word(&p, 0x3000_1ff8), 8);
    assert_eq!(word(&p, 0x3000_1ffc), 1);
    prepare(&mut p, GET, &[device, 3, 0x3000_1ff0]);
    fault(&mut p);
    assert_eq!(word(&p, 0x3000_2000), 120);
    assert_eq!(call(&mut p, RELEASE_MOUSE, &[device]), 0);
}

#[test]
fn output_cross_page_failure_is_atomic_and_same_mouse_recovers() {
    let (mut p, _, device) = setup(true);
    words(&mut p, 0x3000_0fee, &[20, 16, 8, 1]);
    p.memory.write(0x3000_0ffe, &[0xa5; 2]).unwrap();
    for mapped in [false, true] {
        if mapped {
            p.memory
                .map_zeroed(0x3000_1000, 4096, Permissions::READ)
                .unwrap();
        }
        prepare(&mut p, GET, &[device, 3, 0x3000_0fee]);
        fault(&mut p);
        let mut prefix = [0; 2];
        p.memory.read(0x3000_0ffe, &mut prefix).unwrap();
        assert_eq!(prefix, [0xa5; 2]);
    }
    assert_eq!(word(&p, 0x3000_1000), 0);
    p.memory
        .protect(0x3000_1000, 4096, Permissions::READ_WRITE)
        .unwrap();
    assert_eq!(call(&mut p, GET, &[device, 3, 0x3000_0fee]), 0);
    assert_eq!(word(&p, 0x3000_0ffe), 120);
    p.memory
        .map_zeroed(0xffff_f000, 4096, Permissions::READ_WRITE)
        .unwrap();
    words(&mut p, 0xffff_fff0, &[20, 16, 8, 1]);
    prepare(&mut p, GET, &[device, 3, 0xffff_fff0]);
    refused(
        &mut p,
        &ProcessStop::Stopped(StopReason::MemoryFault(MemoryError::AddressOverflow)),
    );
    assert_eq!(word(&p, 0xffff_fffc), 1);
    words(&mut p, 0xffff_ffec, &[20, 16, 8, 1, 77]);
    assert_eq!(call(&mut p, GET, &[device, 3, 0xffff_ffec]), 0);
    assert_eq!(word(&p, 0xffff_fffc), 120);
    assert_eq!(call(&mut p, RELEASE_MOUSE, &[device]), 0);
}

#[test]
fn incomplete_header_reads_do_not_publish_and_retry_succeeds() {
    let (mut p, _, device) = setup(true);
    words(&mut p, 0x3000_0ff8, &[20, 16]);
    for address in [0x3000_0ff8, 0x3000_0ffd, 0x5000_0000, u32::MAX - 1] {
        prepare(&mut p, GET, &[device, 3, address]);
        fault(&mut p);
    }
    words(&mut p, HEADER, &[20, 16, 4, 1, 77]);
    assert_eq!(call(&mut p, GET, &[device, 3, HEADER]), 0);
    assert_eq!(word(&p, HEADER + 16), 1);
    assert_eq!(call(&mut p, RELEASE_MOUSE, &[device]), 0);
}

#[test]
fn identity_checked_frames_and_live_return_alias_are_enforced() {
    let (mut p, root, device) = setup(true);
    let keyboard = create(&mut p, root);
    for receiver in [0, root, keyboard, device + 1, device + 4, u32::MAX] {
        denied(&mut p, GET, &[receiver, 3, 0]);
    }
    words(&mut p, HEADER, &[20, 16, 8, 1, 77]);
    let before = prepare(&mut p, GET, &[device, 3, HEADER]);
    let run = p.run(0);
    assert_eq!((run.instructions, run.api_calls), (0, 0));
    assert_eq!(p.cpu, before);
    assert_eq!(word(&p, HEADER + 16), 77);
    put(&mut p, 0x1000_fffc, CODE);
    p.cpu.set_register(Register32::Esp, 0x1000_fffc);
    fault(&mut p);
    p.memory
        .map_zeroed(0xffff_f000, 4096, Permissions::READ_WRITE)
        .unwrap();
    words(&mut p, 0xffff_fff0, &[CODE, device, 3, HEADER]);
    p.cpu.set_register(Register32::Esp, 0xffff_fff0);
    refused(
        &mut p,
        &ProcessStop::Stopped(StopReason::MemoryFault(MemoryError::AddressOverflow)),
    );
    assert_eq!(word(&p, HEADER + 16), 77);
    words(&mut p, STACK - 16, &[20, 16, 8, 1]);
    assert_eq!(call(&mut p, GET, &[device, 3, STACK - 16]), 0);
    assert_eq!(p.cpu.eip, 120);
    assert_eq!(call(&mut p, RELEASE_MOUSE, &[device]), 0);
    denied(&mut p, GET, &[device, 3, 0]);
    assert_eq!(call(&mut p, RELEASE, &[keyboard]), 0);
}

#[test]
fn foreign_and_scheduled_child_identity_stops_before_header_reads() {
    let (mut p, _, device) = setup(true);
    for fs in [0, 0x1101_0000, 0x1234_0000] {
        p.cpu.set_fs_base(fs);
        p.cpu.eip = GET;
        p.cpu.set_register(Register32::Esp, 0x5000_0000);
        refused(&mut p, &ProcessStop::UnsupportedApi { address: GET });
    }
    p.cpu.set_fs_base(0x7ffd_e000);
    words(&mut p, HEADER, &[20, 16, 8, 1, 77]);
    assert_eq!(call(&mut p, GET, &[device, 3, HEADER]), 0);
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
