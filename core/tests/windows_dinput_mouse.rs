#[allow(dead_code)]
#[path = "support/dinput_device_calls.rs"]
mod dinput_device_calls;
#[allow(dead_code)]
#[path = "support/dinput_device_cases.rs"]
mod dinput_device_cases;
#[path = "support/dinput_mouse_cases.rs"]
mod dinput_mouse_cases;
#[path = "support/imported_executable.rs"]
mod imported_executable;

use dinput_device_calls::*;
use dinput_device_cases::DEVICE;
use dinput_mouse_cases::MOUSE;
use ring3_core::execution::Access;

const MOUSE_QUERY: u32 = 0x7000_058c;
const MOUSE_ADD: u32 = 0x7000_0590;
const MOUSE_RELEASE: u32 = 0x7000_0594;

fn mouse(p: &mut Process32, root: u32) -> u32 {
    p.memory.write(u64::from(GUID), &MOUSE).unwrap();
    assert_eq!(call(p, CREATE, &[root, GUID, DATA + 5, 0]), 0);
    word(p, DATA + 5)
}

#[test]
fn imported_mouse_vtable_lifetime_survives_root_across_budgets() {
    dinput_mouse_cases::imported_mouse_lifetime_across_budgets();
}

#[test]
fn mouse_aliases_survive_root_and_keep_keyboard_and_other_mouse_refs_independent() {
    let mut p = process();
    let root = root(&mut p);
    let keyboard = create(&mut p, root);
    let first = mouse(&mut p, root);
    let second = mouse(&mut p, root);
    assert_eq!(
        (keyboard, first, second),
        (PAGE + 0x900, PAGE + 0xa00, PAGE + 0xa04)
    );
    assert_eq!(call(&mut p, ROOT_RELEASE, &[root]), 0);
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
        assert_eq!(call(&mut p, MOUSE_QUERY, &[first, IID, DATA]), 0);
        assert_eq!(word(&p, DATA), first);
    }
    for count in (0..5).rev() {
        assert_eq!(call(&mut p, MOUSE_RELEASE, &[first]), count);
    }
    assert_eq!(call(&mut p, MOUSE_ADD, &[second]), 2);
    assert_eq!(call(&mut p, MOUSE_RELEASE, &[second]), 1);
    assert_eq!(call(&mut p, RELEASE, &[keyboard]), 0);
    assert_eq!(call(&mut p, MOUSE_RELEASE, &[second]), 0);
}

#[test]
fn mouse_interfaces_and_method_receivers_never_reinterpret_other_kinds() {
    let mut p = process();
    let root = root(&mut p);
    let keyboard = create(&mut p, root);
    let mouse = mouse(&mut p, root);
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
        put(&mut p, DATA, 77);
        assert_eq!(call(&mut p, MOUSE_QUERY, &[mouse, IID, DATA]), 0x8000_4002);
        assert_eq!(word(&p, DATA), 0);
    }
    for pointer in [0, root, keyboard, mouse + 1, mouse + 4, PAGE, u32::MAX] {
        denied(&mut p, MOUSE_QUERY, &[pointer, 0, 0]);
        denied(&mut p, MOUSE_ADD, &[pointer]);
        denied(&mut p, MOUSE_RELEASE, &[pointer]);
    }
    for (api, args) in [
        (ROOT_QUERY, vec![mouse, 0, 0]),
        (ROOT_RELEASE, vec![mouse]),
        (CREATE, vec![mouse, 0, 0, 0]),
        (QUERY, vec![mouse, 0, 0]),
        (ADD, vec![mouse]),
        (RELEASE, vec![mouse]),
        (0x7000_0578, vec![mouse, 0]),
        (0x7000_057c, vec![mouse, 0, 0]),
        (0x7000_0580, vec![mouse, 0, 0]),
        (0x7000_0584, vec![mouse]),
        (0x7000_0588, vec![mouse]),
    ] {
        denied(&mut p, api, &args);
    }
    assert_eq!(call(&mut p, MOUSE_RELEASE, &[mouse]), 0);
    denied(&mut p, MOUSE_QUERY, &[mouse, 0, 0]);
    denied(&mut p, MOUSE_ADD, &[mouse]);
    denied(&mut p, MOUSE_RELEASE, &[mouse]);
    assert_eq!(call(&mut p, RELEASE, &[keyboard]), 0);
}

#[test]
fn mouse_factory_and_query_failures_recover_without_consuming_identity_or_refs() {
    let mut p = process();
    let root = root(&mut p);
    p.memory.write(u64::from(GUID), &MOUSE).unwrap();
    put(&mut p, DATA, 77);
    assert_eq!(call(&mut p, CREATE, &[root, GUID, 0, 1]), 0x8000_4003);
    denied(&mut p, CREATE, &[root, GUID, DATA, 1]);
    assert_eq!(word(&p, DATA), 77);
    assert_eq!(call(&mut p, CREATE, &[root, 0, DATA, 0]), 0x8000_4003);
    assert_eq!(word(&p, DATA), 0);
    put(&mut p, DATA, 77);
    for (guid, output) in [
        (GUID, 0x1000_fffe),
        (GUID, root),
        (GUID, u32::MAX - 1),
        (0x1000_fff8, DATA),
        (u32::MAX - 7, DATA),
    ] {
        prepare(&mut p, CREATE, &[root, guid, output, 0]);
        fault(&mut p);
        assert_eq!(word(&p, DATA), 77);
    }
    assert_eq!(call(&mut p, CREATE, &[root, GUID, GUID + 1, 0]), 0);
    let device = word(&p, GUID + 1);
    assert_eq!(device, PAGE + 0xa00);
    p.memory.write(u64::from(IID), &DEVICE).unwrap();
    for args in [[device, 0, DATA], [device, IID, 0]] {
        assert_eq!(call(&mut p, MOUSE_QUERY, &args), 0x8000_4003);
        assert_eq!(word(&p, DATA), 77);
    }
    for (iid, output) in [(0x1000_fff8, DATA), (IID, 0x1000_fffe), (IID, device)] {
        prepare(&mut p, MOUSE_QUERY, &[device, iid, output]);
        fault(&mut p);
        assert_eq!(word(&p, DATA), 77);
    }
    assert_eq!(call(&mut p, MOUSE_QUERY, &[device, IID, IID + 1]), 0);
    assert_eq!(word(&p, IID + 1), device);
    assert_eq!(call(&mut p, MOUSE_RELEASE, &[device]), 1);
    p.memory.write(u64::from(IID), &DEVICE).unwrap();
    assert_eq!(call(&mut p, MOUSE_QUERY, &[device, IID, STACK]), 0);
    assert_eq!(p.cpu.eip, device);
    assert_eq!(call(&mut p, MOUSE_RELEASE, &[device]), 1);
    assert_eq!(call(&mut p, MOUSE_RELEASE, &[device]), 0);
    assert_eq!(mouse(&mut p, root), device + 4);
    assert_eq!(create(&mut p, root), PAGE + 0x900);
}

#[test]
fn each_kind_has_its_own_nonreused_identity_limit() {
    let mut p = process();
    let first_root = root(&mut p);
    for _ in 1..64 {
        let other = root(&mut p);
        assert_eq!(call(&mut p, ROOT_RELEASE, &[other]), 0);
    }
    assert_eq!(call(&mut p, ROOT_CREATE, &[1, 0x700, DATA, 0]), 0x8007_000e);
    for i in 0..64 {
        let device = mouse(&mut p, first_root);
        assert_eq!(device, PAGE + 0xa00 + i * 4);
        assert_eq!(call(&mut p, MOUSE_RELEASE, &[device]), 0);
    }
    assert_eq!(
        call(&mut p, CREATE, &[first_root, GUID, DATA, 0]),
        0x8007_000e
    );
    assert_eq!(word(&p, DATA), 0);
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
fn mouse_uses_read_only_nonexecutable_table_at_full_page_budget() {
    let bytes = imported_executable::pe32(&[0xcc], "dinput.dll", &["DirectInputCreateA"]);
    let pages = process().memory.mapped_pages() + 1;
    let mut p = Process32::load(&bytes, u32::try_from(pages).unwrap()).unwrap();
    let root = root(&mut p);
    let device = mouse(&mut p, root);
    assert_eq!(p.memory.mapped_pages(), pages);
    let table = word(&p, device);
    assert_eq!(table, PAGE + 0x300);
    for slot in 0..29 {
        let method = word(&p, table + slot * 4);
        assert_eq!(
            method,
            match slot {
                0 => MOUSE_QUERY,
                1 => MOUSE_ADD,
                2 => MOUSE_RELEASE,
                11 => 0x7000_0598,
                3 => 0x7000_05a0,
                5 => 0x7000_05a8,
                6 => 0x7000_05a4,
                7 => 0x7000_05ac,
                8 => 0x7000_05b0,
                9 => 0x7000_05d0,
                13 => 0x7000_059c,
                _ => 0x7000_0ffc,
            }
        );
        if slot >= 3 && !matches!(slot, 11 | 13) {
            denied(&mut p, method, &[]);
        }
    }
    for address in [table, device] {
        assert!(matches!(
            p.memory.write(u64::from(address), &[0; 4]),
            Err(MemoryError::PermissionDenied {
                access: Access::Write,
                ..
            })
        ));
        p.cpu.eip = address;
        refused(
            &mut p,
            &ProcessStop::Stopped(StopReason::MemoryFault(MemoryError::PermissionDenied {
                address: u64::from(address),
                access: Access::Execute,
            })),
        );
    }
}

#[test]
fn mouse_frames_and_zero_budget_preflight_before_mutation() {
    let mut p = process();
    let root = root(&mut p);
    let device = mouse(&mut p, root);
    p.memory.write(u64::from(IID), &DEVICE).unwrap();
    p.memory
        .map_zeroed(0xffff_f000, 4096, Permissions::READ_WRITE)
        .unwrap();
    for (api, args) in [
        (CREATE, vec![root, GUID, DATA, 0]),
        (MOUSE_QUERY, vec![device, IID, DATA]),
        (MOUSE_ADD, vec![device]),
        (MOUSE_RELEASE, vec![device]),
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
    assert_eq!(call(&mut p, MOUSE_RELEASE, &[device]), 0);
    assert_eq!(mouse(&mut p, root), device + 4);
}

#[test]
fn mouse_calls_reject_child_identity_and_preserve_error_storage() {
    let mut p = process();
    let root = root(&mut p);
    for fs in [0, 0x1101_0000, 0x1234_0000] {
        p.cpu.set_fs_base(fs);
        for api in [CREATE, MOUSE_QUERY, MOUSE_ADD, MOUSE_RELEASE] {
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
    let device = mouse(&mut p, root);
    assert_eq!(call(&mut p, MOUSE_ADD, &[device]), 2);
    p.memory.write(u64::from(IID), &DEVICE).unwrap();
    assert_eq!(call(&mut p, MOUSE_QUERY, &[device, IID, DATA]), 0);
    for count in (0..3).rev() {
        assert_eq!(call(&mut p, MOUSE_RELEASE, &[device]), count);
    }
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
    for api in [CREATE, MOUSE_QUERY, MOUSE_ADD, MOUSE_RELEASE] {
        p.cpu.eip = api;
        p.cpu.set_register(Register32::Esp, 0x3000_0000);
        refused(&mut p, &ProcessStop::UnsupportedApi { address: api });
    }
}
