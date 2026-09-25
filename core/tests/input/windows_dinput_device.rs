use super::dinput_device_cases;
use super::imported_executable;

#[test]
fn imported_keyboard_vtable_lifetime_survives_root_across_budgets() {
    dinput_device_cases::imported_keyboard_lifetime_across_budgets();
}

use super::dinput_device_calls;
use dinput_device_calls::*;
use dinput_device_cases::DEVICE;
use ring3_core::execution::Access;

#[test]
fn device_aliases_and_lifetimes_are_independent_of_roots_and_other_devices() {
    let mut p = process();
    let first_root = root(&mut p);
    let second_root = root(&mut p);
    let first = create(&mut p, first_root);
    let second = create(&mut p, second_root);
    assert_eq!(call(&mut p, ROOT_RELEASE, &[first_root]), 0);
    for text in [
        "0000000000000000c000000000000046",
        "80e644592ec9cf11bfc7444553540000",
        "82e644592ec9cf11bfc7444553540000",
        "bcc6d7575623d3118e9d00c04f6844ae",
    ] {
        let bytes: Vec<_> = (0..16)
            .map(|i| u8::from_str_radix(&text[i * 2..i * 2 + 2], 16).unwrap())
            .collect();
        p.memory.write(u64::from(IID), &bytes).unwrap();
        assert_eq!(call(&mut p, QUERY, &[first, IID, DATA]), 0);
        assert_eq!(word(&p, DATA), first);
    }
    assert_eq!(call(&mut p, ADD, &[second]), 2);
    for count in (0..5).rev() {
        assert_eq!(call(&mut p, RELEASE, &[first]), count);
    }
    assert_eq!(call(&mut p, RELEASE, &[second]), 1);
    assert_eq!(call(&mut p, ROOT_RELEASE, &[second_root]), 0);
    assert_eq!(call(&mut p, RELEASE, &[second]), 0);
    let third_root = root(&mut p);
    assert_eq!(create(&mut p, third_root), second + 4);
    assert_eq!(third_root, second_root + 4);
    for pointer in [
        0,
        first,
        second,
        first + 1,
        second + 2,
        third_root,
        PAGE,
        u32::MAX,
    ] {
        denied(&mut p, QUERY, &[pointer, 0, 0]);
        denied(&mut p, ADD, &[pointer]);
        denied(&mut p, RELEASE, &[pointer]);
    }
    for pointer in [
        0,
        first_root,
        second_root,
        third_root + 1,
        first,
        second,
        u32::MAX,
    ] {
        denied(&mut p, CREATE, &[pointer, 0, 0, 0]);
    }
}

#[test]
fn root_and_device_interfaces_do_not_cross_classes() {
    let mut p = process();
    let root = root(&mut p);
    let device = create(&mut p, root);
    p.memory.write(u64::from(IID), &DEVICE).unwrap();
    assert_eq!(call(&mut p, ROOT_QUERY, &[root, IID, DATA]), 0x8000_4002);
    for text in [
        "601352898aaacf11bfc7444553540000",
        "80d4105415dc3348a41b748f73a38179",
        "81e644592ec9cf11bfc7444553540000",
        "00000000000000000000000000000000",
    ] {
        let bytes: Vec<_> = (0..16)
            .map(|i| u8::from_str_radix(&text[i * 2..i * 2 + 2], 16).unwrap())
            .collect();
        p.memory.write(u64::from(IID), &bytes).unwrap();
        assert_eq!(call(&mut p, QUERY, &[device, IID, DATA]), 0x8000_4002);
        assert_eq!(word(&p, DATA), 0);
    }
    denied(&mut p, ROOT_QUERY, &[device, IID, DATA]);
    denied(&mut p, ROOT_RELEASE, &[device]);
    assert_eq!(call(&mut p, RELEASE, &[device]), 0);
    assert_eq!(call(&mut p, ROOT_RELEASE, &[root]), 0);
}

#[test]
fn factory_error_precedence_and_faults_do_not_consume_device_identity() {
    let mut p = process();
    let root = root(&mut p);
    put(&mut p, DATA, 77);
    assert_eq!(call(&mut p, CREATE, &[root, 0, 0, 1]), 0x8000_4003);
    denied(&mut p, CREATE, &[root, 0, DATA, 1]);
    assert_eq!(word(&p, DATA), 77);
    assert_eq!(call(&mut p, CREATE, &[root, 0, DATA, 0]), 0x8000_4003);
    assert_eq!(word(&p, DATA), 0);
    put(&mut p, DATA, 77);
    for guid in [DEVICE, [0; 16]] {
        p.memory.write(u64::from(GUID), &guid).unwrap();
        denied(&mut p, CREATE, &[root, GUID, DATA, 0]);
        assert_eq!(word(&p, DATA), 77);
    }
    p.memory.write(u64::from(GUID), &KEYBOARD).unwrap();
    for (guid, output, outer) in [
        (GUID, 0x3000_0000, 0),
        (GUID, 0x1000_fffe, 0),
        (GUID, root, 0),
        (GUID, CODE, 0),
        (GUID, u32::MAX - 1, 0),
        (0x3000_0000, DATA, 0),
        (0x1000_fff8, DATA, 0),
        (u32::MAX - 7, DATA, 0),
        (0, CODE, 1),
    ] {
        prepare(&mut p, CREATE, &[root, guid, output, outer]);
        fault(&mut p);
        assert_eq!(word(&p, DATA), 77);
    }
    assert_eq!(create(&mut p, root), PAGE + 0x900);
}

#[test]
fn query_faults_and_aliasing_preflight_refs_and_keep_live_return_addresses() {
    let mut p = process();
    let root = root(&mut p);
    let device = create(&mut p, root);
    p.memory.write(u64::from(IID), &DEVICE).unwrap();
    put(&mut p, DATA, 77);
    for args in [[device, 0, DATA], [device, IID, 0]] {
        assert_eq!(call(&mut p, QUERY, &args), 0x8000_4003);
        assert_eq!(word(&p, DATA), 77);
    }
    for (iid, out) in [
        (0x3000_0000, DATA),
        (0x1000_fff8, DATA),
        (u32::MAX - 7, DATA),
        (IID, 0x1000_fffe),
        (IID, device),
        (IID, u32::MAX - 1),
    ] {
        prepare(&mut p, QUERY, &[device, iid, out]);
        fault(&mut p);
        assert_eq!(word(&p, DATA), 77);
    }
    assert_eq!(call(&mut p, QUERY, &[device, IID, IID + 1]), 0);
    assert_eq!(word(&p, IID + 1), device);
    assert_eq!(call(&mut p, RELEASE, &[device]), 1);
    p.memory.write(u64::from(GUID), &KEYBOARD).unwrap();
    assert_eq!(call(&mut p, CREATE, &[root, GUID, GUID + 1, 0]), 0);
    assert_eq!(word(&p, GUID + 1), device + 4);
    p.memory.write(u64::from(GUID), &KEYBOARD).unwrap();
    assert_eq!(call(&mut p, CREATE, &[root, GUID, STACK, 0]), 0);
    assert_eq!(p.cpu.eip, device + 8);
    p.memory.write(u64::from(IID), &DEVICE).unwrap();
    assert_eq!(call(&mut p, QUERY, &[device, IID, STACK + 4]), 0);
    assert_eq!(word(&p, STACK + 4), device);
    assert_eq!(call(&mut p, RELEASE, &[device]), 1);
    assert_eq!(call(&mut p, RELEASE, &[device]), 0);
}

#[test]
fn device_cap_is_separate_from_root_cap_and_released_ids_are_not_reused() {
    let mut p = process();
    let first_root = root(&mut p);
    for _ in 1..64 {
        let other = root(&mut p);
        assert_eq!(call(&mut p, ROOT_RELEASE, &[other]), 0);
    }
    assert_eq!(call(&mut p, ROOT_CREATE, &[1, 0x700, DATA, 0]), 0x8007_000e);
    for i in 0..64 {
        let device = create(&mut p, first_root);
        assert_eq!(device, PAGE + 0x900 + i * 4);
        assert_eq!(call(&mut p, RELEASE, &[device]), 0);
    }
    assert_eq!(
        call(&mut p, CREATE, &[first_root, GUID, DATA, 0]),
        0x8007_000e
    );
    assert_eq!(word(&p, DATA), 0);
    assert_eq!(call(&mut p, ROOT_RELEASE, &[first_root]), 0);
}

#[test]
fn device_creation_uses_existing_read_only_non_executable_page_at_full_budget() {
    let bytes = imported_executable::pe32(&[0xcc], "dinput.dll", &["DirectInputCreateA"]);
    let pages = process().memory.mapped_pages() + 1;
    let mut p = Process32::load(&bytes, u32::try_from(pages).unwrap()).unwrap();
    let root = root(&mut p);
    assert_eq!(p.memory.mapped_pages(), pages);
    let device = create(&mut p, root);
    assert_eq!(p.memory.mapped_pages(), pages);
    let table = word(&p, device);
    assert_eq!(table, PAGE + 0x200);
    for slot in 0..29 {
        let method = word(&p, table + slot * 4);
        assert_eq!(
            method,
            match slot {
                0 => QUERY,
                1 => ADD,
                2 => RELEASE,
                6 => 0x7000_0580,
                7 => 0x7000_0584,
                8 => 0x7000_0588,
                9 => 0x7000_05c8,
                10 => 0x7000_05cc,
                11 => 0x7000_0578,
                13 => 0x7000_057c,
                _ => 0x7000_0ffc,
            }
        );
        if slot >= 3 && !matches!(slot, 6..=11 | 13) {
            denied(&mut p, method, &[]);
        }
    }
    assert!(matches!(
        p.memory.write(u64::from(device), &[0; 4]),
        Err(MemoryError::PermissionDenied {
            access: Access::Write,
            ..
        })
    ));
    p.cpu.eip = device;
    refused(
        &mut p,
        &ProcessStop::Stopped(StopReason::MemoryFault(MemoryError::PermissionDenied {
            address: u64::from(device),
            access: Access::Execute,
        })),
    );
}

#[test]
fn zero_budget_incomplete_and_overflowing_frames_preserve_device_state() {
    let mut p = process();
    let root = root(&mut p);
    let device = create(&mut p, root);
    p.memory.write(u64::from(IID), &DEVICE).unwrap();
    p.memory
        .map_zeroed(0xffff_f000, 4096, Permissions::READ_WRITE)
        .unwrap();
    for (api, args) in [
        (CREATE, vec![root, GUID, DATA, 0]),
        (QUERY, vec![device, IID, DATA]),
        (ADD, vec![device]),
        (RELEASE, vec![device]),
    ] {
        let before = prepare(&mut p, api, &args);
        let run = p.run(0);
        assert_eq!((run.instructions, run.api_calls), (0, 0));
        assert_eq!(p.cpu, before);
        put(&mut p, 0x1000_fffc, CODE);
        p.cpu.set_register(Register32::Esp, 0x1000_fffc);
        fault(&mut p);
        let frame: Vec<_> = std::iter::once(CODE)
            .chain(args)
            .flat_map(u32::to_le_bytes)
            .collect();
        let start = (1_u64 << 32) - frame.len() as u64;
        p.memory.write(start, &frame).unwrap();
        p.cpu
            .set_register(Register32::Esp, u32::try_from(start).unwrap());
        refused(
            &mut p,
            &ProcessStop::Stopped(StopReason::MemoryFault(MemoryError::AddressOverflow)),
        );
    }
    assert_eq!(call(&mut p, RELEASE, &[device]), 0);
    assert_eq!(create(&mut p, root), device + 4);
}

#[test]
fn device_calls_require_primary_identity_but_never_touch_teb_error_storage() {
    let mut p = process();
    let root = root(&mut p);
    for fs in [0, 0x1101_0000, 0x1234_0000] {
        p.cpu.set_fs_base(fs);
        for api in [CREATE, QUERY, ADD, RELEASE] {
            p.cpu.eip = api;
            p.cpu.set_register(Register32::Esp, 0x3000_0000);
            refused(&mut p, &ProcessStop::UnsupportedApi { address: api });
        }
    }
    p.cpu.set_fs_base(0x7ffd_e000);
    put(&mut p, 0x7ffd_e034, 77);
    put(&mut p, 0x7ffd_e040, 88);
    p.memory
        .protect(0x7ffd_e000, 4096, Permissions::NONE)
        .unwrap();
    let device = create(&mut p, root);
    assert_eq!(call(&mut p, ADD, &[device]), 2);
    assert_eq!(call(&mut p, RELEASE, &[device]), 1);
    p.memory.write(u64::from(IID), &DEVICE).unwrap();
    assert_eq!(call(&mut p, QUERY, &[device, IID, DATA]), 0);
    assert_eq!(call(&mut p, CREATE, &[root, 0, DATA, 0]), 0x8000_4003);
    p.memory
        .protect(0x7ffd_e000, 4096, Permissions::READ_WRITE)
        .unwrap();
    assert_eq!(p.last_error().unwrap(), 77);
    assert_eq!(word(&p, 0x7ffd_e040), 88);
    let child = call(&mut p, 0x7000_0548, &[0, 0, CODE, 0, 4, 0]);
    assert_eq!(call(&mut p, 0x7000_0550, &[child]), 1);
    assert_eq!(
        p.run(100).reason,
        ProcessStop::Stopped(StopReason::Breakpoint)
    );
    assert_eq!(p.cpu.fs_base(), 0x1101_0000);
    for api in [CREATE, QUERY, ADD, RELEASE] {
        p.cpu.eip = api;
        p.cpu.set_register(Register32::Esp, 0x3000_0000);
        refused(&mut p, &ProcessStop::UnsupportedApi { address: api });
    }
}
