#[path = "support/dinput_root_cases.rs"]
mod dinput_root_cases;
#[path = "support/imported_executable.rs"]
mod imported_executable;

#[test]
fn imported_factory_and_vtable_lifetime_match_across_budgets() {
    dinput_root_cases::imported_root_lifetime_across_budgets();
}

use dinput_root_cases::{DATA, UNKNOWN};
use ring3_core::execution::{
    Access, Cpu32, MemoryError, Permissions, Process32, ProcessStop, Register32, StopReason,
};

const CREATE: u32 = 0x7000_0558;
const QUERY: u32 = 0x7000_055c;
const ADD: u32 = 0x7000_0560;
const RELEASE: u32 = 0x7000_0564;
const PAGE: u32 = 0x7001_7000;
const STACK: u32 = 0x1000_ef00;
const CODE: u32 = 0x0040_1000;
const IID: u32 = DATA + 64;

fn process() -> Process32 {
    Process32::load(
        &imported_executable::pe32(&[0xcc], "dinput.dll", &["DirectInputCreateA"]),
        96,
    )
    .unwrap()
}

fn put(p: &mut Process32, address: u32, value: u32) {
    p.memory
        .write(u64::from(address), &value.to_le_bytes())
        .unwrap();
}

fn word(p: &Process32, address: u32) -> u32 {
    let mut bytes = [0; 4];
    p.memory.read(u64::from(address), &mut bytes).unwrap();
    u32::from_le_bytes(bytes)
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
    assert_eq!(
        p.cpu.register(Register32::Esp),
        STACK
            + if api == 0x7000_0548 {
                4
            } else {
                u32::try_from((args.len() + 1) * 4).unwrap()
            }
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

fn fault(p: &mut Process32) {
    let before = p.cpu;
    let run = p.run(1);
    assert!(matches!(
        run.reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!((run.instructions, run.api_calls), (0, 0));
    assert_eq!(p.cpu, before);
}

fn create(p: &mut Process32) -> u32 {
    assert_eq!(call(p, CREATE, &[0x0040_0000, 0x700, DATA, 0]), 0);
    word(p, DATA)
}

#[test]
fn ansi_aliases_share_identity_and_roots_have_independent_retired_lifetimes() {
    let mut p = process();
    let before = p.memory.mapped_pages();
    let first = create(&mut p);
    assert_eq!(p.memory.mapped_pages(), before + 1);
    let second = create(&mut p);
    assert_ne!(first, second);
    for text in [
        "0000000000000000c000000000000046",
        "601352898aaacf11bfc7444553540000",
        "62e644598aaacf11bfc7444553540000",
        "84b64c9a6d23d3118e9d00c04f6844ae",
    ] {
        let bytes: Vec<_> = (0..16)
            .map(|i| u8::from_str_radix(&text[i * 2..i * 2 + 2], 16).unwrap())
            .collect();
        p.memory.write(u64::from(IID), &bytes).unwrap();
        assert_eq!(call(&mut p, QUERY, &[first, IID, DATA]), 0);
        assert_eq!(word(&p, DATA), first);
    }
    assert_eq!(call(&mut p, ADD, &[second]), 2);
    assert_eq!(call(&mut p, RELEASE, &[second]), 1);
    for value in (0..5).rev() {
        assert_eq!(call(&mut p, RELEASE, &[first]), value);
    }
    let third = create(&mut p);
    assert_ne!(third, first);
    for pointer in [0, first, first + 1, second + 2, PAGE, u32::MAX] {
        denied(&mut p, QUERY, &[pointer, 0, 0]);
        denied(&mut p, ADD, &[pointer]);
        denied(&mut p, RELEASE, &[pointer]);
    }
    assert_eq!(call(&mut p, RELEASE, &[second]), 0);
    assert_eq!(call(&mut p, RELEASE, &[third]), 0);
    assert_eq!(p.memory.mapped_pages(), before + 1);
    let table = word(&p, third);
    for slot in 0..10 {
        assert_eq!(
            word(&p, table + slot * 4),
            match slot {
                0 => QUERY,
                1 => ADD,
                2 => RELEASE,
                3 => 0x7000_0568,
                _ => 0x7000_0ffc,
            }
        );
    }
    let unsupported = word(&p, table + 16);
    denied(&mut p, unsupported, &[]);
}

#[test]
fn factory_and_query_errors_have_explicit_precedence_and_preserve_lifetime() {
    let mut p = process();
    let before = p.memory.mapped_pages();
    assert_eq!(call(&mut p, CREATE, &[0, 0, 0, 1]), 0x8000_4003);
    put(&mut p, DATA, 77);
    for args in [[1, 0x800, DATA, 0], [1, 0x700, DATA, 1], [0, 0, DATA, 0]] {
        denied(&mut p, CREATE, &args);
        assert_eq!(word(&p, DATA), 77);
    }
    assert_eq!(call(&mut p, CREATE, &[0, 0x700, DATA, 0]), 0x8007_0057);
    assert_eq!(word(&p, DATA), 0);
    assert_eq!(p.memory.mapped_pages(), before);
    let root = create(&mut p);
    for args in [[root, 0, DATA], [root, IID, 0]] {
        assert_eq!(call(&mut p, QUERY, &args), 0x8000_4003);
        assert_eq!(word(&p, DATA), root);
    }
    for iid in [
        [0_u8; 16],
        [
            0x30, 0x80, 0x79, 0xbf, 0x3a, 0x48, 0xa2, 0x4d, 0xaa, 0x99, 0x5d, 0x64, 0xed, 0x36,
            0x97, 0,
        ],
    ] {
        p.memory.write(u64::from(IID), &iid).unwrap();
        assert_eq!(call(&mut p, QUERY, &[root, IID, DATA]), 0x8000_4002);
        assert_eq!(word(&p, DATA), 0);
    }
    assert_eq!(call(&mut p, RELEASE, &[root]), 0);
}

#[test]
fn lifetime_identity_cap_does_not_reuse_released_roots() {
    let mut p = process();
    let mut roots = Vec::new();
    for _ in 0..64 {
        let root = create(&mut p);
        assert!(!roots.contains(&root));
        roots.push(root);
        assert_eq!(call(&mut p, RELEASE, &[root]), 0);
    }
    assert_eq!(call(&mut p, CREATE, &[1, 0x700, DATA, 0]), 0x8007_000e);
    assert_eq!(word(&p, DATA), 0);
    for root in roots {
        denied(&mut p, ADD, &[root]);
    }
}

#[test]
fn output_and_guid_faults_preflight_all_writes_and_reference_changes() {
    let mut p = process();
    let before = p.memory.mapped_pages();
    for output in [0x3000_0000, 0x1000_fffe, PAGE + 16, u32::MAX - 1, CODE] {
        prepare(&mut p, CREATE, &[1, 0x700, output, 0]);
        fault(&mut p);
        assert_eq!(p.memory.mapped_pages(), before);
    }
    let root = create(&mut p);
    p.memory.write(u64::from(IID), &UNKNOWN).unwrap();
    for (iid, output) in [
        (0x3000_0000, DATA),
        (0x1000_fff8, DATA),
        (IID, 0x1000_fffe),
        (IID, root),
        (u32::MAX - 7, DATA),
    ] {
        prepare(&mut p, QUERY, &[root, iid, output]);
        fault(&mut p);
        assert_eq!(word(&p, DATA), root);
    }
    assert_eq!(call(&mut p, QUERY, &[root, IID, IID + 4]), 0);
    assert_eq!(word(&p, IID + 4), root);
    assert_eq!(call(&mut p, RELEASE, &[root]), 1);
    assert_eq!(call(&mut p, RELEASE, &[root]), 0);
}

#[test]
fn lazy_mapping_collision_and_page_budget_leave_outputs_and_ids_untouched() {
    let mut p = process();
    p.memory
        .map_zeroed(u64::from(PAGE), 4096, Permissions::READ_WRITE)
        .unwrap();
    put(&mut p, PAGE, 0xfeed);
    put(&mut p, DATA, 77);
    prepare(&mut p, CREATE, &[1, 0x700, DATA, 0]);
    refused(
        &mut p,
        &ProcessStop::Stopped(StopReason::MemoryFault(MemoryError::AlreadyMapped {
            address: u64::from(PAGE),
        })),
    );
    assert_eq!(word(&p, DATA), 77);
    assert_eq!(word(&p, PAGE), 0xfeed);
    let pages = process().memory.mapped_pages();
    let bytes = imported_executable::pe32(&[0xcc], "dinput.dll", &["DirectInputCreateA"]);
    let mut full = Process32::load(&bytes, u32::try_from(pages).unwrap()).unwrap();
    put(&mut full, DATA, 77);
    prepare(&mut full, CREATE, &[1, 0x700, DATA, 0]);
    refused(
        &mut full,
        &ProcessStop::Stopped(StopReason::MemoryFault(MemoryError::PageLimitExceeded)),
    );
    assert_eq!(word(&full, DATA), 77);
    assert_eq!(full.memory.mapped_pages(), pages);
}

#[test]
fn zero_budget_incomplete_and_overflowing_frames_preserve_root_state() {
    let mut p = process();
    let root = create(&mut p);
    p.memory.write(u64::from(IID), &UNKNOWN).unwrap();
    p.memory
        .map_zeroed(0xffff_f000, 4096, Permissions::READ_WRITE)
        .unwrap();
    for (api, args) in [
        (CREATE, vec![1, 0x700, DATA, 0]),
        (QUERY, vec![root, IID, DATA]),
        (ADD, vec![root]),
        (RELEASE, vec![root]),
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
    assert_eq!(call(&mut p, RELEASE, &[root]), 0);
    let second = create(&mut p);
    assert_eq!(second, root + 4);
}

#[test]
fn primary_identity_guard_precedes_frames_and_success_needs_no_teb_memory() {
    let mut p = process();
    let root = create(&mut p);
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
    assert_eq!(call(&mut p, ADD, &[root]), 2);
    assert_eq!(call(&mut p, RELEASE, &[root]), 1);
    p.memory.write(u64::from(IID), &UNKNOWN).unwrap();
    assert_eq!(call(&mut p, QUERY, &[root, IID, DATA]), 0);
    create(&mut p);
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

#[test]
fn identity_page_is_read_only_non_executable_and_stack_aliases_remain_defined() {
    let mut p = process();
    let root = create(&mut p);
    assert!(matches!(
        p.memory.write(u64::from(root), &[0; 4]),
        Err(MemoryError::PermissionDenied {
            access: Access::Write,
            ..
        })
    ));
    p.cpu.eip = root;
    refused(
        &mut p,
        &ProcessStop::Stopped(StopReason::MemoryFault(MemoryError::PermissionDenied {
            address: u64::from(root),
            access: Access::Execute,
        })),
    );
    assert_eq!(call(&mut p, CREATE, &[1, 0x700, STACK, 0]), 0);
    assert_eq!(p.cpu.eip, word(&p, STACK));
    p.memory.write(u64::from(IID), &UNKNOWN).unwrap();
    assert_eq!(call(&mut p, QUERY, &[root, IID, STACK + 4]), 0);
    assert_eq!(word(&p, STACK + 4), root);
    assert_eq!(call(&mut p, RELEASE, &[root]), 1);
}

#[test]
fn dynamic_lookup_exposes_only_named_ansi_factory() {
    use ring3_core::execution::LoadError;
    let mut p = process();
    p.memory.write(u64::from(DATA), b"DiNpUt.DlL\0").unwrap();
    assert_eq!(call(&mut p, 0x7000_0018, &[DATA]), 0x7000_081c);
    assert_eq!(call(&mut p, 0x7000_0014, &[DATA]), 0x7000_081c);
    for (name, expected) in [
        ("DirectInputCreateA", Some(CREATE)),
        ("directinputcreatea", None),
        ("DirectInputCreateW", None),
        ("DirectInput8Create", None),
    ] {
        let bytes = [name.as_bytes(), &[0]].concat();
        p.memory.write(u64::from(IID), &bytes).unwrap();
        if let Some(expected) = expected {
            assert_eq!(call(&mut p, 0x7000_0274, &[0x7000_081c, IID]), expected);
        } else {
            denied(&mut p, 0x7000_0274, &[0x7000_081c, IID]);
        }
    }
    denied(&mut p, 0x7000_0274, &[0x7000_081c, 1]);
    assert_eq!(call(&mut p, 0x7000_001c, &[0x7000_081c]), 1);
    for name in [
        "DirectInputCreateW",
        "DirectInputCreateEx",
        "DirectInput8Create",
    ] {
        assert!(matches!(
            Process32::load(
                &imported_executable::pe32(&[0xcc], "dinput.dll", &[name]),
                64
            ),
            Err(LoadError::UnresolvedImport { .. })
        ));
    }
}

#[test]
fn images_covering_input_page_obey_reserved_region_placement() {
    use ring3_core::execution::{GuestModule, LoadError, ProcessOptions};
    let program = imported_executable::pe32(&[0xcc], "dinput.dll", &["DirectInputCreateA"]);
    let mut overlap = program.clone();
    overlap[0xb4..0xb8].copy_from_slice(&0x7001_0000_u32.to_le_bytes());
    overlap[0xd0..0xd4].copy_from_slice(&0x8000_u32.to_le_bytes());
    assert!(
        matches!(Process32::load(&overlap, 64), Err(LoadError::Memory(MemoryError::AlreadyMapped { address })) if address == 0x7001_0000)
    );
    overlap[0x96..0x98].copy_from_slice(&0x2102_u16.to_le_bytes());
    overlap[0x100..0x108].fill(0);
    let modules = [GuestModule {
        name: "demo.dll",
        bytes: &overlap,
    }];
    for deferred in [false, true] {
        let options = if deferred {
            ProcessOptions {
                deferred_modules: &modules,
                ..ProcessOptions::default()
            }
        } else {
            ProcessOptions {
                modules: &modules,
                ..ProcessOptions::default()
            }
        };
        assert!(matches!(
            Process32::load_with_options(&program, 64, options),
            Err(LoadError::RelocationRequired)
        ));
    }
}
